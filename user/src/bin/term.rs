//! term [-e command...]: the graphical terminal emulator (an xterm).
//!
//! Runs a shell (or the command) on a pseudo-terminal and shows it in a
//! window. Shift+PageUp/PageDown or the mouse wheel scroll through
//! history.

#![no_std]
#![no_main]

use huldra_gfx::keymap::{self, PGDN, PGUP};
use huldra_gfx::term::{Term, DEFAULT_BG};
use huldra_user::abi::fs::{PollFd, O_RDWR, POLLHUP, POLLIN};
use huldra_user::abi::termios::{Winsize, TIOCGPTN, TIOCSCTTY, TIOCSWINSZ};
use huldra_user::gui::*;
use huldra_user::{env, eprintln, format, process, signal, sys, String, Vec};

huldra_user::main!(main);

const PAD: i32 = 4;

struct App {
    d: Display,
    win: u32,
    term: Term,
    master: i32,
    focused: bool,
    cursor_drawn: Option<(usize, usize)>,
}

impl App {
    fn draw_row(&mut self, y: usize) {
        let t = &self.term;
        let py = PAD + y as i32 * FONT_H;
        let mut x = 0;
        let mut ops: Vec<(i32, u32, u32, String, bool)> = Vec::new();
        while x < t.cols {
            let a = t.cell(x, y).attr;
            let (fg, bg) = a.colors();
            let start = x;
            let mut s = String::new();
            while x < t.cols && t.cell(x, y).attr == a {
                s.push(t.cell(x, y).ch);
                x += 1;
            }
            ops.push((PAD + start as i32 * FONT_W, fg, bg, s, a.underline));
        }
        for (px, fg, bg, s, underline) in ops {
            let n = s.chars().count() as i32;
            self.d.text(self.win, px, py, fg, bg, &s);
            if underline {
                self.d.fill(self.win, Rect::new(px, py + FONT_H - 2, n * FONT_W, 1), fg);
            }
        }
    }

    fn draw_cursor(&mut self) {
        let t = &self.term;
        if !t.cursor_visible || t.view != 0 {
            self.cursor_drawn = None;
            return;
        }
        let (x, y) = (t.cx.min(t.cols - 1), t.cy);
        let cell = t.cell(x, y);
        let (fg, bg) = cell.attr.colors();
        let (px, py) = (PAD + x as i32 * FONT_W, PAD + y as i32 * FONT_H);
        if self.focused {
            let mut b = [0u8; 4];
            self.d.text(self.win, px, py, bg, fg, cell.ch.encode_utf8(&mut b));
        } else {
            let r = Rect::new(px, py, FONT_W, FONT_H);
            self.d.fill(self.win, Rect::new(r.x, r.y, r.w, 1), fg);
            self.d.fill(self.win, Rect::new(r.x, r.bottom() - 1, r.w, 1), fg);
            self.d.fill(self.win, Rect::new(r.x, r.y, 1, r.h), fg);
            self.d.fill(self.win, Rect::new(r.right() - 1, r.y, 1, r.h), fg);
        }
        self.cursor_drawn = Some((x, y));
    }

    fn redraw(&mut self) {
        // Erase the old cursor by redrawing its row.
        if let Some((_, y)) = self.cursor_drawn {
            if y < self.term.rows {
                self.draw_row(y);
            }
        }
        for y in self.term.take_dirty() {
            self.draw_row(y);
        }
        self.draw_cursor();
        self.d.flush();
    }

    fn full_redraw(&mut self, w: i32, h: i32) {
        self.d.fill(self.win, Rect::new(0, 0, w, h), DEFAULT_BG);
        self.term.mark_all_dirty();
        self.redraw();
    }

    fn resize(&mut self, w: i32, h: i32) {
        let cols = ((w - 2 * PAD) / FONT_W).max(2) as usize;
        let rows = ((h - 2 * PAD) / FONT_H).max(1) as usize;
        if cols != self.term.cols || rows != self.term.rows {
            self.term.resize(cols, rows);
            let ws = Winsize { ws_row: rows as u16, ws_col: cols as u16, ws_xpixel: w as u16, ws_ypixel: h as u16 };
            let _ = sys::ioctl(self.master, TIOCSWINSZ, &ws as *const _ as usize);
        }
        self.full_redraw(w, h);
    }
}

fn spawn_child(n: u32, cmd: &[String]) -> core::result::Result<u32, huldra_user::Errno> {
    let path = format!("/dev/pts/{}", n);
    match process::fork()? {
        None => {
            let _ = sys::setsid();
            let fd = sys::open(&path, O_RDWR, 0).unwrap_or(-1);
            if fd < 0 {
                process::exit(126);
            }
            let _ = sys::ioctl(fd, TIOCSCTTY, 0);
            for target in 0..3 {
                let _ = sys::dup2(fd, target);
            }
            if fd > 2 {
                let _ = sys::close(fd);
            }
            env::set_var("TERM", "xterm");
            let (prog, args): (String, Vec<&str>) = if cmd.is_empty() {
                (String::from(env::var("SHELL").unwrap_or("/bin/sh")), Vec::from(["-sh"]))
            } else {
                (process::find_in_path(&cmd[0]).unwrap_or_else(|| cmd[0].clone()), cmd.iter().map(|s| s.as_str()).collect())
            };
            let e = process::exec(&prog, &args, &env::envp());
            eprintln!("term: {}: {}", prog, e);
            process::exit(127)
        }
        Some(pid) => Ok(pid),
    }
}

fn main() -> i32 {
    let args = env::args();
    let cmd: Vec<String> = match args.iter().position(|a| a == "-e") {
        Some(i) => args[i + 1..].to_vec(),
        None => Vec::new(),
    };
    let mut d = match Display::connect() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("term: cannot connect to the display: {}", e);
            return 1;
        }
    };
    let master = match sys::open("/dev/ptmx", O_RDWR, 0) {
        Ok(fd) => fd,
        Err(e) => {
            eprintln!("term: /dev/ptmx: {}", e);
            return 1;
        }
    };
    let mut n = 0u32;
    let _ = sys::ioctl(master, TIOCGPTN, &mut n as *mut u32 as usize);
    let (cols, rows) = (80, 24);
    let (w, h) = (cols * FONT_W + 2 * PAD, rows * FONT_H + 2 * PAD);
    let win = d.create_window(60 + (d.client as i32 % 6) * 30, 60 + (d.client as i32 % 6) * 30, w, h, KIND_NORMAL);
    let title = if cmd.is_empty() { String::from("Terminal") } else { cmd.join(" ") };
    d.set_title(win, &title);
    d.map(win);
    d.flush();
    let ws = Winsize { ws_row: rows as u16, ws_col: cols as u16, ws_xpixel: w as u16, ws_ypixel: h as u16 };
    let _ = sys::ioctl(master, TIOCSWINSZ, &ws as *const _ as usize);
    let child = match spawn_child(n, &cmd) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("term: fork: {}", e);
            return 1;
        }
    };
    let mut app = App { d, win, term: Term::new(cols as usize, rows as usize), master, focused: false, cursor_drawn: None };
    let mut buf = [0u8; 8192];
    loop {
        let mut fds = [PollFd { fd: app.d.fd(), events: POLLIN, revents: 0 }, PollFd { fd: master, events: POLLIN, revents: 0 }];
        let _ = sys::poll(&mut fds, 500);
        if fds[1].revents & (POLLIN | POLLHUP) != 0 {
            if let Ok(k) = sys::read(master, &mut buf) {
                app.term.feed(&buf[..k]);
                let resp = app.term.take_responses();
                if !resp.is_empty() {
                    let _ = sys::write(master, &resp);
                }
                if let Some(t) = app.term.take_title() {
                    app.d.set_title(win, &t);
                }
                app.redraw();
            }
        }
        if fds[0].revents != 0 {
            app.d.receive_ready();
        }
        while let Some(ev) = app.d.poll_event() {
            match ev {
                Event::Expose { w, h, .. } => app.resize(w, h),
                Event::Key { code, pressed: true, mods, ch, .. } => {
                    if mods & MOD_SHIFT != 0 && (code == PGUP || code == PGDN) {
                        let page = app.term.rows as isize / 2;
                        app.term.scroll_view(if code == PGUP { page } else { -page });
                        app.redraw();
                        continue;
                    }
                    let bytes = keymap::terminal_bytes(code, mods, ch, app.term.app_cursor);
                    if !bytes.is_empty() {
                        if app.term.view != 0 {
                            app.term.scroll_view(-(app.term.view as isize));
                            app.redraw();
                        }
                        let _ = sys::write(master, &bytes);
                    }
                }
                Event::Button { button, pressed: true, .. } if button == WHEEL_UP || button == WHEEL_DOWN => {
                    app.term.scroll_view(if button == WHEEL_UP { 3 } else { -3 });
                    app.redraw();
                }
                Event::Focus { focused, .. } => {
                    app.focused = focused;
                    app.redraw();
                }
                Event::CloseRequest { .. } => {
                    let _ = signal::kill(child as i32, huldra_user::abi::signal::SIGHUP);
                    return 0;
                }
                _ => {}
            }
        }
        if app.d.closed {
            let _ = signal::kill(child as i32, huldra_user::abi::signal::SIGHUP);
            return 0;
        }
        if let Ok(Some(_)) = process::try_wait(child as i32) {
            return 0;
        }
    }
}
