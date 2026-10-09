//! display: the Huldra display server (the X server of this system).
//!
//! Owns the frame buffer, keyboard and mouse, and serves clients on TCP
//! 127.0.0.1:6000 (`DISPLAY=:0`). Every window keeps its own pixels
//! (backing store), so the screen is recomposed from them for damaged
//! areas only and clients never have to redraw for overlapping windows.
//! Window management is delegated to a window manager client (see the
//! protocol in huldra-gfx); without one, windows simply appear where
//! their owners put them.
//!
//! Usage: display [--no-crt] [WIDTHxHEIGHT]. Ctrl+Alt+Backspace kills it.
//! The screen imitates a CRT (faint scanlines) unless `--no-crt` is given;
//! screenshots are taken before that, so they stay clean.

#![no_std]
#![no_main]

mod cursor;

use huldra_gfx::canvas::{Canvas, Rect};
use huldra_gfx::font::Font;
use huldra_gfx::keymap::{self, Modifiers};
use huldra_gfx::proto::*;
use huldra_gfx::theme;
use huldra_user::abi::fs::{PollFd, O_RDWR, POLLIN};
use huldra_user::abi::mm::{MAP_SHARED, PROT_READ, PROT_WRITE};
use huldra_user::abi::net::MSG_DONTWAIT;
use huldra_user::net::{self, Ip, Socket};
use huldra_user::{env, eprintln, gui, println, sys, String, Vec};
use alloc::collections::BTreeMap;

extern crate alloc;

huldra_user::main!(main);

const FBIOGET_VSCREENINFO: u32 = 0x4600;
const FBIOPUT_VSCREENINFO: u32 = 0x4601;

struct Win {
    owner: u32,
    rect: Rect,
    mapped: bool,
    kind: u8,
    title: String,
    canvas: Canvas,
    /// Shown by (or on behalf of) the window manager.
    managed: bool,
}

struct Client {
    sock: Socket,
    inbuf: Vec<u8>,
    out: Writer,
    wm: bool,
    select: bool,
    dead: bool,
}

struct Server {
    fb: *mut u32,
    width: i32,
    height: i32,
    screen: Canvas,
    font: Font,
    root: Canvas,
    wins: BTreeMap<u32, Win>,
    /// Stacking order, bottom first.
    stack: Vec<u32>,
    clients: BTreeMap<u32, Client>,
    next_client: u32,
    wm: Option<u32>,
    focus: u32,
    px: i32,
    py: i32,
    buttons: u8,
    pointer_grab: Option<u32>,
    /// Window that got the button press (gets motion until release).
    pressed: Option<u32>,
    under: u32,
    mods: Modifiers,
    key_grabs: Vec<(u16, u8)>,
    damage: Vec<Rect>,
    status: String,
    quit: bool,
    /// Darken every other row on the way to the frame buffer.
    crt: bool,
}

fn send_to(c: &mut Client, e: &Event) {
    if !c.dead {
        e.encode(&mut c.out);
    }
}

impl Server {
    fn client_of(&self, win: u32) -> Option<u32> {
        if win == ROOT {
            return None;
        }
        self.wins.get(&win).map(|w| w.owner)
    }

    fn emit(&mut self, client: u32, e: Event) {
        if let Some(c) = self.clients.get_mut(&client) {
            send_to(c, &e);
        }
    }

    fn emit_wm(&mut self, e: Event) {
        if let Some(wm) = self.wm {
            self.emit(wm, e);
        }
    }

    fn emit_panels(&mut self, e: &Event) {
        for c in self.clients.values_mut().filter(|c| c.select) {
            send_to(c, e);
        }
    }

    fn damage(&mut self, r: Rect) {
        let r = r.intersect(&Rect::new(0, 0, self.width, self.height));
        if r.is_empty() {
            return;
        }
        // Merge with an overlapping rectangle to keep the list short.
        if let Some(d) = self.damage.iter_mut().find(|d| !d.intersect(&r).is_empty()) {
            *d = d.union(&r);
        } else if self.damage.len() < 16 {
            self.damage.push(r);
        } else {
            self.damage[0] = self.damage[0].union(&r);
        }
    }

    fn damage_window(&mut self, id: u32) {
        if let Some(w) = self.wins.get(&id) {
            if w.mapped {
                let r = w.rect;
                self.damage(r);
            }
        }
    }

    /// Windows in painting order: normal windows, then docks, then popups.
    fn paint_order(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        for layer in [[KIND_NORMAL, KIND_DIALOG], [KIND_DOCK, KIND_DOCK], [KIND_POPUP, KIND_POPUP]] {
            v.extend(self.stack.iter().copied().filter(|id| self.wins.get(id).is_some_and(|w| w.mapped && layer.contains(&w.kind))));
        }
        v
    }

    fn window_at(&self, x: i32, y: i32) -> u32 {
        self.paint_order().into_iter().rev().find(|id| self.wins[id].rect.contains(x, y)).unwrap_or(ROOT)
    }

    fn compose(&mut self) {
        let damage = core::mem::take(&mut self.damage);
        if damage.is_empty() {
            return;
        }
        let order = self.paint_order();
        let cursor = cursor::rect(self.px, self.py);
        for r in damage {
            self.screen.set_clip(r);
            self.screen.blit(&self.root, r, r.x, r.y);
            for id in &order {
                let w = &self.wins[id];
                let part = r.intersect(&w.rect);
                if !part.is_empty() {
                    self.screen.blit(&w.canvas, part.offset(-w.rect.x, -w.rect.y), part.x, part.y);
                }
            }
            if !r.intersect(&cursor).is_empty() {
                cursor::draw(&mut self.screen, self.px, self.py);
            }
            self.screen.reset_clip();
            // Copy to the frame buffer.
            for y in r.y..r.bottom() {
                let start = (y * self.width + r.x) as usize;
                if self.crt && y & 1 == 1 {
                    for (i, &p) in self.screen.pixels[start..start + r.w as usize].iter().enumerate() {
                        unsafe { self.fb.add(start + i).write(theme::darken(p)) };
                    }
                    continue;
                }
                unsafe {
                    core::ptr::copy_nonoverlapping(self.screen.pixels.as_ptr().add(start), self.fb.add(start), r.w as usize);
                }
            }
        }
    }

    // ---- windows ----------------------------------------------------------

    fn set_focus(&mut self, id: u32) {
        if id == self.focus {
            return;
        }
        let old = self.focus;
        if let Some(c) = self.client_of(old) {
            self.emit(c, Event::Focus { id: old, focused: false });
        }
        self.focus = id;
        if let Some(c) = self.client_of(id) {
            self.emit(c, Event::Focus { id, focused: true });
        }
        self.emit_panels(&Event::FocusChanged { id });
    }

    fn raise(&mut self, id: u32) {
        if let Some(i) = self.stack.iter().position(|&x| x == id) {
            self.stack.remove(i);
            self.stack.push(id);
            self.damage_window(id);
        }
    }

    fn lower(&mut self, id: u32) {
        if let Some(i) = self.stack.iter().position(|&x| x == id) {
            self.stack.remove(i);
            self.stack.insert(0, id);
            let r = self.wins[&id].rect;
            self.damage(r);
        }
    }

    fn is_client_window(w: &Win) -> bool {
        w.kind == KIND_NORMAL || w.kind == KIND_DIALOG
    }

    fn do_map(&mut self, id: u32) {
        let Some(w) = self.wins.get_mut(&id) else { return };
        if w.mapped {
            return;
        }
        w.mapped = true;
        let (owner, size, normal, title) = (w.owner, (w.rect.w, w.rect.h), Self::is_client_window(w), w.title.clone());
        self.raise(id);
        self.damage_window(id);
        self.emit(owner, Event::Expose { id, w: size.0, h: size.1 });
        if normal && Some(owner) != self.wm {
            self.emit_panels(&Event::WindowListItem { id, title, mapped: true });
            if self.wm.is_none() {
                self.set_focus(id);
            }
        }
    }

    /// `notify`: tell the window manager (the owner hid its window).
    fn do_unmap(&mut self, id: u32, notify: bool) {
        let Some(w) = self.wins.get_mut(&id) else { return };
        if !w.mapped {
            return;
        }
        let r = w.rect;
        w.mapped = false;
        let (owner, normal, title) = (w.owner, Self::is_client_window(w), w.title.clone());
        self.damage(r);
        if normal && Some(owner) != self.wm {
            self.emit_panels(&Event::WindowListItem { id, title, mapped: false });
            if notify {
                self.emit_wm(Event::Unmapped { id });
            }
        }
        if self.focus == id {
            self.set_focus(ROOT);
            // Let the window manager pick the next window.
            self.emit_wm(Event::FocusChanged { id: ROOT });
        }
    }

    fn do_configure(&mut self, id: u32, r: Rect) {
        let Some(w) = self.wins.get_mut(&id) else { return };
        let r = Rect::new(r.x, r.y, r.w.clamp(1, 4096), r.h.clamp(1, 4096));
        let old = w.rect;
        if old == r {
            return;
        }
        let resized = old.w != r.w || old.h != r.h;
        if resized {
            w.canvas.resize(r.w, r.h, theme::SURFACE);
        }
        w.rect = r;
        let (owner, mapped) = (w.owner, w.mapped);
        if mapped {
            self.damage(old);
            self.damage(r);
        }
        self.emit(owner, Event::Configure { id, x: r.x, y: r.y, w: r.w, h: r.h });
        if resized {
            self.emit(owner, Event::Expose { id, w: r.w, h: r.h });
        }
    }

    fn destroy(&mut self, id: u32) {
        self.do_unmap(id, false);
        let Some(w) = self.wins.remove(&id) else { return };
        self.stack.retain(|&x| x != id);
        if self.pointer_grab == Some(id) {
            self.pointer_grab = None;
        }
        if self.pressed == Some(id) {
            self.pressed = None;
        }
        if Self::is_client_window(&w) && Some(w.owner) != self.wm {
            self.emit_wm(Event::Destroyed { id });
            self.emit_panels(&Event::WindowRemoved { id });
        }
    }

    fn drop_client(&mut self, cid: u32) {
        let ids: Vec<u32> = self.wins.iter().filter(|(_, w)| w.owner == cid).map(|(id, _)| *id).collect();
        for id in ids {
            self.destroy(id);
        }
        self.clients.remove(&cid);
        if self.wm == Some(cid) {
            self.wm = None;
            self.key_grabs.clear();
            // Without a manager, windows it was holding back appear.
            let hidden: Vec<u32> = self.wins.iter().filter(|(_, w)| !w.mapped && w.managed).map(|(id, _)| *id).collect();
            for id in hidden {
                self.do_map(id);
            }
        }
    }

    fn drawable(&mut self, cid: u32, id: u32) -> Option<(&mut Canvas, Rect)> {
        if id == ROOT {
            return Some((&mut self.root, Rect::new(0, 0, self.width, self.height)));
        }
        let w = self.wins.get_mut(&id)?;
        // Only the owner (or the window manager) may draw.
        if w.owner != cid && Some(cid) != self.wm {
            return None;
        }
        let r = w.rect;
        Some((&mut w.canvas, if w.mapped { r } else { Rect::default() }))
    }

    fn handle(&mut self, cid: u32, req: Request) {
        let is_wm = self.wm == Some(cid);
        let font = &self.font as *const Font;
        match req {
            Request::Hello { .. } => {
                let (w, h) = (self.width, self.height);
                self.emit(cid, Event::Welcome { client: cid, width: w, height: h });
            }
            Request::CreateWindow { id, x, y, w, h, kind } => {
                if id >> 20 != cid || self.wins.contains_key(&id) {
                    self.emit(cid, Event::Error { code: 1, message: String::from("bad window id") });
                    return;
                }
                let (w, h) = (w.clamp(1, 4096), h.clamp(1, 4096));
                let mut canvas = Canvas::new(w, h);
                canvas.fill_rect(canvas.bounds(), theme::SURFACE);
                self.wins.insert(id, Win { owner: cid, rect: Rect::new(x, y, w, h), mapped: false, kind, title: String::new(), canvas, managed: false });
                self.stack.push(id);
            }
            Request::DestroyWindow { id } => {
                if self.client_of(id) == Some(cid) || is_wm {
                    self.destroy(id);
                }
            }
            Request::Map { id } => {
                let Some(w) = self.wins.get_mut(&id) else { return };
                let managed_kind = Self::is_client_window(w) && w.owner != self.wm.unwrap_or(u32::MAX);
                if managed_kind && !is_wm && self.wm.is_some() {
                    // The window manager decides where and when.
                    w.managed = true;
                    let (r, kind, title) = (w.rect, w.kind, w.title.clone());
                    self.emit_wm(Event::MapRequest { id, client: cid, x: r.x, y: r.y, w: r.w, h: r.h, kind, title });
                } else if w.owner == cid || is_wm {
                    if is_wm {
                        w.managed = true;
                    }
                    self.do_map(id);
                }
            }
            Request::Unmap { id } => {
                if self.client_of(id) == Some(cid) || is_wm {
                    self.do_unmap(id, !is_wm);
                }
            }
            Request::Configure { id, x, y, w, h } => {
                let Some(win) = self.wins.get(&id) else { return };
                let r = Rect::new(x, y, w, h);
                if win.managed && !is_wm && self.wm.is_some() && Self::is_client_window(win) {
                    self.emit_wm(Event::ConfigureRequest { id, x, y, w, h });
                } else if win.owner == cid || is_wm {
                    self.do_configure(id, r);
                }
            }
            Request::SetTitle { id, title } => {
                let Some(w) = self.wins.get_mut(&id) else { return };
                if w.owner != cid {
                    return;
                }
                w.title = title.clone();
                let (mapped, normal) = (w.mapped, Self::is_client_window(w));
                if normal && !is_wm {
                    self.emit_wm(Event::TitleChanged { id, title: title.clone() });
                    self.emit_panels(&Event::WindowListItem { id, title, mapped });
                }
            }
            Request::Raise { id } => {
                if self.client_of(id) == Some(cid) || is_wm {
                    self.raise(id);
                }
            }
            Request::Lower { id } => {
                if self.client_of(id) == Some(cid) || is_wm {
                    self.lower(id);
                }
            }
            Request::SetFocus { id } => {
                if is_wm || self.wm.is_none() || self.client_of(id) == Some(cid) {
                    self.set_focus(id);
                }
            }
            Request::Fill { id, x, y, w, h, color } => {
                if let Some((c, at)) = self.drawable(cid, id) {
                    c.fill_rect(Rect::new(x, y, w, h), color);
                    self.damage(Rect::new(at.x + x, at.y + y, w, h).intersect(&at));
                }
            }
            Request::Text { id, x, y, fg, bg, text } => {
                if let Some((c, at)) = self.drawable(cid, id) {
                    let font = unsafe { &*font };
                    let end = c.draw_text(font, x, y, &text, fg, if bg == 0 { None } else { Some(bg) });
                    self.damage(Rect::new(at.x + x, at.y + y, end - x, font.height).intersect(&at));
                }
            }
            Request::Line { id, x0, y0, x1, y1, color } => {
                if let Some((c, at)) = self.drawable(cid, id) {
                    c.line(x0, y0, x1, y1, color);
                    let r = Rect::new(x0.min(x1), y0.min(y1), (x1 - x0).abs() + 1, (y1 - y0).abs() + 1);
                    self.damage(r.offset(at.x, at.y).intersect(&at));
                }
            }
            Request::Circle { id, x, y, r, color } => {
                if let Some((c, at)) = self.drawable(cid, id) {
                    c.fill_circle(x, y, r, color);
                    self.damage(Rect::new(x - r, y - r, 2 * r + 1, 2 * r + 1).offset(at.x, at.y).intersect(&at));
                }
            }
            Request::Image { id, x, y, w, h, pixels } => {
                if let Some((c, at)) = self.drawable(cid, id) {
                    c.put_image(x, y, w, h, &pixels);
                    self.damage(Rect::new(at.x + x, at.y + y, w, h).intersect(&at));
                }
            }
            Request::Copy { id, x, y, w, h, dx, dy } => {
                if let Some((c, at)) = self.drawable(cid, id) {
                    c.copy_within(Rect::new(x, y, w, h), dx, dy);
                    self.damage(Rect::new(at.x + dx, at.y + dy, w, h).intersect(&at));
                }
            }
            Request::BecomeWm => {
                if self.wm.is_some() {
                    self.emit(cid, Event::Error { code: 2, message: String::from("another window manager is running") });
                    return;
                }
                self.wm = Some(cid);
                if let Some(c) = self.clients.get_mut(&cid) {
                    c.wm = true;
                }
                // Hand over windows that are already up.
                let existing: Vec<(u32, u32, Rect, u8, String)> = self.wins.iter().filter(|(_, w)| w.mapped && Self::is_client_window(w) && w.owner != cid).map(|(id, w)| (*id, w.owner, w.rect, w.kind, w.title.clone())).collect();
                for (id, owner, r, kind, title) in existing {
                    if let Some(w) = self.wins.get_mut(&id) {
                        w.managed = true;
                    }
                    self.do_unmap(id, false);
                    self.emit(cid, Event::MapRequest { id, client: owner, x: r.x, y: r.y, w: r.w, h: r.h, kind, title });
                }
            }
            Request::GrabKey { code, mods } => {
                if is_wm && !self.key_grabs.contains(&(code, mods)) {
                    self.key_grabs.push((code, mods));
                }
            }
            Request::GrabPointer { id } => {
                if self.wins.contains_key(&id) {
                    self.pointer_grab = Some(id);
                }
            }
            Request::UngrabPointer => self.pointer_grab = None,
            Request::Close { id } => {
                if let Some(owner) = self.client_of(id) {
                    self.emit(owner, Event::CloseRequest { id });
                }
            }
            Request::SelectWindows => {
                if let Some(c) = self.clients.get_mut(&cid) {
                    c.select = true;
                }
                let wm = self.wm;
                let items: Vec<Event> = self.stack.iter().filter_map(|id| self.wins.get(id).map(|w| (id, w))).filter(|(_, w)| Self::is_client_window(w) && Some(w.owner) != wm && (w.mapped || w.managed)).map(|(id, w)| Event::WindowListItem { id: *id, title: w.title.clone(), mapped: w.mapped }).collect();
                for e in items {
                    self.emit(cid, e);
                }
                let (focus, status) = (self.focus, self.status.clone());
                self.emit(cid, Event::FocusChanged { id: focus });
                self.emit(cid, Event::Status { text: status });
            }
            Request::Activate { id } => {
                if self.wm.is_some() {
                    self.emit_wm(Event::ActivateRequest { id });
                } else {
                    self.do_map(id);
                    self.raise(id);
                    self.set_focus(id);
                }
            }
            Request::SetStatus { text } => {
                self.status = text.clone();
                self.emit_panels(&Event::Status { text });
            }
            Request::Quit => self.quit = true,
            Request::GetImage { id, x, y, w, h } => {
                let r = Rect::new(x, y, w.clamp(0, 4096), h.clamp(0, 4096));
                // The screen as composed (without the cursor) or a window's pixels.
                let src: Option<Canvas> = if id == ROOT {
                    self.damage(r);
                    let saved = (self.px, self.py);
                    self.px = -100;
                    self.py = -100;
                    self.compose();
                    let (px, py) = saved;
                    self.px = px;
                    self.py = py;
                    self.damage(cursor::rect(px, py));
                    let mut c = Canvas::new(r.w, r.h);
                    c.blit(&self.screen, r, 0, 0);
                    Some(c)
                } else {
                    self.wins.get(&id).map(|win| {
                        let mut c = Canvas::new(r.w, r.h);
                        c.blit(&win.canvas, r, 0, 0);
                        c
                    })
                };
                let ev = match src {
                    Some(c) => Event::ImageData { w: c.width, h: c.height, pixels: c.pixels },
                    None => Event::Error { code: 4, message: String::from("no such window") },
                };
                self.emit(cid, ev);
            }
            Request::QueryPointer => {
                let (x, y, b) = (self.px, self.py, self.buttons);
                self.emit(cid, Event::Pointer { x, y, buttons: b });
            }
        }
    }

    // ---- input ----------------------------------------------------------

    /// Where pointer events go: a grab, the window holding a button press,
    /// or the window under the pointer.
    fn pointer_target(&self) -> u32 {
        self.pointer_grab.or(self.pressed).unwrap_or_else(|| self.window_at(self.px, self.py))
    }

    fn rel(&self, id: u32) -> (i32, i32) {
        self.wins.get(&id).map_or((self.px, self.py), |w| (self.px - w.rect.x, self.py - w.rect.y))
    }

    fn pointer_moved(&mut self, x: i32, y: i32) {
        let (x, y) = (x.clamp(0, self.width - 1), y.clamp(0, self.height - 1));
        if (x, y) == (self.px, self.py) {
            return;
        }
        let old = cursor::rect(self.px, self.py);
        self.px = x;
        self.py = y;
        self.damage(old);
        self.damage(cursor::rect(x, y));
        // Enter/leave.
        let under = self.window_at(x, y);
        if under != self.under {
            let old = self.under;
            if let Some(c) = self.client_of(old) {
                self.emit(c, Event::Crossing { id: old, entered: false });
            }
            if let Some(c) = self.client_of(under) {
                self.emit(c, Event::Crossing { id: under, entered: true });
            }
            self.under = under;
        }
        let target = self.pointer_target();
        let (wx, wy) = self.rel(target);
        let b = self.buttons;
        if let Some(c) = self.client_of(target) {
            self.emit(c, Event::Motion { id: target, x: wx, y: wy, rx: x, ry: y, buttons: b });
        }
    }

    fn button(&mut self, button: u8, pressed: bool) {
        if pressed {
            self.buttons |= button;
        } else {
            self.buttons &= !button;
        }
        let target = if pressed && button < WHEEL_UP { self.pointer_grab.unwrap_or_else(|| self.window_at(self.px, self.py)) } else { self.pointer_target() };
        let mods = self.mods.mods;
        if pressed && button < WHEEL_UP && self.pressed.is_none() && self.pointer_grab.is_none() {
            self.pressed = Some(target);
            if self.wm.is_some() {
                let (px, py) = (self.px, self.py);
                self.emit_wm(Event::Clicked { id: target, rx: px, ry: py, button, mods });
            } else if target != ROOT {
                self.raise(target);
                if self.wins.get(&target).is_some_and(|w| w.kind != KIND_DOCK && w.kind != KIND_POPUP) {
                    self.set_focus(target);
                }
            }
        }
        let (wx, wy) = self.rel(target);
        let (px, py) = (self.px, self.py);
        if let Some(c) = self.client_of(target) {
            self.emit(c, Event::Button { id: target, x: wx, y: wy, rx: px, ry: py, button, pressed, mods });
        }
        if !pressed && self.buttons & 7 == 0 {
            self.pressed = None;
        }
    }

    fn mouse_event(&mut self, e: &[u8]) {
        let kind = e[0];
        let buttons = e[1] & 7;
        let wheel = e[2] as i8;
        let x = u16::from_le_bytes([e[4], e[5]]);
        let y = u16::from_le_bytes([e[6], e[7]]);
        if kind == 1 {
            let nx = x as i32 * (self.width - 1) / 65535;
            let ny = y as i32 * (self.height - 1) / 65535;
            self.pointer_moved(nx, ny);
        } else {
            let (dx, dy) = (x as i16 as i32, y as i16 as i32);
            let (px, py) = (self.px, self.py);
            self.pointer_moved(px + dx, py - dy);
        }
        for b in [BUTTON_LEFT, BUTTON_RIGHT, BUTTON_MIDDLE] {
            let now = buttons & b != 0;
            if now != (self.buttons & b != 0) {
                self.button(b, now);
            }
        }
        if wheel != 0 {
            let b = if wheel > 0 { WHEEL_UP } else { WHEEL_DOWN };
            self.button(b, true);
            self.button(b, false);
        }
    }

    fn key_event(&mut self, code: u16, pressed: bool) {
        self.mods.update(code, pressed);
        let mods = self.mods.mods;
        if self.key_grabs.contains(&(code, mods)) || (!pressed && self.key_grabs.iter().any(|g| g.0 == code)) {
            self.emit_wm(Event::KeyGrabbed { code, mods, pressed });
            return;
        }
        let focus = self.focus;
        let ch = keymap::character(code, mods, self.mods.caps);
        if let Some(c) = self.client_of(focus) {
            self.emit(c, Event::Key { id: focus, code, pressed, mods, ch });
        } else if let Some(wm) = self.wm {
            // Nothing focused: the window manager may want it (menus).
            self.emit(wm, Event::Key { id: ROOT, code, pressed, mods, ch });
        }
    }

    // ---- I/O --------------------------------------------------------------

    fn read_client(&mut self, cid: u32) {
        let mut buf = [0u8; 65536];
        let Some(c) = self.clients.get_mut(&cid) else { return };
        match net::recv_flags(&c.sock, &mut buf, MSG_DONTWAIT) {
            Ok(0) | Err(huldra_user::Errno::ECONNRESET) => c.dead = true,
            Ok(n) => c.inbuf.extend_from_slice(&buf[..n]),
            Err(_) => {}
        }
        if message_too_long(&c.inbuf) {
            c.dead = true;
        }
        loop {
            let Some(c) = self.clients.get_mut(&cid) else { return };
            let Some((tag, payload, used)) = frame(&c.inbuf) else { break };
            let req = Request::decode(tag, payload);
            c.inbuf.drain(..used);
            match req {
                Some(r) => self.handle(cid, r),
                None => {
                    self.emit(cid, Event::Error { code: 3, message: String::from("unknown request") });
                }
            }
        }
    }

    fn flush_clients(&mut self) {
        for c in self.clients.values_mut() {
            while !c.out.buf.is_empty() && !c.dead {
                match net::send_flags(&c.sock, &c.out.buf, MSG_DONTWAIT) {
                    Ok(n) => {
                        c.out.buf.drain(..n);
                    }
                    Err(huldra_user::Errno::EAGAIN) => {
                        // A slow client: give up on it if it falls far behind.
                        if c.out.buf.len() > 8 << 20 {
                            c.dead = true;
                        }
                        break;
                    }
                    Err(_) => c.dead = true,
                }
            }
        }
        let dead: Vec<u32> = self.clients.iter().filter(|(_, c)| c.dead).map(|(id, _)| *id).collect();
        for id in dead {
            self.drop_client(id);
        }
    }
}

/// The desktop: the glass of an old green monitor, a little brighter in
/// the middle and darker toward the corners, with a boot header.
fn paint_background(root: &mut Canvas, font: &Font) {
    let b = root.bounds();
    let (cx, cy) = (b.w / 2, b.h / 2);
    let max = (cx * cx + cy * cy) as i64;
    let center = huldra_gfx::canvas::mix(theme::BG, theme::LINE_DIM, 70);
    let edge = 0xFF050B08;
    for y in 0..b.h {
        for x in 0..b.w {
            let (dx, dy) = ((x - cx) as i64, (y - cy) as i64);
            // Squared distance, squared again: the edges darken late.
            let d = (dx * dx + dy * dy) * 255 / max;
            root.pixels[(y * b.w + x) as usize] = huldra_gfx::canvas::mix(center, edge, (d * d / 255) as u32);
        }
    }
    let header = [
        "HULDRA INDUSTRIES (TM) UNIFIED OPERATING SYSTEM",
        "COPYRIGHT 2026 HULDRA INDUSTRIES",
        "",
        "> SYSTEM READY_",
    ];
    for (i, line) in header.iter().enumerate() {
        root.draw_text(font, 32, 28 + i as i32 * 18, line, if i == 3 { theme::TEXT } else { theme::TEXT_DIM }, None);
    }
    // A big logo in the corner, drawn 4x from the bitmap font.
    let text = "HULDRA";
    let scale = 4;
    let (w, h) = (font.text_width(text), font.height);
    let mut small = Canvas::new(w, h);
    small.fill_rect(small.bounds(), 0);
    small.draw_text(font, 0, 0, text, theme::LINE_DIM, None);
    let (x0, y0) = (b.w - w * scale - 40, b.h - h * scale - 60);
    for y in 0..h * scale {
        for x in 0..w * scale {
            let p = small.get(x / scale, y / scale);
            if p != 0 {
                root.put(x0 + x, y0 + y, p);
            }
        }
    }
}

fn main() -> i32 {
    let args = env::args();
    let fd = match sys::open("/dev/fb0", O_RDWR, 0) {
        Ok(fd) => fd,
        Err(e) => {
            eprintln!("display: /dev/fb0: {}", e);
            return 1;
        }
    };
    let mut info = [0u32; 40];
    let _ = sys::ioctl(fd, FBIOGET_VSCREENINFO, info.as_mut_ptr() as usize);
    let crt = !args.iter().any(|a| a == "--no-crt");
    if let Some((w, h)) = args.iter().skip(1).find(|a| !a.starts_with('-')).and_then(|m| m.split_once('x')) {
        if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
            info[0] = w;
            info[1] = h;
            if sys::ioctl(fd, FBIOPUT_VSCREENINFO, info.as_mut_ptr() as usize).is_err() {
                eprintln!("display: mode {}x{} not supported", w, h);
            }
        }
    }
    let _ = sys::ioctl(fd, FBIOGET_VSCREENINFO, info.as_mut_ptr() as usize);
    let (width, height) = (info[0] as i32, info[1] as i32);
    let fb = match sys::mmap(0, (width * height * 4) as usize, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0) {
        Ok(p) => p as *mut u32,
        Err(e) => {
            eprintln!("display: mmap: {}", e);
            return 1;
        }
    };
    let kbd = sys::open("/dev/kbd", 0, 0);
    let mouse = sys::open("/dev/mouse", 0, 0);
    let listener = match Socket::tcp().and_then(|s| s.bind(Ip::LOCALHOST, PORT_BASE).and_then(|_| s.listen(32)).map(|_| s)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("display: port {}: {} (already running?)", PORT_BASE, e);
            return 1;
        }
    };
    let font = gui::load_font();
    let mut root = Canvas::new(width, height);
    paint_background(&mut root, &font);
    let mut s = Server {
        fb,
        width,
        height,
        screen: Canvas::new(width, height),
        font,
        root,
        wins: BTreeMap::new(),
        stack: Vec::new(),
        clients: BTreeMap::new(),
        next_client: 1,
        wm: None,
        focus: ROOT,
        px: width / 2,
        py: height / 2,
        buttons: 0,
        pointer_grab: None,
        pressed: None,
        under: ROOT,
        mods: Modifiers::default(),
        key_grabs: Vec::new(),
        damage: Vec::new(),
        status: String::new(),
        quit: false,
        crt,
    };
    s.damage(Rect::new(0, 0, width, height));
    s.compose();
    println!("display: {}x{} on :0 (port {})", width, height, PORT_BASE);

    while !s.quit {
        let mut fds: Vec<PollFd> = Vec::new();
        fds.push(PollFd { fd: listener.fd(), events: POLLIN, revents: 0 });
        fds.push(PollFd { fd: *kbd.as_ref().unwrap_or(&-1), events: POLLIN, revents: 0 });
        fds.push(PollFd { fd: *mouse.as_ref().unwrap_or(&-1), events: POLLIN, revents: 0 });
        let ids: Vec<u32> = s.clients.keys().copied().collect();
        for id in &ids {
            fds.push(PollFd { fd: s.clients[id].sock.fd(), events: POLLIN, revents: 0 });
        }
        if sys::poll(&mut fds, 1000).is_err() {
            continue;
        }
        if fds[0].revents != 0 {
            if let Ok((sock, _, _)) = listener.accept() {
                let id = s.next_client;
                s.next_client += 1;
                s.clients.insert(id, Client { sock, inbuf: Vec::new(), out: Writer::default(), wm: false, select: false, dead: false });
            }
        }
        if fds[1].revents != 0 {
            let mut buf = [0u8; 256];
            if let Ok(n) = sys::read(fds[1].fd, &mut buf) {
                for e in buf[..n].chunks_exact(4) {
                    s.key_event(u16::from_le_bytes([e[0], e[1]]), e[2] != 0);
                }
            }
        }
        if fds[2].revents != 0 {
            let mut buf = [0u8; 512];
            if let Ok(n) = sys::read(fds[2].fd, &mut buf) {
                for e in buf[..n].chunks_exact(8) {
                    s.mouse_event(e);
                }
            }
        }
        for (i, id) in ids.iter().enumerate() {
            if fds[3 + i].revents != 0 {
                s.read_client(*id);
            }
        }
        s.flush_clients();
        s.compose();
        gui::reap_children();
    }
    println!("display: exiting");
    0
}
