//! web [URL]: the graphical web browser. Same engine as `browse`, in a
//! window: click links, scroll with the wheel, type an address (or search
//! words) in the bar at the top.
//!
//!   Ctrl+L or click the bar   edit the address    Enter   go
//!   Alt+Left / Backspace      back                Alt+Right  forward
//!   F5                        reload              Tab   next link or field
//!   Ctrl+S                    save the page or the selected link

#![no_std]
#![no_main]

use huldra_gfx::keymap;
use huldra_user::browser::{Browser, Target};
use huldra_user::gui::*;
use huldra_user::{env, eprintln, format, String, ToString};
use huldra_web::layout::{BOLD, CODE, DIM, FIELD, HEADING, ITALIC, LINK};

huldra_user::main!(main);

const BAR: i32 = 30;
const STATUS: i32 = 20;
const PAD: i32 = 8;
const BUTTONS: [&str; 4] = ["<", ">", "R", "H"];
const BW: i32 = 26;

/// What the keyboard is typing into.
enum Edit {
    None,
    Address(String),
    Field(usize, String),
}

struct App {
    b: Browser,
    w: i32,
    h: i32,
    edit: Edit,
    hover: Option<String>,
    loading: Option<String>,
}

impl App {
    fn rows(&self) -> usize {
        ((self.h - BAR - STATUS) / FONT_H).max(1) as usize
    }

    fn cols(&self) -> usize {
        ((self.w - 2 * PAD) / FONT_W).max(20) as usize
    }

    fn button_rect(i: usize) -> Rect {
        Rect::new(4 + i as i32 * (BW + 4), 3, BW, BAR - 6)
    }

    fn address_rect(&self) -> Rect {
        let x = 4 + BUTTONS.len() as i32 * (BW + 4) + 4;
        Rect::new(x, 3, self.w - x - 6, BAR - 6)
    }

    fn draw(&self, d: &mut Display, win: u32, font: &Font) {
        let mut c = Canvas::new(self.w, self.h);
        c.fill_rect(c.bounds(), theme::BG);
        // Toolbar.
        c.fill_rect(Rect::new(0, 0, self.w, BAR), theme::SURFACE);
        c.line(0, BAR - 1, self.w, BAR - 1, theme::LINE_DIM);
        for (i, label) in BUTTONS.iter().enumerate() {
            theme::button(&mut c, font, Self::button_rect(i), label, false);
        }
        let ar = self.address_rect();
        c.fill_rect(ar, theme::BG);
        c.rect_outline(ar, if matches!(self.edit, Edit::Address(_)) { theme::ACCENT } else { theme::LINE });
        let addr = match &self.edit {
            Edit::Address(s) => format!("{}_", s),
            _ => self.b.doc.location.to_string(),
        };
        let max = ((ar.w - 12) / FONT_W) as usize;
        let shown: String = if addr.chars().count() > max { addr.chars().skip(addr.chars().count() - max).collect() } else { addr };
        c.draw_text(font, ar.x + 6, ar.y + (ar.h - FONT_H) / 2, &shown, theme::BRIGHT, None);
        // Page.
        let rows = self.rows();
        for r in 0..rows {
            let Some(line) = self.b.doc.page.lines.get(self.b.top + r) else { break };
            let y = BAR + 2 + r as i32 * FONT_H;
            let mut x = PAD;
            for span in line {
                let w = span.text.chars().count() as i32 * FONT_W;
                let selected = match self.b.selected {
                    Some(Target::Link(n)) => span.link == Some(n) && span.field.is_none(),
                    Some(Target::Field(n)) => span.field == Some(n),
                    None => false,
                };
                let fg = if span.style & HEADING != 0 {
                    theme::BRIGHT
                } else if span.style & FIELD != 0 {
                    theme::AMBER
                } else if span.style & LINK != 0 {
                    theme::ACCENT
                } else if span.style & CODE != 0 {
                    theme::AMBER
                } else if span.style & (DIM | ITALIC) != 0 {
                    theme::TEXT_DIM
                } else {
                    theme::TEXT
                };
                if selected {
                    c.fill_rect(Rect::new(x, y, w, FONT_H), theme::HOVER);
                }
                if span.style & FIELD != 0 {
                    c.fill_rect(Rect::new(x, y + 1, w, FONT_H - 2), theme::RAISED);
                }
                if span.style & (BOLD | HEADING) != 0 {
                    c.draw_text_bold(font, x, y, &span.text, fg);
                } else {
                    c.draw_text(font, x, y, &span.text, fg, None);
                }
                if span.style & LINK != 0 && span.text.trim() != "" {
                    c.line(x, y + FONT_H - 2, x + w - 1, y + FONT_H - 2, theme::dim(theme::ACCENT, 160));
                }
                x += w;
            }
        }
        // Scroll bar.
        let total = self.b.doc.page.lines.len().max(1);
        if total > rows {
            let area = self.h - BAR - STATUS;
            let th = (area * rows as i32 / total as i32).max(12);
            let ty = BAR + (area - th) * self.b.top as i32 / (total - rows).max(1) as i32;
            c.fill_rect(Rect::new(self.w - 5, ty, 3, th), theme::LINE);
        }
        // Status bar.
        let sy = self.h - STATUS;
        c.fill_rect(Rect::new(0, sy, self.w, STATUS), theme::SURFACE);
        c.line(0, sy, self.w, sy, theme::LINE_DIM);
        let status = if let Some(l) = &self.loading {
            format!("Loading {} ...", l)
        } else if let Edit::Field(f, v) = &self.edit {
            format!("{}: {}_   (Enter submits, Esc cancels)", self.b.doc.page.fields[*f].name, v)
        } else if let Some(h) = &self.hover {
            h.clone()
        } else if !self.b.message.is_empty() {
            self.b.message.clone()
        } else {
            self.b.doc.page.title.clone()
        };
        c.draw_text(font, 6, sy + 2, &status.chars().take(((self.w - 12) / FONT_W) as usize).collect::<String>(), theme::TEXT_DIM, None);
        d.put_canvas(win, &c, c.bounds(), 0, 0);
        d.flush();
    }

    /// The link or field under (x, y), if any.
    fn hit(&self, x: i32, y: i32) -> Option<Target> {
        if y < BAR + 2 || y >= self.h - STATUS {
            return None;
        }
        let row = ((y - BAR - 2) / FONT_H) as usize + self.b.top;
        let line = self.b.doc.page.lines.get(row)?;
        let mut sx = PAD;
        for span in line {
            let w = span.text.chars().count() as i32 * FONT_W;
            if x >= sx && x < sx + w {
                return match (span.field, span.link) {
                    (Some(f), _) => Some(Target::Field(f)),
                    (None, Some(l)) => Some(Target::Link(l)),
                    _ => None,
                };
            }
            sx += w;
        }
        None
    }

    fn scroll(&mut self, delta: isize) {
        let max = self.b.doc.page.lines.len().saturating_sub(self.rows());
        self.b.top = (self.b.top as isize + delta).clamp(0, max as isize) as usize;
    }
}

/// Runs a slow action with "Loading" shown first.
fn busy(app: &mut App, d: &mut Display, win: u32, font: &Font, what: &str, f: impl FnOnce(&mut Browser)) {
    app.loading = Some(what.to_string());
    app.draw(d, win, font);
    f(&mut app.b);
    app.loading = None;
    app.hover = None;
    d.set_title(win, &format!("{} - web", app.b.doc.page.title));
}

fn activate(app: &mut App, d: &mut Display, win: u32, font: &Font) {
    let what = match app.b.selected {
        Some(Target::Link(n)) => app.b.resolve(&app.b.doc.page.links[n]).map(|l| l.to_string()).unwrap_or_default(),
        _ => String::from("form"),
    };
    let mut field = None;
    busy(app, d, win, font, &what, |b| field = b.activate());
    if let Some(f) = field {
        app.edit = Edit::Field(f, app.b.doc.page.fields[f].value.clone());
    }
}

fn main() -> i32 {
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("web: {} (start the desktop with startgui)", e);
            return 1;
        }
    };
    let font = load_font();
    let (w, h) = (800, 560);
    let win = d.create_window(40, 40, w, h, KIND_NORMAL);
    d.set_title(win, "web");
    d.map(win);
    let mut app = App { b: Browser::new(((w - 2 * PAD) / FONT_W) as usize), w, h, edit: Edit::None, hover: None, loading: None };
    let args = env::args();
    app.b.insecure = args.iter().any(|a| a == "-k");
    if let Some(start) = args[1..].iter().find(|a| !a.starts_with('-')) {
        if let Ok(loc) = app.b.parse_input(start) {
            busy(&mut app, &mut d, win, &font, start, |b| b.open(loc));
        }
    }
    while let Some(ev) = d.wait_event(-1) {
        match ev {
            Event::Expose { w, h, .. } | Event::Configure { w, h, .. } => {
                app.w = w;
                app.h = h;
                let cols = app.cols();
                app.b.set_width(cols);
            }
            Event::Motion { x, y, .. } => {
                let hover = match app.hit(x, y) {
                    Some(Target::Link(n)) => app.b.resolve(&app.b.doc.page.links[n]).ok().map(|l| l.to_string()),
                    _ => None,
                };
                if hover == app.hover {
                    continue;
                }
                app.hover = hover;
            }
            Event::Button { button, pressed: true, x, y, .. } => match button {
                WHEEL_UP => app.scroll(-3),
                WHEEL_DOWN => app.scroll(3),
                BUTTON_LEFT => {
                    if let Some(i) = (0..BUTTONS.len()).find(|&i| App::button_rect(i).contains(x, y)) {
                        app.edit = Edit::None;
                        match i {
                            0 => busy(&mut app, &mut d, win, &font, "previous page", |b| b.go_back()),
                            1 => busy(&mut app, &mut d, win, &font, "next page", |b| b.go_forward()),
                            2 => busy(&mut app, &mut d, win, &font, "page", |b| b.reload()),
                            _ => busy(&mut app, &mut d, win, &font, "home", |b| b.open(huldra_user::browser::Location::Home)),
                        }
                    } else if app.address_rect().contains(x, y) {
                        app.edit = Edit::Address(String::new());
                    } else if let Some(t) = app.hit(x, y) {
                        app.edit = Edit::None;
                        app.b.selected = Some(t);
                        activate(&mut app, &mut d, win, &font);
                    }
                }
                _ => continue,
            },
            Event::Key { code, pressed: true, mods, ch, .. } => {
                let c = char::from_u32(ch).unwrap_or('\0');
                let ctrl = mods & MOD_CTRL != 0;
                let alt = mods & MOD_ALT != 0;
                match &mut app.edit {
                    Edit::Address(s) | Edit::Field(_, s) => match code {
                        keymap::ESC => app.edit = Edit::None,
                        keymap::BACKSPACE => {
                            s.pop();
                        }
                        keymap::ENTER | keymap::KP_ENTER => {
                            match core::mem::replace(&mut app.edit, Edit::None) {
                                Edit::Address(text) => match app.b.parse_input(&text) {
                                    Ok(loc) => busy(&mut app, &mut d, win, &font, &text, |b| b.open(loc)),
                                    Err(e) => app.b.message = e,
                                },
                                Edit::Field(f, value) => {
                                    app.b.set_field(f, value);
                                    busy(&mut app, &mut d, win, &font, "form", |b| b.submit(f));
                                }
                                Edit::None => {}
                            }
                        }
                        _ if c >= ' ' && !ctrl => s.push(c),
                        _ => continue,
                    },
                    Edit::None => match code {
                        keymap::UP => app.scroll(-1),
                        keymap::DOWN => app.scroll(1),
                        keymap::PGUP => app.scroll(-(app.rows() as isize - 1)),
                        keymap::PGDN => app.scroll(app.rows() as isize - 1),
                        keymap::HOME => app.b.top = 0,
                        keymap::END => app.scroll(isize::MAX / 2),
                        keymap::TAB => {
                            let rows = app.rows();
                            app.b.select_next(mods & MOD_SHIFT == 0, rows);
                        }
                        keymap::ENTER | keymap::KP_ENTER if app.b.selected.is_some() => activate(&mut app, &mut d, win, &font),
                        keymap::LEFT if alt => busy(&mut app, &mut d, win, &font, "previous page", |b| b.go_back()),
                        keymap::BACKSPACE => busy(&mut app, &mut d, win, &font, "previous page", |b| b.go_back()),
                        keymap::RIGHT if alt => busy(&mut app, &mut d, win, &font, "next page", |b| b.go_forward()),
                        0x3F => busy(&mut app, &mut d, win, &font, "page", |b| b.reload()), // F5
                        _ if ctrl && (c == 'l' || c == '\x0c') => app.edit = Edit::Address(String::new()),
                        _ if ctrl && (c == 's' || c == '\x13') => {
                            let msg = app.b.save().unwrap_or_else(|e| e);
                            app.b.message = msg;
                        }
                        _ if c == ' ' => app.scroll(app.rows() as isize - 1),
                        _ => continue,
                    },
                }
            }
            Event::CloseRequest { .. } => return 0,
            _ => continue,
        }
        app.draw(&mut d, win, &font);
    }
    0
}
