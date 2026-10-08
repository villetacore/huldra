//! tilewm: a tiling window manager in the spirit of i3.
//!
//! Windows fill the screen in a tree of horizontal and vertical splits,
//! on nine workspaces. $mod is Alt:
//!
//!   $mod+Enter        terminal            $mod+d             run (menu)
//!   $mod+j/k/l/;      focus left/down/up/right (or $mod+arrows)
//!   $mod+Shift+j/k/l/; move the window (or $mod+Shift+arrows)
//!   $mod+h / $mod+v   split horizontally / vertically for the next window
//!   $mod+e            toggle the split direction
//!   $mod+f            fullscreen          $mod+Shift+q       close window
//!   $mod+1..9         workspace           $mod+Shift+1..9    move to workspace
//!   $mod+Shift+b      switch to boxwm     $mod+Shift+e       exit

#![no_std]
#![no_main]

use huldra_gfx::keymap::{self, code_of};
use huldra_gfx::tile::{Dir, Side, Workspace};
use huldra_user::gui::*;
use huldra_user::{eprintln, format, process, String, Vec};

huldra_user::main!(main);

const MOD: u8 = MOD_ALT;
const PANEL: i32 = 26;
const GAP: i32 = 6;
const TITLE: i32 = 18;
const BORDER: i32 = 2;

// i3's default colors.
const FOCUSED_BORDER: u32 = 0xFF4C7899;
const FOCUSED_BG: u32 = 0xFF285577;
const UNFOCUSED_BORDER: u32 = 0xFF333333;
const UNFOCUSED_BG: u32 = 0xFF222222;
const UNFOCUSED_TEXT: u32 = 0xFF888888;

struct Client {
    win: u32,
    frame: u32,
    title: String,
    ws: usize,
    rect: Rect,
}

struct Wm {
    d: Display,
    font: Font,
    ws: [Workspace; 9],
    cur: usize,
    clients: Vec<Client>,
}

impl Wm {
    fn area(&self) -> Rect {
        Rect::new(0, 0, self.d.width, self.d.height - PANEL)
    }

    fn client(&self, win: u32) -> Option<usize> {
        self.clients.iter().position(|c| c.win == win)
    }

    fn by_frame(&self, frame: u32) -> Option<usize> {
        self.clients.iter().position(|c| c.frame == frame)
    }

    fn focused(&self) -> Option<u32> {
        self.ws[self.cur].focus
    }

    fn draw_frame(&mut self, i: usize) {
        let c = &self.clients[i];
        let focused = self.ws[c.ws].focus == Some(c.win);
        let (border, bg, fg) = if focused { (FOCUSED_BORDER, FOCUSED_BG, 0xFFFFFFFF) } else { (UNFOCUSED_BORDER, UNFOCUSED_BG, UNFOCUSED_TEXT) };
        let r = c.rect;
        let mut cv = Canvas::new(r.w, TITLE + BORDER);
        cv.fill_rect(cv.bounds(), border);
        cv.fill_rect(Rect::new(1, 1, r.w - 2, TITLE - 1), bg);
        let max = ((r.w - 12) / FONT_W).max(0) as usize;
        let title: String = c.title.chars().take(max).collect();
        cv.draw_text(&self.font, 6, 1, &title, fg, None);
        let frame = c.frame;
        self.d.put_canvas(frame, &cv, cv.bounds(), 0, 0);
        self.d.fill(frame, Rect::new(0, TITLE, BORDER, r.h), border);
        self.d.fill(frame, Rect::new(r.w - BORDER, TITLE, BORDER, r.h), border);
        self.d.fill(frame, Rect::new(0, r.h - BORDER, r.w, BORDER), border);
    }

    fn status(&mut self) {
        let mut s = String::new();
        for (i, w) in self.ws.iter().enumerate() {
            if i == self.cur {
                s.push_str(&format!("[{}] ", i + 1));
            } else if !w.is_empty() {
                s.push_str(&format!(" {}  ", i + 1));
            }
        }
        s.push_str(if self.ws[self.cur].fullscreen.is_some() { " fullscreen" } else { "" });
        self.d.send(Request::SetStatus { text: String::from(s.trim_end()) });
    }

    /// Positions every window of the current workspace.
    fn relayout(&mut self) {
        let area = self.area();
        let layout = self.ws[self.cur].layout(area, if self.ws[self.cur].fullscreen.is_some() { 0 } else { GAP });
        let full = self.ws[self.cur].fullscreen;
        for (win, r) in layout {
            let Some(i) = self.client(win) else { continue };
            self.clients[i].rect = r;
            let frame = self.clients[i].frame;
            if full == Some(win) {
                self.d.unmap(frame);
                self.d.configure(win, r);
            } else {
                self.d.configure(frame, r);
                self.d.map(frame);
                self.d.configure(win, Rect::new(r.x + BORDER, r.y + TITLE, r.w - 2 * BORDER, r.h - TITLE - BORDER));
                self.draw_frame(i);
            }
            self.d.map(win);
        }
        // Hide windows that are not part of the layout (fullscreen hides the rest).
        if let Some(f) = full {
            for c in self.clients.iter().filter(|c| c.ws == self.cur && c.win != f) {
                self.d.unmap(c.frame);
                self.d.unmap(c.win);
            }
        }
        self.apply_focus();
        self.status();
    }

    fn apply_focus(&mut self) {
        match self.focused() {
            Some(w) => {
                self.d.send(Request::SetFocus { id: w });
                if let Some(i) = self.client(w) {
                    let frame = self.clients[i].frame;
                    self.d.send(Request::Raise { id: frame });
                    self.d.send(Request::Raise { id: w });
                }
            }
            None => self.d.send(Request::SetFocus { id: ROOT }),
        }
        for i in 0..self.clients.len() {
            if self.clients[i].ws == self.cur {
                self.draw_frame(i);
            }
        }
    }

    fn manage(&mut self, win: u32, title: String) {
        if self.client(win).is_some() {
            self.relayout();
            return;
        }
        let frame = self.d.create_window(0, 0, 10, 10, KIND_NORMAL);
        self.clients.push(Client { win, frame, title, ws: self.cur, rect: Rect::default() });
        self.ws[self.cur].insert(win);
        self.relayout();
    }

    fn unmanage(&mut self, win: u32) {
        let Some(i) = self.client(win) else { return };
        let c = self.clients.remove(i);
        self.d.destroy(c.frame);
        self.ws[c.ws].remove(win);
        if c.ws == self.cur {
            self.relayout();
        } else {
            self.status();
        }
    }

    fn switch(&mut self, n: usize) {
        if n == self.cur {
            return;
        }
        for c in self.clients.iter().filter(|c| c.ws == self.cur) {
            self.d.unmap(c.frame);
            self.d.unmap(c.win);
        }
        self.cur = n;
        self.relayout();
    }

    fn move_to(&mut self, n: usize) {
        let Some(w) = self.focused() else { return };
        if n == self.cur {
            return;
        }
        let i = self.client(w).unwrap();
        self.ws[self.cur].remove(w);
        self.ws[n].insert(w);
        self.clients[i].ws = n;
        let (frame, win) = (self.clients[i].frame, self.clients[i].win);
        self.d.unmap(frame);
        self.d.unmap(win);
        self.relayout();
    }

    fn key(&mut self, code: u16, mods: u8) {
        let area = self.area();
        let shift = mods & MOD_SHIFT != 0;
        let side = match code {
            c if c == code_of('j') || c == keymap::LEFT => Some(Side::Left),
            c if c == code_of('k') || c == keymap::DOWN => Some(Side::Down),
            c if c == code_of('l') || c == keymap::UP => Some(Side::Up),
            0x27 | keymap::RIGHT => Some(Side::Right), // ';'
            _ => None,
        };
        if let Some(side) = side {
            let ws = &mut self.ws[self.cur];
            if shift {
                ws.move_focused(side, area);
            } else if let Some(n) = ws.focus.and_then(|f| ws.neighbor(f, side, area)) {
                ws.focus = Some(n);
            }
            self.relayout();
            return;
        }
        if (0x02..=0x0A).contains(&code) {
            let n = (code - 0x02) as usize;
            if shift {
                self.move_to(n);
            } else {
                self.switch(n);
            }
            return;
        }
        match (code, shift) {
            (keymap::ENTER, false) => {
                spawn("term");
            }
            (c, false) if c == code_of('d') => {
                spawn("menu");
            }
            (c, false) if c == code_of('h') => self.ws[self.cur].split(Dir::H),
            (c, false) if c == code_of('v') => self.ws[self.cur].split(Dir::V),
            (c, false) if c == code_of('e') => {
                self.ws[self.cur].toggle_split();
                self.relayout();
            }
            (c, false) if c == code_of('f') => {
                let ws = &mut self.ws[self.cur];
                ws.fullscreen = if ws.fullscreen.is_some() { None } else { ws.focus };
                self.relayout();
            }
            (c, true) if c == code_of('q') => {
                if let Some(w) = self.focused() {
                    self.d.send(Request::Close { id: w });
                }
            }
            (c, true) if c == code_of('e') => self.d.send(Request::Quit),
            (c, true) if c == code_of('b') => {
                for c in &self.clients {
                    self.d.destroy(c.frame);
                }
                spawn("boxwm");
                self.d.flush();
                process::exit(0);
            }
            _ => {}
        }
    }

    fn handle(&mut self, ev: Event) {
        match ev {
            Event::MapRequest { id, title, .. } => self.manage(id, title),
            Event::ConfigureRequest { id, .. } => {
                // Tiled windows keep their place; tell the client where it is.
                if self.client(id).is_some() {
                    self.relayout();
                }
            }
            Event::Destroyed { id } | Event::Unmapped { id } => {
                // Our own unmaps (workspace switches) do not come back here:
                // only the owner's.
                self.unmanage(id);
            }
            Event::TitleChanged { id, title } => {
                if let Some(i) = self.client(id) {
                    self.clients[i].title = title;
                    if self.clients[i].ws == self.cur {
                        self.draw_frame(i);
                    }
                }
            }
            Event::Clicked { id, .. } => {
                let win = self.client(id).map(|i| self.clients[i].win).or_else(|| self.by_frame(id).map(|i| self.clients[i].win));
                if let Some(w) = win {
                    if self.ws[self.cur].contains(w) && self.ws[self.cur].focus != Some(w) {
                        self.ws[self.cur].focus = Some(w);
                        self.apply_focus();
                    }
                }
            }
            Event::ActivateRequest { id } => {
                if let Some(i) = self.client(id) {
                    let ws = self.clients[i].ws;
                    self.ws[ws].focus = Some(id);
                    if ws != self.cur {
                        self.switch(ws);
                    } else {
                        self.apply_focus();
                    }
                }
            }
            Event::Expose { id, .. } => {
                if let Some(i) = self.by_frame(id) {
                    self.draw_frame(i);
                }
            }
            Event::FocusChanged { id: ROOT } => self.apply_focus(),
            Event::KeyGrabbed { code, mods, pressed: true } => self.key(code, mods),
            _ => {}
        }
    }
}

fn main() -> i32 {
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("tilewm: cannot connect to the display: {}", e);
            return 1;
        }
    };
    d.send(Request::BecomeWm);
    let mut keys: Vec<u16> = Vec::from([keymap::ENTER, keymap::LEFT, keymap::RIGHT, keymap::UP, keymap::DOWN, 0x27]);
    for c in ['d', 'h', 'v', 'e', 'f', 'q', 'j', 'k', 'l', 'b'] {
        keys.push(code_of(c));
    }
    keys.extend(0x02..=0x0A);
    for &k in &keys {
        d.send(Request::GrabKey { code: k, mods: MOD });
        d.send(Request::GrabKey { code: k, mods: MOD | MOD_SHIFT });
    }
    // i3's background is plain.
    d.fill(ROOT, Rect::new(0, 0, d.width, d.height), 0xFF101418);
    d.text(ROOT, 20, d.height - PANEL - 40, 0xFF4A5560, 0, "tilewm: Alt+Enter terminal, Alt+d run, Alt+1..9 workspaces, Alt+Shift+b floating mode");
    d.flush();
    let mut wm = Wm { d, font: load_font(), ws: Default::default(), cur: 0, clients: Vec::new() };
    wm.status();
    while let Some(ev) = wm.d.wait_event(-1) {
        if let Event::Error { message, .. } = &ev {
            eprintln!("tilewm: {}", message);
            if message.contains("window manager") {
                return 1;
            }
        }
        wm.handle(ev);
        wm.d.flush();
        reap_children();
    }
    0
}
