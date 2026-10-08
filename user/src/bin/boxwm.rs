//! boxwm: a stacking window manager in the spirit of Openbox.
//!
//! Windows get a frame with a title bar and minimize/maximize/close
//! buttons. Drag the title bar (or Alt+drag anywhere) to move, the
//! bottom-right corner to resize; double-click the title to maximize.
//! Right-click the desktop for the menu.
//!
//! Keys: Alt+Tab next window, Alt+F4 close, Ctrl+Alt+T terminal,
//! Alt+F2 run a command, Alt+F10 maximize.

#![no_std]
#![no_main]

use huldra_gfx::keymap;
use huldra_user::gui::*;
use huldra_user::{eprintln, String, Vec};

huldra_user::main!(main);

const TITLE: i32 = 22;
const BORDER: i32 = 2;
const BUTTON: i32 = 16;
const PANEL: i32 = 26;
const CORNER: i32 = 14;

const ACTIVE_TOP: u32 = 0xFF5A82B4;
const ACTIVE_BOTTOM: u32 = 0xFF2E5486;
const INACTIVE_TOP: u32 = 0xFF9A9A9A;
const INACTIVE_BOTTOM: u32 = 0xFF6E6E6E;
const BORDER_ACTIVE: u32 = 0xFF22406A;
const BORDER_INACTIVE: u32 = 0xFF505050;

const MENU_ITEMS: &[(&str, &str)] = &[
    ("Terminal", "term"),
    ("Clock", "clock"),
    ("Calculator", "calc"),
    ("Paint", "paint"),
    ("Files", "files"),
    ("System info", "sysinfo"),
    ("Run...", "menu"),
    ("", ""),
    ("Switch to tiling (tilewm)", "@tilewm"),
    ("Exit", "@exit"),
];

struct Managed {
    win: u32,
    frame: u32,
    /// Client area on screen.
    rect: Rect,
    title: String,
    minimized: bool,
    restore: Option<Rect>,
}

impl Managed {
    fn frame_rect(&self) -> Rect {
        Rect::new(self.rect.x - BORDER, self.rect.y - TITLE - BORDER, self.rect.w + 2 * BORDER, self.rect.h + TITLE + 2 * BORDER)
    }
}

#[derive(Clone, Copy)]
enum DragKind {
    Move,
    Resize,
}

struct Drag {
    kind: DragKind,
    frame: u32,
    start: (i32, i32),
    orig: Rect,
}

struct Menu {
    win: u32,
    rect: Rect,
    hover: Option<usize>,
}

struct Wm {
    d: Display,
    font: Font,
    clients: Vec<Managed>,
    /// Focus history, most recent last.
    focus_order: Vec<u32>,
    focused: Option<u32>,
    drag: Option<Drag>,
    menu: Option<Menu>,
    last_click: (u32, u64),
    cascade: i32,
}

fn now_ms() -> u64 {
    huldra_user::time::uptime_ms()
}

impl Wm {
    fn find_win(&self, win: u32) -> Option<usize> {
        self.clients.iter().position(|c| c.win == win)
    }

    fn find_frame(&self, frame: u32) -> Option<usize> {
        self.clients.iter().position(|c| c.frame == frame)
    }

    fn work_area(&self) -> Rect {
        Rect::new(0, 0, self.d.width, self.d.height - PANEL)
    }

    fn draw_frame(&mut self, i: usize) {
        let c = &self.clients[i];
        let active = self.focused == Some(c.win);
        let fr = c.frame_rect();
        let (w, h) = (fr.w, fr.h);
        let mut canvas = Canvas::new(w, TITLE + BORDER);
        let (top, bottom, border) = if active { (ACTIVE_TOP, ACTIVE_BOTTOM, BORDER_ACTIVE) } else { (INACTIVE_TOP, INACTIVE_BOTTOM, BORDER_INACTIVE) };
        canvas.fill_rect(canvas.bounds(), border);
        canvas.gradient(Rect::new(BORDER, BORDER, w - 2 * BORDER, TITLE), top, bottom);
        canvas.fill_rect(Rect::new(BORDER, BORDER, w - 2 * BORDER, 1), huldra_gfx::canvas::mix(top, 0xFFFFFFFF, 90));
        // Buttons: minimize, maximize, close (right to left: close last).
        let fg = if active { 0xFFFFFFFF } else { 0xFFE0E0E0 };
        for (k, kind) in ["min", "max", "close"].iter().enumerate() {
            let bx = w - BORDER - 4 - (3 - k as i32) * (BUTTON + 3);
            let by = BORDER + (TITLE - BUTTON) / 2;
            let r = Rect::new(bx, by, BUTTON, BUTTON);
            let bg = if *kind == "close" { if active { 0xFFC8504A } else { 0xFF8A6A6A } } else { huldra_gfx::canvas::mix(bottom, 0xFFFFFFFF, 40) };
            canvas.fill_round_rect(r, 3, bg);
            match *kind {
                "min" => canvas.fill_rect(Rect::new(bx + 4, by + 11, 8, 2), fg),
                "max" => {
                    canvas.rect_outline(Rect::new(bx + 4, by + 4, 8, 8), fg);
                    canvas.fill_rect(Rect::new(bx + 4, by + 4, 8, 2), fg);
                }
                _ => {
                    canvas.thick_line(bx + 4, by + 4, bx + 11, by + 11, 2, fg);
                    canvas.thick_line(bx + 11, by + 4, bx + 4, by + 11, 2, fg);
                }
            }
        }
        let max_chars = ((w - 2 * BORDER - 3 * (BUTTON + 3) - 16) / FONT_W).max(0) as usize;
        let title: String = c.title.chars().take(max_chars).collect();
        canvas.set_clip(Rect::new(0, 0, w - 3 * (BUTTON + 3) - 8, TITLE + BORDER));
        canvas.draw_text_bold(&self.font, BORDER + 8, BORDER + (TITLE - 16) / 2, &title, fg);
        canvas.reset_clip();
        let frame = c.frame;
        self.d.put_canvas(frame, &canvas, canvas.bounds(), 0, 0);
        // The rest of the border.
        self.d.fill(frame, Rect::new(0, TITLE + BORDER, BORDER, h), border);
        self.d.fill(frame, Rect::new(w - BORDER, TITLE + BORDER, BORDER, h), border);
        self.d.fill(frame, Rect::new(0, h - BORDER, w, BORDER), border);
        // Resize grip.
        self.d.fill(frame, Rect::new(w - CORNER, h - BORDER, CORNER, BORDER), if active { 0xFF7FA6D6 } else { 0xFF808080 });
    }

    /// Which title bar button is at frame-relative (x, y).
    fn button_at(&self, i: usize, x: i32, y: i32) -> Option<usize> {
        let w = self.clients[i].frame_rect().w;
        let by = BORDER + (TITLE - BUTTON) / 2;
        (0..3).find(|&k| {
            let bx = w - BORDER - 4 - (3 - k as i32) * (BUTTON + 3);
            Rect::new(bx, by, BUTTON, BUTTON).contains(x, y)
        })
    }

    fn place(&mut self, i: usize) {
        let fr = self.clients[i].frame_rect();
        let (frame, win, rect) = (self.clients[i].frame, self.clients[i].win, self.clients[i].rect);
        self.d.configure(frame, fr);
        self.d.configure(win, rect);
        self.draw_frame(i);
    }

    fn focus(&mut self, win: Option<u32>) {
        let old = self.focused;
        self.focused = win;
        if let Some(w) = win {
            self.focus_order.retain(|&x| x != w);
            self.focus_order.push(w);
            if let Some(i) = self.find_win(w) {
                let frame = self.clients[i].frame;
                self.d.send(Request::Raise { id: frame });
                self.d.send(Request::Raise { id: w });
                self.d.send(Request::SetFocus { id: w });
                self.draw_frame(i);
            }
        } else {
            self.d.send(Request::SetFocus { id: ROOT });
        }
        if old != win {
            if let Some(i) = old.and_then(|o| self.find_win(o)) {
                self.draw_frame(i);
            }
        }
    }

    /// Focuses the most recently used visible window.
    fn focus_previous(&mut self) {
        let next = self.focus_order.iter().rev().copied().find(|w| self.find_win(*w).is_some_and(|i| !self.clients[i].minimized));
        self.focus(next);
    }

    fn manage(&mut self, win: u32, x: i32, y: i32, w: i32, h: i32, title: String) {
        if let Some(i) = self.find_win(win) {
            // Already framed (shown again after a minimize).
            self.clients[i].minimized = false;
            let frame = self.clients[i].frame;
            self.d.map(frame);
            self.d.map(win);
            self.focus(Some(win));
            return;
        }
        let area = self.work_area();
        let (w, h) = (w.min(area.w - 2 * BORDER), h.min(area.h - TITLE - 2 * BORDER));
        let (mut x, mut y) = (x, y);
        if x <= 0 && y <= 0 {
            self.cascade = (self.cascade + 1) % 8;
            x = 80 + self.cascade * 28;
            y = 60 + self.cascade * 28;
        }
        x = x.clamp(BORDER, (area.w - w - BORDER).max(BORDER));
        y = y.clamp(TITLE + BORDER, (area.h - h - BORDER).max(TITLE + BORDER));
        let m = Managed { win, frame: 0, rect: Rect::new(x, y, w, h), title, minimized: false, restore: None };
        let fr = m.frame_rect();
        let frame = self.d.create_window(fr.x, fr.y, fr.w, fr.h, KIND_NORMAL);
        self.clients.push(Managed { frame, ..m });
        let i = self.clients.len() - 1;
        self.place(i);
        self.d.map(frame);
        self.d.map(win);
        self.focus(Some(win));
    }

    fn unmanage(&mut self, win: u32) {
        let Some(i) = self.find_win(win) else { return };
        let c = self.clients.remove(i);
        self.d.destroy(c.frame);
        self.focus_order.retain(|&x| x != win);
        if self.focused == Some(win) {
            self.focused = None;
            self.focus_previous();
        }
    }

    fn minimize(&mut self, i: usize) {
        self.clients[i].minimized = true;
        let (frame, win) = (self.clients[i].frame, self.clients[i].win);
        self.d.unmap(frame);
        self.d.unmap(win);
        if self.focused == Some(win) {
            self.focused = None;
            self.focus_previous();
        }
    }

    fn toggle_maximize(&mut self, i: usize) {
        let c = &mut self.clients[i];
        match c.restore.take() {
            Some(r) => c.rect = r,
            None => {
                c.restore = Some(c.rect);
                let area = Rect::new(0, 0, self.d.width, self.d.height - PANEL);
                c.rect = Rect::new(BORDER, TITLE + BORDER, area.w - 2 * BORDER, area.h - TITLE - 2 * BORDER);
            }
        }
        self.place(i);
    }

    fn open_menu(&mut self, x: i32, y: i32) {
        self.close_menu();
        let w = MENU_ITEMS.iter().map(|(l, _)| l.len() as i32).max().unwrap_or(10) * FONT_W + 40;
        let h = MENU_ITEMS.iter().map(|(l, _)| if l.is_empty() { 7 } else { 22 }).sum::<i32>() + 8;
        let x = x.min(self.d.width - w - 1);
        let y = y.min(self.d.height - h - 1);
        let win = self.d.create_window(x, y, w, h, KIND_POPUP);
        self.menu = Some(Menu { win, rect: Rect::new(x, y, w, h), hover: None });
        self.draw_menu();
        self.d.map(win);
        self.d.send(Request::Raise { id: win });
    }

    fn menu_item_at(&self, y: i32) -> Option<usize> {
        let mut top = 4;
        for (i, (label, _)) in MENU_ITEMS.iter().enumerate() {
            let h = if label.is_empty() { 7 } else { 22 };
            if y >= top && y < top + h && !label.is_empty() {
                return Some(i);
            }
            top += h;
        }
        None
    }

    fn draw_menu(&mut self) {
        let Some(m) = &self.menu else { return };
        let mut c = Canvas::new(m.rect.w, m.rect.h);
        c.fill_rect(c.bounds(), 0xFFF2F2F2);
        c.rect_outline(c.bounds(), 0xFF5A5A5A);
        let mut top = 4;
        for (i, (label, _)) in MENU_ITEMS.iter().enumerate() {
            if label.is_empty() {
                c.fill_rect(Rect::new(8, top + 3, m.rect.w - 16, 1), 0xFFB0B0B0);
                top += 7;
                continue;
            }
            let hover = m.hover == Some(i);
            if hover {
                c.gradient(Rect::new(3, top, m.rect.w - 6, 22), ACTIVE_TOP, ACTIVE_BOTTOM);
            }
            c.draw_text(&self.font, 16, top + 3, label, if hover { 0xFFFFFFFF } else { 0xFF202020 }, None);
            top += 22;
        }
        let win = m.win;
        self.d.put_canvas(win, &c, c.bounds(), 0, 0);
    }

    fn close_menu(&mut self) {
        if let Some(m) = self.menu.take() {
            self.d.destroy(m.win);
        }
    }

    fn run_menu_item(&mut self, i: usize) {
        self.close_menu();
        match MENU_ITEMS[i].1 {
            "@exit" => self.d.send(Request::Quit),
            "@tilewm" => {
                spawn("tilewm");
                self.d.flush();
                huldra_user::process::exit(0);
            }
            cmd => {
                spawn(cmd);
            }
        }
    }

    fn cycle(&mut self) {
        let visible: Vec<u32> = self.clients.iter().filter(|c| !c.minimized).map(|c| c.win).collect();
        if visible.is_empty() {
            return;
        }
        let next = match self.focused.and_then(|f| visible.iter().position(|&w| w == f)) {
            Some(i) => visible[(i + 1) % visible.len()],
            None => visible[0],
        };
        self.focus(Some(next));
    }

    fn start_drag(&mut self, i: usize, kind: DragKind, rx: i32, ry: i32) {
        let frame = self.clients[i].frame;
        self.drag = Some(Drag { kind, frame, start: (rx, ry), orig: self.clients[i].rect });
        self.d.send(Request::GrabPointer { id: frame });
    }

    fn clicked(&mut self, id: u32, rx: i32, ry: i32, button: u8, mods: u8) {
        if let Some(m) = &self.menu {
            if id != m.win {
                self.close_menu();
                if id == ROOT && button == BUTTON_LEFT {
                    return;
                }
            }
        }
        if id == ROOT {
            if button == BUTTON_RIGHT || button == BUTTON_MIDDLE {
                self.open_menu(rx, ry);
            }
            return;
        }
        if let Some(i) = self.find_frame(id) {
            let win = self.clients[i].win;
            self.focus(Some(win));
            let fr = self.clients[i].frame_rect();
            let (x, y) = (rx - fr.x, ry - fr.y);
            if button != BUTTON_LEFT {
                return;
            }
            if y < TITLE + BORDER {
                match self.button_at(i, x, y) {
                    Some(0) => self.minimize(i),
                    Some(1) => self.toggle_maximize(i),
                    Some(_) => self.d.send(Request::Close { id: win }),
                    None => {
                        let t = now_ms();
                        if self.last_click.0 == id && t - self.last_click.1 < 400 {
                            self.toggle_maximize(i);
                            self.last_click = (0, 0);
                        } else {
                            self.last_click = (id, t);
                            self.start_drag(i, DragKind::Move, rx, ry);
                        }
                    }
                }
            } else if x >= fr.w - CORNER && y >= fr.h - CORNER {
                self.start_drag(i, DragKind::Resize, rx, ry);
            }
            return;
        }
        if let Some(i) = self.find_win(id) {
            self.focus(Some(id));
            if mods & MOD_ALT != 0 && button == BUTTON_LEFT {
                self.start_drag(i, DragKind::Move, rx, ry);
            } else if mods & MOD_ALT != 0 && button == BUTTON_RIGHT {
                self.start_drag(i, DragKind::Resize, rx, ry);
            }
        }
    }

    fn motion(&mut self, rx: i32, ry: i32) {
        let Some(drag) = &self.drag else { return };
        let Some(i) = self.find_frame(drag.frame) else { return };
        let (dx, dy) = (rx - drag.start.0, ry - drag.start.1);
        let o = drag.orig;
        let r = match drag.kind {
            DragKind::Move => Rect::new(o.x + dx, (o.y + dy).max(TITLE + BORDER), o.w, o.h),
            DragKind::Resize => Rect::new(o.x, o.y, (o.w + dx).max(80), (o.h + dy).max(40)),
        };
        if r != self.clients[i].rect {
            self.clients[i].rect = r;
            self.clients[i].restore = None;
            self.place(i);
        }
    }

    fn handle(&mut self, ev: Event) {
        match ev {
            Event::MapRequest { id, x, y, w, h, title, .. } => self.manage(id, x, y, w, h, title),
            Event::ConfigureRequest { id, x, y, w, h } => {
                if let Some(i) = self.find_win(id) {
                    let r = self.clients[i].rect;
                    self.clients[i].rect = Rect::new(if x > 0 { x } else { r.x }, if y > 0 { y } else { r.y }, w, h);
                    self.place(i);
                } else {
                    self.d.configure(id, Rect::new(x, y, w, h));
                }
            }
            Event::Destroyed { id } => self.unmanage(id),
            Event::Unmapped { id } => {
                // Unmapped by its owner (not minimized by us): forget it.
                if self.find_win(id).is_some_and(|i| !self.clients[i].minimized) {
                    self.unmanage(id);
                }
            }
            Event::TitleChanged { id, title } => {
                if let Some(i) = self.find_win(id) {
                    self.clients[i].title = title;
                    self.draw_frame(i);
                }
            }
            Event::ActivateRequest { id } => {
                if let Some(i) = self.find_win(id) {
                    if self.clients[i].minimized {
                        self.clients[i].minimized = false;
                        let frame = self.clients[i].frame;
                        self.d.map(frame);
                        self.d.map(id);
                    } else if self.focused == Some(id) {
                        // Clicking the focused window's panel entry minimizes it.
                        self.minimize(i);
                        return;
                    }
                    self.focus(Some(id));
                }
            }
            Event::Expose { id, .. } => {
                if let Some(i) = self.find_frame(id) {
                    self.draw_frame(i);
                } else if self.menu.as_ref().is_some_and(|m| m.win == id) {
                    self.draw_menu();
                }
            }
            Event::Clicked { id, rx, ry, button, mods } => self.clicked(id, rx, ry, button, mods),
            Event::Motion { id, x, y, rx, ry, .. } => {
                if self.drag.is_some() {
                    self.motion(rx, ry);
                } else if let Some(m) = &self.menu {
                    if m.win == id {
                        let hover = self.menu_item_at(y).filter(|_| x >= 0 && x < m.rect.w);
                        if hover != m.hover {
                            self.menu.as_mut().unwrap().hover = hover;
                            self.draw_menu();
                        }
                    }
                }
            }
            Event::Button { id, y, button, pressed: false, .. } if button < WHEEL_UP => {
                if self.drag.take().is_some() {
                    self.d.send(Request::UngrabPointer);
                } else if button == BUTTON_LEFT && self.menu.as_ref().is_some_and(|m| m.win == id) {
                    if let Some(i) = self.menu_item_at(y) {
                        self.run_menu_item(i);
                    }
                }
            }
            Event::KeyGrabbed { code, mods, pressed: true } => {
                if mods == MOD_ALT && code == keymap::TAB {
                    self.cycle();
                } else if mods == MOD_ALT && code == keymap::F1 + 3 {
                    if let Some(w) = self.focused {
                        self.d.send(Request::Close { id: w });
                    }
                } else if mods == MOD_ALT && code == keymap::F1 + 1 {
                    spawn("menu");
                } else if mods == MOD_ALT && code == keymap::F10 {
                    if let Some(i) = self.focused.and_then(|w| self.find_win(w)) {
                        self.toggle_maximize(i);
                    }
                } else if mods == MOD_CTRL | MOD_ALT && code == keymap::code_of('t') {
                    spawn("term");
                }
            }
            Event::Key { code: keymap::ESC, pressed: true, .. } => self.close_menu(),
            Event::FocusChanged { id: ROOT } => {
                self.focused = None;
                self.focus_previous();
            }
            _ => {}
        }
    }
}

fn main() -> i32 {
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("boxwm: cannot connect to the display: {}", e);
            return 1;
        }
    };
    d.send(Request::BecomeWm);
    for (code, mods) in [(keymap::TAB, MOD_ALT), (keymap::F1 + 3, MOD_ALT), (keymap::F1 + 1, MOD_ALT), (keymap::F10, MOD_ALT), (keymap::code_of('t'), MOD_CTRL | MOD_ALT)] {
        d.send(Request::GrabKey { code, mods });
    }
    d.send(Request::SetStatus { text: String::new() });
    d.flush();
    let mut wm = Wm { d, font: load_font(), clients: Vec::new(), focus_order: Vec::new(), focused: None, drag: None, menu: None, last_click: (0, 0), cascade: 0 };
    while let Some(ev) = wm.d.wait_event(-1) {
        if let Event::Error { message, .. } = &ev {
            eprintln!("boxwm: {}", message);
            if message.contains("window manager") {
                return 1;
            }
        }
        wm.handle(ev);
        reap_children();
    }
    0
}
