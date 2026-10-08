//! panel: the task bar at the bottom of the screen (like tint2 or
//! i3bar): an applications menu, the window list, status text from the
//! window manager (workspaces) and a clock.

#![no_std]
#![no_main]

use huldra_user::gui::*;
use huldra_user::time::{self, DateTime};
use huldra_user::{eprintln, format, String, Vec};

huldra_user::main!(main);

const HEIGHT: i32 = 26;
const BG_TOP: u32 = 0xFF3A3F48;
const BG_BOTTOM: u32 = 0xFF1E2228;
const TEXT: u32 = 0xFFE8E8E8;
const ACTIVE: u32 = 0xFF4E6E9A;
const MENU_W: i32 = 92;
const CLOCK_W: i32 = 80;

const APPS: &[(&str, &str)] = &[("Terminal", "term"), ("Files", "files"), ("Clock", "clock"), ("Calculator", "calc"), ("Paint", "paint"), ("System info", "sysinfo"), ("Run...", "menu")];

struct Item {
    id: u32,
    title: String,
    mapped: bool,
}

struct Panel {
    d: Display,
    win: u32,
    font: Font,
    width: i32,
    items: Vec<Item>,
    focus: u32,
    status: String,
    menu: Option<(u32, Rect, Option<usize>)>,
}

impl Panel {
    fn task_area(&self) -> (i32, i32) {
        let status_w = if self.status.is_empty() { 0 } else { self.font.text_width(&self.status) + 16 };
        (MENU_W + 4 + status_w, self.width - CLOCK_W - 4)
    }

    /// Positions of the window buttons.
    fn buttons(&self) -> Vec<(usize, Rect)> {
        let (left, right) = self.task_area();
        let n = self.items.len() as i32;
        if n == 0 {
            return Vec::new();
        }
        let w = ((right - left) / n).min(200);
        self.items.iter().enumerate().map(|(i, _)| (i, Rect::new(left + i as i32 * w, 3, w - 4, HEIGHT - 6))).collect()
    }

    fn draw(&mut self) {
        let mut c = Canvas::new(self.width, HEIGHT);
        c.gradient(c.bounds(), BG_TOP, BG_BOTTOM);
        c.fill_rect(Rect::new(0, 0, self.width, 1), 0xFF5A606A);
        // Menu button.
        let mb = Rect::new(3, 3, MENU_W - 6, HEIGHT - 6);
        c.fill_round_rect(mb, 4, if self.menu.is_some() { ACTIVE } else { 0xFF2C6E49 });
        c.draw_text_bold(&self.font, mb.x + 10, 5, "Huldra", TEXT);
        // Status (workspaces).
        if !self.status.is_empty() {
            c.draw_text(&self.font, MENU_W + 8, 5, &self.status, 0xFFFFD27A, None);
        }
        // Windows.
        for (i, r) in self.buttons() {
            let it = &self.items[i];
            let bg = if it.id == self.focus { ACTIVE } else if it.mapped { 0xFF454B55 } else { 0xFF2A2E35 };
            c.fill_round_rect(r, 3, bg);
            let max = ((r.w - 12) / FONT_W).max(0) as usize;
            let mut t: String = it.title.chars().take(max).collect();
            if t.is_empty() {
                t = String::from("(untitled)");
            }
            c.set_clip(r);
            c.draw_text(&self.font, r.x + 6, 5, &t, if it.mapped { TEXT } else { 0xFF9A9A9A }, None);
            c.reset_clip();
        }
        self.draw_clock(&mut c);
        let win = self.win;
        self.d.put_canvas(win, &c, c.bounds(), 0, 0);
        self.d.flush();
    }

    fn draw_clock(&self, c: &mut Canvas) {
        let t = DateTime::from_unix(time::now());
        let s = format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second);
        let x = self.width - CLOCK_W;
        c.gradient(Rect::new(x, 1, CLOCK_W, HEIGHT - 1), BG_TOP, BG_BOTTOM);
        c.draw_text(&self.font, x + (CLOCK_W - self.font.text_width(&s)) / 2, 5, &s, TEXT, None);
    }

    fn tick(&mut self) {
        let mut c = Canvas::new(self.width, HEIGHT);
        c.gradient(c.bounds(), BG_TOP, BG_BOTTOM);
        self.draw_clock(&mut c);
        let x = self.width - CLOCK_W;
        let win = self.win;
        self.d.put_canvas(win, &c, Rect::new(x, 1, CLOCK_W, HEIGHT - 1), x, 1);
        self.d.flush();
    }

    fn toggle_menu(&mut self) {
        if let Some((w, _, _)) = self.menu.take() {
            self.d.destroy(w);
            self.draw();
            return;
        }
        let w = 180;
        let h = APPS.len() as i32 * 24 + 8;
        let r = Rect::new(2, self.d.height - HEIGHT - h, w, h);
        let win = self.d.create_window(r.x, r.y, r.w, r.h, KIND_POPUP);
        self.menu = Some((win, r, None));
        self.draw_menu();
        self.d.map(win);
        self.draw();
    }

    fn draw_menu(&mut self) {
        let Some((win, r, hover)) = self.menu else { return };
        let mut c = Canvas::new(r.w, r.h);
        c.gradient(c.bounds(), 0xFF2E333B, 0xFF22262C);
        c.rect_outline(c.bounds(), 0xFF5A606A);
        for (i, (label, _)) in APPS.iter().enumerate() {
            let row = Rect::new(4, 4 + i as i32 * 24, r.w - 8, 24);
            if hover == Some(i) {
                c.fill_round_rect(row, 3, ACTIVE);
            }
            c.draw_text(&self.font, row.x + 10, row.y + 4, label, TEXT, None);
        }
        self.d.put_canvas(win, &c, c.bounds(), 0, 0);
        self.d.flush();
    }

    fn menu_index(&self, y: i32) -> Option<usize> {
        let i = (y - 4) / 24;
        (y >= 4 && (i as usize) < APPS.len()).then_some(i as usize)
    }

    fn handle(&mut self, ev: Event) {
        match ev {
            Event::Expose { id, .. } if id == self.win => self.draw(),
            Event::WindowListItem { id, title, mapped } => {
                match self.items.iter_mut().find(|i| i.id == id) {
                    Some(it) => {
                        it.title = title;
                        it.mapped = mapped;
                    }
                    None => self.items.push(Item { id, title, mapped }),
                }
                self.draw();
            }
            Event::WindowRemoved { id } => {
                self.items.retain(|i| i.id != id);
                self.draw();
            }
            Event::FocusChanged { id } => {
                self.focus = id;
                self.draw();
            }
            Event::Status { text } => {
                self.status = text;
                self.draw();
            }
            Event::Button { id, x, y, button: BUTTON_LEFT, pressed: true, .. } => {
                if id == self.win {
                    if x < MENU_W {
                        self.toggle_menu();
                        return;
                    }
                    let hit = self.buttons().into_iter().find(|(_, r)| r.contains(x, y)).map(|(i, _)| self.items[i].id);
                    if let Some(w) = hit {
                        self.d.send(Request::Activate { id: w });
                        self.d.flush();
                    }
                }
            }
            Event::Button { id, y, button: BUTTON_LEFT, pressed: false, .. } => {
                if self.menu.is_some_and(|m| m.0 == id) {
                    if let Some(i) = self.menu_index(y) {
                        spawn(APPS[i].1);
                    }
                    self.toggle_menu();
                }
            }
            Event::Motion { id, y, .. } => {
                if let Some((w, r, hover)) = self.menu {
                    if w == id {
                        let h = self.menu_index(y);
                        if h != hover {
                            self.menu = Some((w, r, h));
                            self.draw_menu();
                        }
                    }
                }
            }
            Event::Crossing { id, entered: false } => {
                if let Some((w, r, Some(_))) = self.menu {
                    if w == id {
                        self.menu = Some((w, r, None));
                        self.draw_menu();
                    }
                }
            }
            _ => {}
        }
    }
}

fn main() -> i32 {
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("panel: cannot connect to the display: {}", e);
            return 1;
        }
    };
    let width = d.width;
    let win = d.create_window(0, d.height - HEIGHT, width, HEIGHT, KIND_DOCK);
    d.map(win);
    d.send(Request::SelectWindows);
    d.flush();
    let mut p = Panel { d, win, font: load_font(), width, items: Vec::new(), focus: 0, status: String::new(), menu: None };
    p.draw();
    let mut last = time::now();
    loop {
        match p.d.wait_event(500) {
            Some(ev) => p.handle(ev),
            None if p.d.closed => return 0,
            None => {}
        }
        let now = time::now();
        if now != last {
            last = now;
            p.tick();
        }
        reap_children();
    }
}
