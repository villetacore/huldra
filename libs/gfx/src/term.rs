//! The core of a terminal emulator: an xterm-compatible (VT100 plus common
//! extensions) state machine over a grid of cells, with scrollback. A
//! graphical terminal feeds it the program's output and draws the cells
//! it reports as changed.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// The 16 standard colors (xterm-like, slightly softened).
pub const PALETTE: [u32; 16] = [
    0xFF1D1F21, 0xFFCC6666, 0xFFB5BD68, 0xFFF0C674, 0xFF81A2BE, 0xFFB294BB, 0xFF8ABEB7, 0xFFC5C8C6, 0xFF666666, 0xFFFF7B7B, 0xFFD5E26B, 0xFFFFE08A, 0xFF9CC4F0, 0xFFD4A8E0, 0xFF9EE6DD, 0xFFFFFFFF,
];
pub const DEFAULT_FG: u32 = 0xFFC5C8C6;
pub const DEFAULT_BG: u32 = 0xFF1D1F21;

const SCROLLBACK: usize = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attr {
    pub fg: u32,
    pub bg: u32,
    pub bold: bool,
    pub underline: bool,
    pub inverse: bool,
}

impl Default for Attr {
    fn default() -> Attr {
        Attr { fg: DEFAULT_FG, bg: DEFAULT_BG, bold: false, underline: false, inverse: false }
    }
}

impl Attr {
    /// Foreground and background as drawn.
    pub fn colors(&self) -> (u32, u32) {
        if self.inverse {
            (self.bg, self.fg)
        } else {
            (self.fg, self.bg)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub attr: Attr,
}

impl Default for Cell {
    fn default() -> Cell {
        Cell { ch: ' ', attr: Attr::default() }
    }
}

enum State {
    Ground,
    Escape,
    /// ESC ( and friends: one designator byte follows.
    Charset,
    Csi { params: Vec<u16>, cur: Option<u16>, private: bool },
    Osc(Vec<u8>),
    OscEscape(Vec<u8>),
}

pub struct Term {
    pub cols: usize,
    pub rows: usize,
    cells: Vec<Cell>,
    pub cx: usize,
    pub cy: usize,
    attr: Attr,
    saved: (usize, usize, Attr),
    top: usize,
    bottom: usize,
    wrap_pending: bool,
    pub autowrap: bool,
    pub cursor_visible: bool,
    pub app_cursor: bool,
    alt: Option<(Vec<Cell>, usize, usize)>,
    dirty: Vec<bool>,
    state: State,
    utf8: (u32, u8),
    responses: Vec<u8>,
    title: Option<String>,
    scrollback: VecDeque<Vec<Cell>>,
    /// Lines scrolled back into view (0 = live).
    pub view: usize,
    pub bell: bool,
}

impl Term {
    pub fn new(cols: usize, rows: usize) -> Term {
        let (cols, rows) = (cols.max(1), rows.max(1));
        Term {
            cols,
            rows,
            cells: vec![Cell::default(); cols * rows],
            cx: 0,
            cy: 0,
            attr: Attr::default(),
            saved: (0, 0, Attr::default()),
            top: 0,
            bottom: rows - 1,
            wrap_pending: false,
            autowrap: true,
            cursor_visible: true,
            app_cursor: false,
            alt: None,
            dirty: vec![true; rows],
            state: State::Ground,
            utf8: (0, 0),
            responses: Vec::new(),
            title: None,
            scrollback: VecDeque::new(),
            view: 0,
            bell: false,
        }
    }

    /// The cell shown at (`x`, `y`), taking the scrollback view into account.
    pub fn cell(&self, x: usize, y: usize) -> Cell {
        if self.view > 0 && self.alt.is_none() {
            let sb = self.scrollback.len();
            let line = sb as isize - self.view as isize + y as isize;
            if line < sb as isize {
                return if line < 0 { Cell::default() } else { self.scrollback[line as usize].get(x).copied().unwrap_or_default() };
            }
            return self.cells[(line as usize - sb) * self.cols + x];
        }
        self.cells[y * self.cols + x]
    }

    /// Rows changed since the last call.
    pub fn take_dirty(&mut self) -> Vec<usize> {
        let v = (0..self.rows).filter(|&r| self.dirty[r]).collect();
        self.dirty.iter_mut().for_each(|d| *d = false);
        v
    }

    pub fn mark_all_dirty(&mut self) {
        self.dirty.iter_mut().for_each(|d| *d = true);
    }

    /// Replies to send back to the program (cursor reports and the like).
    pub fn take_responses(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.responses)
    }

    /// A new window title set by the program.
    pub fn take_title(&mut self) -> Option<String> {
        self.title.take()
    }

    /// Scrolls the view by `lines` (positive = into history).
    pub fn scroll_view(&mut self, lines: isize) {
        let max = if self.alt.is_some() { 0 } else { self.scrollback.len() };
        self.view = (self.view as isize + lines).clamp(0, max as isize) as usize;
        self.mark_all_dirty();
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let (cols, rows) = (cols.max(1), rows.max(1));
        if cols == self.cols && rows == self.rows {
            return;
        }
        // Keep the bottom of the screen (where the cursor usually is).
        let mut cells = vec![Cell::default(); cols * rows];
        let shift = (self.cy + 1).saturating_sub(rows);
        for y in 0..rows.min(self.rows - shift) {
            for x in 0..cols.min(self.cols) {
                cells[y * cols + x] = self.cells[(y + shift) * self.cols + x];
            }
        }
        for y in 0..shift {
            let line = self.cells[y * self.cols..(y + 1) * self.cols].to_vec();
            self.push_scrollback(line);
        }
        self.cells = cells;
        self.cols = cols;
        self.rows = rows;
        self.cy = (self.cy - shift).min(rows - 1);
        self.cx = self.cx.min(cols - 1);
        self.top = 0;
        self.bottom = rows - 1;
        self.dirty = vec![true; rows];
        self.wrap_pending = false;
        self.alt = None;
    }

    fn push_scrollback(&mut self, line: Vec<Cell>) {
        if self.scrollback.len() == SCROLLBACK {
            self.scrollback.pop_front();
        }
        self.scrollback.push_back(line);
    }

    fn blank(&self) -> Cell {
        Cell { ch: ' ', attr: Attr { bold: false, underline: false, inverse: false, ..self.attr } }
    }

    fn clear_cells(&mut self, from: usize, to: usize) {
        let b = self.blank();
        for c in &mut self.cells[from..to] {
            *c = b;
        }
        for r in from / self.cols..=(to.saturating_sub(1)) / self.cols {
            if r < self.rows {
                self.dirty[r] = true;
            }
        }
    }

    /// Scrolls the region [top, bottom] up by `n` lines.
    fn scroll_up(&mut self, n: usize) {
        self.scroll_lines(n, true);
    }

    /// Scrolls up; lines leaving the top of the screen go to the
    /// scrollback if `history`.
    fn scroll_lines(&mut self, n: usize, history: bool) {
        let (top, bottom, cols) = (self.top, self.bottom, self.cols);
        let n = n.min(bottom - top + 1);
        if history && top == 0 && self.alt.is_none() {
            for r in 0..n {
                let line = self.cells[r * cols..(r + 1) * cols].to_vec();
                self.push_scrollback(line);
            }
        }
        self.cells.copy_within((top + n) * cols..(bottom + 1) * cols, top * cols);
        self.clear_cells((bottom + 1 - n) * cols, (bottom + 1) * cols);
        for r in top..=bottom {
            self.dirty[r] = true;
        }
    }

    fn scroll_down(&mut self, n: usize) {
        let (top, bottom, cols) = (self.top, self.bottom, self.cols);
        let n = n.min(bottom - top + 1);
        self.cells.copy_within(top * cols..(bottom + 1 - n) * cols, (top + n) * cols);
        self.clear_cells(top * cols, (top + n) * cols);
        for r in top..=bottom {
            self.dirty[r] = true;
        }
    }

    fn newline(&mut self) {
        if self.cy == self.bottom {
            self.scroll_up(1);
        } else if self.cy + 1 < self.rows {
            self.cy += 1;
        }
    }

    fn put(&mut self, ch: char) {
        if self.wrap_pending {
            self.wrap_pending = false;
            self.cx = 0;
            self.newline();
        }
        let i = self.cy * self.cols + self.cx;
        self.cells[i] = Cell { ch, attr: self.attr };
        self.dirty[self.cy] = true;
        if self.cx + 1 >= self.cols {
            if self.autowrap {
                self.wrap_pending = true;
            }
        } else {
            self.cx += 1;
        }
    }

    /// Processes program output.
    pub fn feed(&mut self, bytes: &[u8]) {
        if self.view != 0 {
            self.view = 0;
            self.mark_all_dirty();
        }
        let old = (self.cx, self.cy);
        for &b in bytes {
            self.byte(b);
        }
        // The cursor moved: both rows need redrawing.
        if old != (self.cx, self.cy) {
            self.dirty[old.1.min(self.rows - 1)] = true;
            self.dirty[self.cy] = true;
        }
    }

    fn byte(&mut self, b: u8) {
        match core::mem::replace(&mut self.state, State::Ground) {
            State::Ground => self.ground(b),
            State::Escape => self.escape(b),
            State::Charset => {}
            State::Csi { mut params, mut cur, mut private } => match b {
                b'0'..=b'9' => {
                    cur = Some(cur.unwrap_or(0).saturating_mul(10).saturating_add((b - b'0') as u16));
                    self.state = State::Csi { params, cur, private };
                }
                b';' | b':' => {
                    params.push(cur.unwrap_or(0));
                    self.state = State::Csi { params, cur: None, private };
                }
                b'?' | b'>' | b'=' | b'!' => {
                    private = true;
                    self.state = State::Csi { params, cur, private };
                }
                b' ' | b'"' | b'\'' | b'$' => self.state = State::Csi { params, cur, private },
                0x18 | 0x1A => {}
                0x1B => self.state = State::Escape,
                0x40..=0x7E => {
                    if let Some(c) = cur {
                        params.push(c);
                    }
                    self.csi(b, &params, private);
                }
                _ => self.state = State::Csi { params, cur, private },
            },
            State::Osc(mut buf) => match b {
                0x07 => self.osc(&buf),
                0x1B => self.state = State::OscEscape(buf),
                _ => {
                    if buf.len() < 512 {
                        buf.push(b);
                    }
                    self.state = State::Osc(buf);
                }
            },
            State::OscEscape(buf) => {
                // ESC \ ends the string.
                self.osc(&buf);
                if b != b'\\' {
                    self.escape(b);
                }
            }
        }
    }

    fn osc(&mut self, buf: &[u8]) {
        let s = String::from_utf8_lossy(buf);
        if let Some((code, text)) = s.split_once(';') {
            if code == "0" || code == "2" {
                self.title = Some(String::from(text));
            }
        }
    }

    fn ground(&mut self, b: u8) {
        // UTF-8 decoding.
        if self.utf8.1 > 0 {
            if b & 0xC0 == 0x80 {
                self.utf8.0 = (self.utf8.0 << 6) | (b & 0x3F) as u32;
                self.utf8.1 -= 1;
                if self.utf8.1 == 0 {
                    self.put(char::from_u32(self.utf8.0).unwrap_or('?'));
                }
                return;
            }
            self.utf8.1 = 0;
            self.put('?');
        }
        match b {
            0x07 => self.bell = true,
            0x08 => {
                self.cx = self.cx.saturating_sub(1);
                self.wrap_pending = false;
            }
            0x09 => {
                self.cx = ((self.cx / 8 + 1) * 8).min(self.cols - 1);
                self.wrap_pending = false;
            }
            0x0A..=0x0C => {
                self.wrap_pending = false;
                self.newline();
            }
            0x0D => {
                self.cx = 0;
                self.wrap_pending = false;
            }
            0x1B => self.state = State::Escape,
            0x00..=0x1F | 0x7F => {}
            0x20..=0x7E => self.put(b as char),
            0xC0..=0xDF => self.utf8 = ((b & 0x1F) as u32, 1),
            0xE0..=0xEF => self.utf8 = ((b & 0x0F) as u32, 2),
            0xF0..=0xF7 => self.utf8 = ((b & 0x07) as u32, 3),
            _ => self.put('?'),
        }
    }

    fn escape(&mut self, b: u8) {
        match b {
            b'[' => self.state = State::Csi { params: Vec::new(), cur: None, private: false },
            b']' => self.state = State::Osc(Vec::new()),
            b'(' | b')' | b'*' | b'+' | b'#' | b'%' => self.state = State::Charset,
            b'7' => self.saved = (self.cx, self.cy, self.attr),
            b'8' => {
                (self.cx, self.cy, self.attr) = self.saved;
                self.cx = self.cx.min(self.cols - 1);
                self.cy = self.cy.min(self.rows - 1);
                self.wrap_pending = false;
            }
            b'D' => self.newline(),
            b'E' => {
                self.cx = 0;
                self.newline();
            }
            b'M' => {
                if self.cy == self.top {
                    self.scroll_down(1);
                } else {
                    self.cy = self.cy.saturating_sub(1);
                }
            }
            b'c' => {
                let (c, r) = (self.cols, self.rows);
                let sb = core::mem::take(&mut self.scrollback);
                *self = Term::new(c, r);
                self.scrollback = sb;
            }
            _ => {}
        }
    }

    fn csi(&mut self, cmd: u8, p: &[u16], private: bool) {
        let arg = |i: usize, default: u16| -> usize {
            match p.get(i) {
                Some(&0) | None => default as usize,
                Some(&v) => v as usize,
            }
        };
        let n = arg(0, 1);
        let (cols, rows) = (self.cols, self.rows);
        self.wrap_pending = false;
        match (cmd, private) {
            (b'h' | b'l', true) => {
                let on = cmd == b'h';
                for &mode in p {
                    match mode {
                        1 => self.app_cursor = on,
                        7 => self.autowrap = on,
                        25 => {
                            self.cursor_visible = on;
                            self.dirty[self.cy] = true;
                        }
                        47 | 1047 | 1049 => self.alternate(on),
                        _ => {}
                    }
                }
            }
            (b'c', _) => self.responses.extend_from_slice(b"\x1b[?6c"),
            (_, true) => {}
            (b'A', _) => self.cy = self.cy.saturating_sub(n).max(if self.cy >= self.top { self.top } else { 0 }),
            (b'B', _) => self.cy = (self.cy + n).min(if self.cy <= self.bottom { self.bottom } else { rows - 1 }),
            (b'C', _) => self.cx = (self.cx + n).min(cols - 1),
            (b'D', _) => self.cx = self.cx.saturating_sub(n),
            (b'E', _) => {
                self.cx = 0;
                self.cy = (self.cy + n).min(rows - 1);
            }
            (b'F', _) => {
                self.cx = 0;
                self.cy = self.cy.saturating_sub(n);
            }
            (b'G' | b'`', _) => self.cx = (n - 1).min(cols - 1),
            (b'd', _) => self.cy = (n - 1).min(rows - 1),
            (b'H' | b'f', _) => {
                self.cy = (arg(0, 1) - 1).min(rows - 1);
                self.cx = (arg(1, 1) - 1).min(cols - 1);
            }
            (b'J', _) => {
                let at = self.cy * cols + self.cx;
                match p.first().copied().unwrap_or(0) {
                    0 => self.clear_cells(at, cols * rows),
                    1 => self.clear_cells(0, at + 1),
                    _ => self.clear_cells(0, cols * rows),
                }
            }
            (b'K', _) => {
                let line = self.cy * cols;
                match p.first().copied().unwrap_or(0) {
                    0 => self.clear_cells(line + self.cx, line + cols),
                    1 => self.clear_cells(line, line + self.cx + 1),
                    _ => self.clear_cells(line, line + cols),
                }
            }
            (b'L' | b'M', _) => {
                if self.cy >= self.top && self.cy <= self.bottom {
                    let saved_top = self.top;
                    self.top = self.cy;
                    if cmd == b'L' {
                        self.scroll_down(n);
                    } else {
                        // Deleted lines are not history.
                        self.scroll_lines(n, false);
                    }
                    self.top = saved_top;
                    self.cx = 0;
                }
            }
            (b'P', _) => {
                let line = self.cy * cols;
                let n = n.min(cols - self.cx);
                self.cells.copy_within(line + self.cx + n..line + cols, line + self.cx);
                self.clear_cells(line + cols - n, line + cols);
            }
            (b'@', _) => {
                let line = self.cy * cols;
                let n = n.min(cols - self.cx);
                self.cells.copy_within(line + self.cx..line + cols - n, line + self.cx + n);
                self.clear_cells(line + self.cx, line + self.cx + n);
            }
            (b'X', _) => {
                let at = self.cy * cols + self.cx;
                self.clear_cells(at, at + n.min(cols - self.cx));
            }
            (b'S', _) => self.scroll_up(n),
            (b'T', _) => self.scroll_down(n),
            (b'r', _) => {
                let top = arg(0, 1) - 1;
                let bottom = arg(1, rows as u16) - 1;
                if top < bottom && bottom < rows {
                    self.top = top;
                    self.bottom = bottom;
                    self.cx = 0;
                    self.cy = 0;
                }
            }
            (b's', _) => self.saved = (self.cx, self.cy, self.attr),
            (b'u', _) => {
                (self.cx, self.cy, self.attr) = self.saved;
                self.cx = self.cx.min(cols - 1);
                self.cy = self.cy.min(rows - 1);
            }
            (b'n', _) => {
                if n == 6 {
                    let r = alloc::format!("\x1b[{};{}R", self.cy + 1, self.cx + 1);
                    self.responses.extend_from_slice(r.as_bytes());
                } else if n == 5 {
                    self.responses.extend_from_slice(b"\x1b[0n");
                }
            }
            (b'm', _) => self.sgr(p),
            _ => {}
        }
    }

    fn alternate(&mut self, on: bool) {
        if on && self.alt.is_none() {
            let main = core::mem::replace(&mut self.cells, vec![Cell::default(); self.cols * self.rows]);
            self.alt = Some((main, self.cx, self.cy));
        } else if !on {
            if let Some((main, cx, cy)) = self.alt.take() {
                if main.len() == self.cells.len() {
                    self.cells = main;
                    self.cx = cx;
                    self.cy = cy;
                }
            }
        }
        self.mark_all_dirty();
    }

    fn sgr(&mut self, p: &[u16]) {
        if p.is_empty() {
            self.attr = Attr::default();
            return;
        }
        let mut i = 0;
        while i < p.len() {
            match p[i] {
                0 => self.attr = Attr::default(),
                1 => self.attr.bold = true,
                4 => self.attr.underline = true,
                7 => self.attr.inverse = true,
                22 => self.attr.bold = false,
                24 => self.attr.underline = false,
                27 => self.attr.inverse = false,
                c @ 30..=37 => self.attr.fg = PALETTE[(c - 30) as usize + if self.attr.bold { 8 } else { 0 }],
                39 => self.attr.fg = DEFAULT_FG,
                c @ 40..=47 => self.attr.bg = PALETTE[(c - 40) as usize],
                49 => self.attr.bg = DEFAULT_BG,
                c @ 90..=97 => self.attr.fg = PALETTE[(c - 90) as usize + 8],
                c @ 100..=107 => self.attr.bg = PALETTE[(c - 100) as usize + 8],
                c @ (38 | 48) => {
                    let color = match p.get(i + 1) {
                        Some(5) => {
                            i += 2;
                            p.get(i).map(|&n| color256(n as u8))
                        }
                        Some(2) => {
                            i += 4;
                            match (p.get(i - 2), p.get(i - 1), p.get(i)) {
                                (Some(&r), Some(&g), Some(&b)) => Some(0xFF00_0000 | (r as u32 & 255) << 16 | (g as u32 & 255) << 8 | (b as u32 & 255)),
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    if let Some(color) = color {
                        if c == 38 {
                            self.attr.fg = color;
                        } else {
                            self.attr.bg = color;
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
}

/// xterm's 256-color palette.
pub fn color256(n: u8) -> u32 {
    match n {
        0..=15 => PALETTE[n as usize],
        16..=231 => {
            let n = n - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v as u32 * 40 };
            0xFF00_0000 | level(n / 36) << 16 | level(n / 6 % 6) << 8 | level(n % 6)
        }
        _ => {
            let v = 8 + (n - 232) as u32 * 10;
            0xFF00_0000 | v << 16 | v << 8 | v
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(t: &Term, y: usize) -> String {
        (0..t.cols).map(|x| t.cell(x, y).ch).collect::<String>().trim_end().into()
    }

    #[test]
    fn text_and_scroll() {
        let mut t = Term::new(10, 3);
        t.feed(b"hello\r\nworld\r\nthird\r\nfourth");
        assert_eq!((line(&t, 0), line(&t, 1), line(&t, 2)), ("world".into(), "third".into(), "fourth".into()));
        assert_eq!((t.cx, t.cy), (6, 2));
        t.scroll_view(1);
        assert_eq!(line(&t, 0), "hello");
        t.feed(b"!");
        assert_eq!(t.view, 0);
        // Wrapping.
        let mut t = Term::new(4, 2);
        t.feed(b"abcdef");
        assert_eq!((line(&t, 0), line(&t, 1)), ("abcd".into(), "ef".into()));
        t.feed("\u{2500}é".as_bytes());
        assert_eq!(t.cell(2, 1).ch, '─');
        assert_eq!(t.cell(3, 1).ch, 'é');
    }

    #[test]
    fn escapes() {
        let mut t = Term::new(10, 5);
        t.feed(b"\x1b[2;3Hx\x1b[1;31my\x1b[0m");
        assert_eq!(t.cell(2, 1).ch, 'x');
        assert_eq!(t.cell(3, 1).attr.fg, PALETTE[9]);
        assert_eq!(t.cell(2, 1).attr.fg, DEFAULT_FG);
        t.feed(b"\x1b[2K");
        assert_eq!(line(&t, 1), "");
        t.feed(b"\x1b[6n");
        assert_eq!(t.take_responses(), b"\x1b[2;5R");
        t.feed(b"\x1b]0;my title\x07");
        assert_eq!(t.take_title().as_deref(), Some("my title"));
        t.feed(b"\x1b[?1049h\x1b[Halt\x1b[?1049l");
        assert_eq!(line(&t, 0), "");
        t.feed(b"\x1b[H12345\x1b[1;2H\x1b[2P");
        assert_eq!(line(&t, 0), "145");
        t.feed(b"\x1b[38;5;196mR\x1b[48;2;1;2;3mB");
        assert_eq!(t.cell(1, 0).attr.fg, color256(196));
        assert_eq!(t.cell(2, 0).attr.bg, 0xFF010203);
        let d = t.take_dirty();
        assert!(d.contains(&0));
        assert!(t.take_dirty().is_empty());
    }

    #[test]
    fn regions_and_resize() {
        let mut t = Term::new(5, 4);
        t.feed(b"a\r\nb\r\nc\r\nd");
        t.feed(b"\x1b[2;3r\x1b[3;1H\n");
        assert_eq!((line(&t, 0), line(&t, 1), line(&t, 2), line(&t, 3)), ("a".into(), "c".into(), "".into(), "d".into()));
        t.resize(8, 2);
        assert_eq!((t.cols, t.rows), (8, 2));
        assert!(t.cy < 2);
    }
}
