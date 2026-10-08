//! Display server client: connect, create windows, draw, receive events.
//!
//! ```ignore
//! let mut d = Display::connect()?;
//! let w = d.create_window(100, 100, 300, 200, KIND_NORMAL);
//! d.set_title(w, "Hello");
//! d.map(w);
//! while let Some(ev) = d.wait_event(-1) {
//!     if let Event::Expose { .. } = ev { d.fill(w, Rect::new(0, 0, 300, 200), BG); d.flush(); }
//! }
//! ```

use crate::net::Socket;
use crate::{env, sys, Errno, Result, String, Vec};
use alloc::collections::VecDeque;
use huldra_abi::fs::{PollFd, POLLIN};
use huldra_abi::net::MSG_DONTWAIT;
pub use huldra_gfx::canvas::{rgb, Canvas, Rect};
pub use huldra_gfx::font::Font;
pub use huldra_gfx::proto::*;
use huldra_net::Ip;

pub const FONT_W: i32 = 8;
pub const FONT_H: i32 = 16;

pub struct Display {
    sock: Socket,
    pub client: u32,
    next: u32,
    pub width: i32,
    pub height: i32,
    out: Writer,
    inbuf: Vec<u8>,
    queue: VecDeque<Event>,
    pub closed: bool,
}

/// `DISPLAY`: `:N` or `HOST:N` (TCP port 6000 + N). Default `:0`.
fn address() -> (Ip, u16) {
    let d = env::var("DISPLAY").unwrap_or(":0");
    let (host, n) = d.rsplit_once(':').unwrap_or(("", d));
    let n: u16 = n.split('.').next().and_then(|n| n.parse().ok()).unwrap_or(0);
    let ip = if host.is_empty() || host == "localhost" || host == "unix" { Ip::LOCALHOST } else { Ip::parse(host).unwrap_or(Ip::LOCALHOST) };
    (ip, PORT_BASE + n)
}

impl Display {
    pub fn connect() -> Result<Display> {
        let (ip, port) = address();
        let sock = Socket::tcp()?;
        sock.connect(ip, port)?;
        let mut d = Display { sock, client: 0, next: 1, width: 0, height: 0, out: Writer::default(), inbuf: Vec::new(), queue: VecDeque::new(), closed: false };
        d.send(Request::Hello { version: VERSION });
        d.flush();
        loop {
            match d.read_event(5000)? {
                Some(Event::Welcome { client, width, height }) => {
                    d.client = client;
                    d.width = width;
                    d.height = height;
                    return Ok(d);
                }
                Some(e) => d.queue.push_back(e),
                None => return Err(Errno::ETIMEDOUT),
            }
        }
    }

    pub fn fd(&self) -> i32 {
        self.sock.fd()
    }

    pub fn send(&mut self, r: Request) {
        r.encode(&mut self.out);
        if self.out.buf.len() > 64 * 1024 {
            self.flush();
        }
    }

    pub fn flush(&mut self) {
        if !self.out.buf.is_empty() {
            if self.sock.send_all(&self.out.buf).is_err() {
                self.closed = true;
            }
            self.out.buf.clear();
        }
    }

    pub fn new_id(&mut self) -> u32 {
        self.next += 1;
        self.client << 20 | self.next
    }

    pub fn create_window(&mut self, x: i32, y: i32, w: i32, h: i32, kind: u8) -> u32 {
        let id = self.new_id();
        self.send(Request::CreateWindow { id, x, y, w, h, kind });
        id
    }

    pub fn map(&mut self, id: u32) {
        self.send(Request::Map { id });
    }

    pub fn unmap(&mut self, id: u32) {
        self.send(Request::Unmap { id });
    }

    pub fn destroy(&mut self, id: u32) {
        self.send(Request::DestroyWindow { id });
    }

    pub fn configure(&mut self, id: u32, r: Rect) {
        self.send(Request::Configure { id, x: r.x, y: r.y, w: r.w, h: r.h });
    }

    pub fn set_title(&mut self, id: u32, title: &str) {
        self.send(Request::SetTitle { id, title: String::from(title) });
    }

    pub fn fill(&mut self, id: u32, r: Rect, color: u32) {
        self.send(Request::Fill { id, x: r.x, y: r.y, w: r.w, h: r.h, color });
    }

    /// Text with a background (`bg` 0: transparent).
    pub fn text(&mut self, id: u32, x: i32, y: i32, fg: u32, bg: u32, text: &str) {
        self.send(Request::Text { id, x, y, fg, bg, text: String::from(text) });
    }

    pub fn line(&mut self, id: u32, x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
        self.send(Request::Line { id, x0, y0, x1, y1, color });
    }

    pub fn circle(&mut self, id: u32, x: i32, y: i32, r: i32, color: u32) {
        self.send(Request::Circle { id, x, y, r, color });
    }

    /// Sends a locally drawn canvas (or part of it) to the window.
    pub fn put_canvas(&mut self, id: u32, c: &Canvas, src: Rect, x: i32, y: i32) {
        let src = src.intersect(&c.bounds());
        if src.is_empty() {
            return;
        }
        let mut pixels = Vec::with_capacity((src.w * src.h) as usize);
        for row in src.y..src.bottom() {
            let start = (row * c.width + src.x) as usize;
            pixels.extend_from_slice(&c.pixels[start..start + src.w as usize]);
        }
        self.send(Request::Image { id, x, y, w: src.w, h: src.h, pixels });
    }

    /// Reads from the socket; `None` on timeout. Errors mean the server
    /// is gone.
    fn read_event(&mut self, timeout_ms: i32) -> Result<Option<Event>> {
        loop {
            if let Some((tag, payload, used)) = frame(&self.inbuf) {
                let e = Event::decode(tag, payload);
                self.inbuf.drain(..used);
                match e {
                    Some(e) => return Ok(Some(e)),
                    None => continue,
                }
            }
            let mut fds = [PollFd { fd: self.sock.fd(), events: POLLIN, revents: 0 }];
            if sys::poll(&mut fds, timeout_ms)? == 0 {
                return Ok(None);
            }
            let mut buf = [0u8; 16384];
            let n = self.sock.recv(&mut buf)?;
            if n == 0 {
                self.closed = true;
                return Err(Errno::ECONNRESET);
            }
            self.inbuf.extend_from_slice(&buf[..n]);
        }
    }

    /// Next event, waiting up to `timeout_ms` (-1: forever). `None` on
    /// timeout or when the display closed (`closed` tells which).
    pub fn wait_event(&mut self, timeout_ms: i32) -> Option<Event> {
        self.flush();
        if let Some(e) = self.queue.pop_front() {
            return Some(e);
        }
        if self.closed {
            return None;
        }
        match self.read_event(timeout_ms) {
            Ok(e) => e,
            Err(_) => {
                self.closed = true;
                None
            }
        }
    }

    /// An event if one is ready now.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.wait_event(0)
    }

    /// Takes whatever the socket has without blocking (for programs that
    /// poll several descriptors themselves).
    pub fn receive_ready(&mut self) {
        let mut buf = [0u8; 16384];
        loop {
            match crate::net::recv_flags(&self.sock, &mut buf, MSG_DONTWAIT) {
                Ok(0) => {
                    self.closed = true;
                    return;
                }
                Ok(n) => self.inbuf.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        while let Some((tag, payload, used)) = frame(&self.inbuf) {
            if let Some(e) = Event::decode(tag, payload) {
                self.queue.push_back(e);
            }
            self.inbuf.drain(..used);
        }
    }
}

/// Starts a program in the background (for launchers and window managers).
pub fn spawn(cmd: &str) -> bool {
    let args: Vec<&str> = cmd.split_whitespace().collect();
    let Some(&prog) = args.first() else { return false };
    let path = if prog.contains('/') { Some(String::from(prog)) } else { crate::process::find_in_path(prog) };
    let Some(path) = path else { return false };
    match crate::process::fork() {
        Ok(None) => {
            let _ = sys::setsid();
            let e = crate::process::exec(&path, &args, &env::envp());
            crate::eprintln!("{}: {}", path, e);
            crate::process::exit(127)
        }
        Ok(Some(_)) => true,
        Err(_) => false,
    }
}

/// Reaps finished children without blocking.
pub fn reap_children() {
    while let Ok(Some(_)) = crate::process::try_wait(-1) {}
}

/// Loads the VGA font from /dev/font.
pub fn load_font() -> Font {
    match crate::fs::read("/dev/font") {
        Ok(data) if data.len() == 4096 => Font::from_vga(data),
        _ => Font::fallback(),
    }
}
