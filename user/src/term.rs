//! Terminal control for interactive programs: raw mode, window size, key
//! decoding (VT100/xterm escape sequences) and a screen buffer that only
//! redraws lines that changed.

use crate::io::{write_all, STDIN, STDOUT};
use crate::{sys, Errno, Result};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use huldra_abi::termios::*;

/// True if `fd` is a terminal.
pub fn is_tty(fd: i32) -> bool {
    let mut t = Termios::default();
    sys::ioctl(fd, TCGETS, &mut t as *mut Termios as usize).is_ok()
}

/// (rows, columns) of the terminal.
pub fn size() -> (usize, usize) {
    let mut w = Winsize::default();
    match sys::ioctl(STDOUT, TIOCGWINSZ, &mut w as *mut Winsize as usize) {
        Ok(_) if w.ws_row > 0 && w.ws_col > 0 => (w.ws_row as usize, w.ws_col as usize),
        _ => (25, 80),
    }
}

/// Puts the terminal in raw mode; the old settings come back on drop.
pub struct RawMode {
    saved: Termios,
    fd: i32,
}

impl RawMode {
    pub fn enable() -> Result<RawMode> {
        Self::enable_fd(STDIN)
    }

    pub fn enable_fd(fd: i32) -> Result<RawMode> {
        let mut t = Termios::default();
        sys::ioctl(fd, TCGETS, &mut t as *mut Termios as usize)?;
        let saved = t;
        t.c_iflag &= !ICRNL;
        t.c_lflag &= !(ICANON | ECHO | ISIG | IEXTEN);
        t.c_cc[VMIN] = 1;
        t.c_cc[VTIME] = 0;
        sys::ioctl(fd, TCSETS, &t as *const Termios as usize)?;
        Ok(RawMode { saved, fd })
    }
}

impl RawMode {
    /// Temporarily restores the original settings (to run another program).
    pub fn suspend(&self) {
        crate::io::flush_stdout();
        let _ = sys::ioctl(self.fd, TCSETS, &self.saved as *const Termios as usize);
    }

    /// Re-enters raw mode after [`suspend`](Self::suspend).
    pub fn resume(&self) {
        let mut t = self.saved;
        t.c_iflag &= !ICRNL;
        t.c_lflag &= !(ICANON | ECHO | ISIG | IEXTEN);
        t.c_cc[VMIN] = 1;
        t.c_cc[VTIME] = 0;
        let _ = sys::ioctl(self.fd, TCSETS, &t as *const Termios as usize);
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        crate::io::flush_stdout();
        let _ = sys::ioctl(self.fd, TCSETS, &self.saved as *const Termios as usize);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    /// Ctrl + letter, as the lowercase letter.
    Ctrl(char),
    Enter,
    Tab,
    BackTab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
    Unknown,
}

/// Reads keys from stdin, decoding escape sequences and UTF-8.
pub struct Keys {
    buf: Vec<u8>,
    fd: i32,
}

impl Default for Keys {
    fn default() -> Self {
        Self::new()
    }
}

impl Keys {
    pub fn new() -> Keys {
        Keys { buf: Vec::new(), fd: STDIN }
    }

    /// Reads keys from `fd` (e.g. /dev/tty when stdin is a pipe).
    pub fn from_fd(fd: i32) -> Keys {
        Keys { buf: Vec::new(), fd }
    }

    /// Waits up to `ms` milliseconds for a key.
    pub fn read_timeout(&mut self, ms: i32) -> Result<Option<Key>> {
        if self.buf.is_empty() {
            let mut p = [huldra_abi::fs::PollFd { fd: self.fd, events: huldra_abi::fs::POLLIN, revents: 0 }];
            if sys::poll(&mut p, ms)? == 0 {
                return Ok(None);
            }
        }
        self.read().map(Some)
    }

    fn fill(&mut self) -> Result<()> {
        let mut chunk = [0u8; 64];
        loop {
            match sys::read(self.fd, &mut chunk) {
                Ok(0) => return Err(Errno::EIO),
                Ok(n) => {
                    self.buf.extend_from_slice(&chunk[..n]);
                    return Ok(());
                }
                Err(Errno::EINTR) => continue,
                Err(e) => return Err(e),
            }
        }
    }

    /// Blocks until a key is available.
    pub fn read(&mut self) -> Result<Key> {
        if self.buf.is_empty() {
            self.fill()?;
        }
        let (key, used) = decode(&self.buf);
        self.buf.drain(..used.max(1));
        Ok(key)
    }
}

/// Decodes one key from the start of `b`; returns it and the bytes used.
pub fn decode(b: &[u8]) -> (Key, usize) {
    match b[0] {
        b'\r' | b'\n' => (Key::Enter, 1),
        b'\t' => (Key::Tab, 1),
        0x7F | 0x08 => (Key::Backspace, 1),
        0x1B => decode_escape(b),
        c @ 1..=26 => (Key::Ctrl((b'a' + c - 1) as char), 1),
        c if c < 0x80 => (Key::Char(c as char), 1),
        c => {
            let len = match c {
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF7 => 4,
                _ => 1,
            };
            if b.len() < len {
                return (Key::Unknown, b.len());
            }
            match core::str::from_utf8(&b[..len]).ok().and_then(|s| s.chars().next()) {
                Some(ch) => (Key::Char(ch), len),
                None => (Key::Unknown, 1),
            }
        }
    }
}

fn decode_escape(b: &[u8]) -> (Key, usize) {
    if b.len() == 1 {
        return (Key::Escape, 1);
    }
    match b[1] {
        b'O' if b.len() >= 3 => {
            let k = match b[2] {
                b'P' => Key::F(1),
                b'Q' => Key::F(2),
                b'R' => Key::F(3),
                b'S' => Key::F(4),
                b'H' => Key::Home,
                b'F' => Key::End,
                b'A' => Key::Up,
                b'B' => Key::Down,
                b'C' => Key::Right,
                b'D' => Key::Left,
                _ => Key::Unknown,
            };
            (k, 3)
        }
        b'[' => {
            // CSI: parameters, then a final byte in 0x40..=0x7E.
            let mut i = 2;
            while i < b.len() && !(0x40..=0x7E).contains(&b[i]) {
                i += 1;
            }
            if i >= b.len() {
                return (Key::Escape, 1);
            }
            let params = core::str::from_utf8(&b[2..i]).unwrap_or("");
            let num: u32 = params.split(';').next().and_then(|p| p.parse().ok()).unwrap_or(0);
            let k = match b[i] {
                b'A' => Key::Up,
                b'B' => Key::Down,
                b'C' => Key::Right,
                b'D' => Key::Left,
                b'H' => Key::Home,
                b'F' => Key::End,
                b'Z' => Key::BackTab,
                b'~' => match num {
                    1 | 7 => Key::Home,
                    2 => Key::Insert,
                    3 => Key::Delete,
                    4 | 8 => Key::End,
                    5 => Key::PageUp,
                    6 => Key::PageDown,
                    11..=15 => Key::F((num - 10) as u8),
                    17..=21 => Key::F((num - 11) as u8),
                    23 | 24 => Key::F((num - 12) as u8),
                    _ => Key::Unknown,
                },
                _ => Key::Unknown,
            };
            (k, i + 1)
        }
        _ => (Key::Escape, 1),
    }
}

/// Text attributes for [`Screen`] cells.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Style {
    pub fg: Option<u8>,
    pub bg: Option<u8>,
    pub bold: bool,
    pub reverse: bool,
}

impl Style {
    pub const NORMAL: Style = Style { fg: None, bg: None, bold: false, reverse: false };
    pub const REVERSE: Style = Style { fg: None, bg: None, bold: false, reverse: true };

    pub fn fg(color: u8) -> Style {
        Style { fg: Some(color), ..Style::NORMAL }
    }

    pub fn colors(fg: u8, bg: u8) -> Style {
        Style { fg: Some(fg), bg: Some(bg), ..Style::NORMAL }
    }

    pub fn bold(mut self) -> Style {
        self.bold = true;
        self
    }

    fn sgr(&self, out: &mut String) {
        out.push_str("\x1b[0");
        if self.bold {
            out.push_str(";1");
        }
        if self.reverse {
            out.push_str(";7");
        }
        if let Some(c) = self.fg {
            out.push_str(&alloc::format!(";{}", if c < 8 { 30 + c as u32 } else { 90 + c as u32 - 8 }));
        }
        if let Some(c) = self.bg {
            out.push_str(&alloc::format!(";{}", if c < 8 { 40 + c as u32 } else { 100 + c as u32 - 8 }));
        }
        out.push('m');
    }
}

pub const BLACK: u8 = 0;
pub const RED: u8 = 1;
pub const GREEN: u8 = 2;
pub const YELLOW: u8 = 3;
pub const BLUE: u8 = 4;
pub const MAGENTA: u8 = 5;
pub const CYAN: u8 = 6;
pub const WHITE: u8 = 7;
pub const BRIGHT: u8 = 8;

/// A double-buffered character grid; `present` sends only changed rows.
pub struct Screen {
    pub rows: usize,
    pub cols: usize,
    cells: Vec<(char, Style)>,
    shown: Vec<(char, Style)>,
    cursor: (usize, usize),
    first: bool,
}

impl Screen {
    pub fn new() -> Screen {
        let (rows, cols) = size();
        let blank = (' ', Style::NORMAL);
        Screen { rows, cols, cells: vec![blank; rows * cols], shown: vec![('\0', Style::NORMAL); rows * cols], cursor: (0, 0), first: true }
    }

    pub fn clear(&mut self) {
        self.cells.fill((' ', Style::NORMAL));
    }

    pub fn put(&mut self, row: usize, col: usize, c: char, style: Style) {
        if row < self.rows && col < self.cols {
            self.cells[row * self.cols + col] = (c, style);
        }
    }

    /// Writes `text` at (row, col), clipped to the screen; returns the end column.
    pub fn text(&mut self, row: usize, col: usize, text: &str, style: Style) -> usize {
        let mut c = col;
        for ch in text.chars() {
            if c >= self.cols {
                break;
            }
            let ch = if ch == '\t' || (ch as u32) < 0x20 { ' ' } else { ch };
            self.put(row, c, ch, style);
            c += 1;
        }
        c
    }

    /// Fills the rest of the row from `col` with spaces in `style`.
    pub fn fill_row(&mut self, row: usize, col: usize, style: Style) {
        for c in col..self.cols {
            self.put(row, c, ' ', style);
        }
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor = (row.min(self.rows - 1), col.min(self.cols - 1));
    }

    /// Sends changes to the terminal.
    pub fn present(&mut self) {
        let mut out = String::new();
        if self.first {
            out.push_str("\x1b[0m\x1b[2J");
            self.first = false;
        }
        out.push_str("\x1b[?25l");
        for r in 0..self.rows {
            let range = r * self.cols..(r + 1) * self.cols;
            if self.cells[range.clone()] == self.shown[range.clone()] {
                continue;
            }
            out.push_str(&alloc::format!("\x1b[{};1H", r + 1));
            let mut style = None;
            // Avoid writing the bottom-right cell, which would scroll some terminals.
            let last = if r + 1 == self.rows { self.cols - 1 } else { self.cols };
            for c in 0..last {
                let (ch, st) = self.cells[r * self.cols + c];
                if style != Some(st) {
                    st.sgr(&mut out);
                    style = Some(st);
                }
                out.push(ch);
            }
            self.shown[range.clone()].copy_from_slice(&self.cells[range]);
        }
        out.push_str(&alloc::format!("\x1b[0m\x1b[{};{}H\x1b[?25h", self.cursor.0 + 1, self.cursor.1 + 1));
        let _ = write_all(STDOUT, out.as_bytes());
    }

    /// Forces a full redraw on the next `present`.
    pub fn invalidate(&mut self) {
        self.shown.fill(('\0', Style::NORMAL));
        self.first = true;
    }
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

/// Clears the screen and puts the cursor home (on leaving a full-screen app).
pub fn reset_screen() {
    let _ = write_all(STDOUT, b"\x1b[0m\x1b[2J\x1b[H\x1b[?25h");
}

/// Display width of a string (one column per char; tabs expanded to 8).
pub fn display_width(s: &str) -> usize {
    let mut w = 0;
    for c in s.chars() {
        if c == '\t' {
            w = (w + 8) & !7;
        } else {
            w += 1;
        }
    }
    w
}
