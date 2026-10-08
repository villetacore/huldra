//! files [dir]: a graphical file manager. Double-click (or Enter) opens a
//! directory, views a text file in a terminal (`less`), or runs a program.
//! Backspace goes up; the wheel and arrow keys scroll.

#![no_std]
#![no_main]

use huldra_gfx::keymap;
use huldra_user::fs::{self, DirEntry};
use huldra_user::gui::*;
use huldra_user::{env, eprintln, format, String, Vec};

huldra_user::main!(main);

const ROW: i32 = 20;
const HEAD: i32 = 30;

struct Files {
    d: Display,
    win: u32,
    font: Font,
    w: i32,
    h: i32,
    cwd: String,
    entries: Vec<(String, bool, u64, bool)>, // name, dir, size, executable
    sel: usize,
    top: usize,
    last_click: (usize, u64),
}

fn human(n: u64) -> String {
    if n < 1024 {
        format!("{} B", n)
    } else if n < 1024 * 1024 {
        format!("{}.{} K", n / 1024, n % 1024 * 10 / 1024)
    } else {
        format!("{}.{} M", n >> 20, (n & 0xFFFFF) * 10 >> 20)
    }
}

impl Files {
    fn load(&mut self, dir: &str) {
        let mut v: Vec<DirEntry> = fs::read_dir(dir).unwrap_or_default();
        v.sort_by(|a, b| b.is_dir().cmp(&a.is_dir()).then(a.name.cmp(&b.name)));
        self.cwd = String::from(dir);
        self.entries.clear();
        if dir != "/" {
            self.entries.push((String::from(".."), true, 0, false));
        }
        for e in v {
            let path = fs::join(dir, &e.name);
            let st = fs::metadata(&path).ok();
            let size = st.as_ref().map_or(0, |s| s.st_size as u64);
            let exec = st.as_ref().is_some_and(|s| s.st_mode & 0o111 != 0);
            self.entries.push((e.name.clone(), e.is_dir(), size, exec));
        }
        self.sel = 0;
        self.top = 0;
        let title = format!("Files - {}", self.cwd);
        let win = self.win;
        self.d.set_title(win, &title);
    }

    fn visible_rows(&self) -> usize {
        ((self.h - HEAD) / ROW).max(1) as usize
    }

    fn draw(&mut self) {
        let mut c = Canvas::new(self.w, self.h);
        c.fill_rect(c.bounds(), 0xFFFFFFFF);
        c.gradient(Rect::new(0, 0, self.w, HEAD), 0xFFEDEFF2, 0xFFD5D9DF);
        c.fill_rect(Rect::new(0, HEAD - 1, self.w, 1), 0xFFA0A4AA);
        c.draw_text_bold(&self.font, 10, 7, &self.cwd, 0xFF202830);
        let rows = self.visible_rows();
        for (i, (name, dir, size, exec)) in self.entries.iter().enumerate().skip(self.top).take(rows) {
            let y = HEAD + (i - self.top) as i32 * ROW;
            if i == self.sel {
                c.fill_rect(Rect::new(0, y, self.w, ROW), 0xFF3D6FB4);
            } else if i % 2 == 1 {
                c.fill_rect(Rect::new(0, y, self.w, ROW), 0xFFF4F6F9);
            }
            let fg = if i == self.sel { 0xFFFFFFFF } else { 0xFF202020 };
            // Icon: a folder or a page.
            if *dir {
                c.fill_rect(Rect::new(10, y + 6, 16, 10), 0xFFE8B84A);
                c.fill_rect(Rect::new(10, y + 4, 7, 3), 0xFFE8B84A);
            } else {
                c.fill_rect(Rect::new(12, y + 3, 12, 15), if *exec { 0xFF7CB342 } else { 0xFFDADDE2 });
                c.rect_outline(Rect::new(12, y + 3, 12, 15), 0xFF8A8E94);
            }
            c.draw_text(&self.font, 34, y + 2, name, fg, None);
            if !dir {
                let s = human(*size);
                c.draw_text(&self.font, self.w - 12 - self.font.text_width(&s), y + 2, &s, if i == self.sel { fg } else { 0xFF707070 }, None);
            }
        }
        let win = self.win;
        self.d.put_canvas(win, &c, c.bounds(), 0, 0);
        self.d.flush();
    }

    fn open(&mut self, i: usize) {
        let Some((name, dir, _, exec)) = self.entries.get(i).cloned() else { return };
        let path = if name == ".." {
            match self.cwd.rfind('/') {
                Some(0) | None => String::from("/"),
                Some(p) => String::from(&self.cwd[..p]),
            }
        } else {
            fs::join(&self.cwd, &name)
        };
        if dir {
            self.load(&path);
        } else if exec {
            spawn(&format!("term -e {}", path));
        } else {
            spawn(&format!("term -e less {}", path));
        }
    }

    fn ensure_visible(&mut self) {
        let rows = self.visible_rows();
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + rows {
            self.top = self.sel + 1 - rows;
        }
    }
}

fn main() -> i32 {
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("files: {}", e);
            return 1;
        }
    };
    let (w, h) = (460, 420);
    let win = d.create_window(0, 0, w, h, KIND_NORMAL);
    d.map(win);
    let start = env::args().get(1).cloned().unwrap_or_else(|| String::from(env::var("HOME").unwrap_or("/")));
    let mut f = Files { d, win, font: load_font(), w, h, cwd: String::new(), entries: Vec::new(), sel: 0, top: 0, last_click: (usize::MAX, 0) };
    f.load(&start);
    while let Some(ev) = f.d.wait_event(-1) {
        match ev {
            Event::Expose { w, h, .. } => {
                f.w = w;
                f.h = h;
            }
            Event::Button { y, button: BUTTON_LEFT, pressed: true, .. } if y >= HEAD => {
                let i = f.top + ((y - HEAD) / ROW) as usize;
                if i < f.entries.len() {
                    let t = huldra_user::time::uptime_ms();
                    if f.last_click.0 == i && t - f.last_click.1 < 500 {
                        f.last_click = (usize::MAX, 0);
                        f.open(i);
                    } else {
                        f.last_click = (i, t);
                        f.sel = i;
                    }
                }
            }
            Event::Button { button, pressed: true, .. } if button == WHEEL_UP || button == WHEEL_DOWN => {
                let max = f.entries.len().saturating_sub(f.visible_rows());
                f.top = if button == WHEEL_UP { f.top.saturating_sub(3) } else { (f.top + 3).min(max) };
            }
            Event::Key { code, pressed: true, .. } => match code {
                keymap::UP => {
                    f.sel = f.sel.saturating_sub(1);
                    f.ensure_visible();
                }
                keymap::DOWN => {
                    f.sel = (f.sel + 1).min(f.entries.len().saturating_sub(1));
                    f.ensure_visible();
                }
                keymap::ENTER | keymap::KP_ENTER => {
                    let i = f.sel;
                    f.open(i);
                }
                keymap::BACKSPACE => {
                    if f.entries.first().is_some_and(|e| e.0 == "..") {
                        f.open(0);
                    }
                }
                _ => continue,
            },
            Event::CloseRequest { .. } => return 0,
            _ => continue,
        }
        f.draw();
        reap_children();
    }
    0
}
