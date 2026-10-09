//! VGA text mode (80x25) console with a VT100/xterm-style escape
//! interpreter: cursor movement and positioning, erase, insert/delete lines,
//! scroll, SGR colors and reverse video, cursor visibility and save/restore.
//! Unicode box-drawing and block characters are shown via code page 437.

use crate::arch::port::outb;
use crate::mm::phys_to_virt;

pub const WIDTH: usize = 80;
pub const HEIGHT: usize = 25;
const BUFFER_PHYS: u64 = 0xB8000;
const DEFAULT_ATTR: u8 = 0x07;

/// ANSI color index (0..8) to VGA color index.
const ANSI_TO_VGA: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

/// The 16 text colors (in VGA order: black, blue, green, cyan, red,
/// magenta, brown, grey, then the bright ones), retuned like an old
/// phosphor monitor: a dark green-grey screen, soft green text, and the
/// other colors tinted to match. The same palette as the graphical
/// terminal (huldra-gfx `term::PALETTE`).
const PALETTE: [u32; 16] = [
    0x0C1712, 0x4F9C94, 0x5FD27E, 0x62C8AE, 0xD9694C, 0xB08AA6, 0xE0B04E, 0x96E6A8, //
    0x3E6250, 0x7FCFC6, 0x9AF7AE, 0x9EF2D6, 0xF28C6C, 0xD4AECB, 0xFFD27E, 0xDCFFE2,
];

/// DAC entries the attribute controller uses for the 16 text colors.
const DAC_INDEX: [u8; 16] = [0, 1, 2, 3, 4, 5, 0x14, 7, 0x38, 0x39, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F];

/// Loads the palette and switches to a block cursor. Call at boot, before
/// the frame buffer driver saves the text mode state.
pub fn init() {
    unsafe {
        for (i, &rgb) in PALETTE.iter().enumerate() {
            outb(0x3C8, DAC_INDEX[i]);
            outb(0x3C9, (rgb >> 18) as u8 & 0x3F);
            outb(0x3C9, (rgb >> 10) as u8 & 0x3F);
            outb(0x3C9, (rgb >> 2) as u8 & 0x3F);
        }
        // Cursor scanlines 0..=15: a full block, like a terminal of the 70s.
        outb(0x3D4, 0x0A);
        outb(0x3D5, 0x00);
        outb(0x3D4, 0x0B);
        outb(0x3D5, 0x0F);
    }
}

enum Escape {
    None,
    Esc,
    Csi { params: [u16; 8], count: usize, private: bool },
}

pub struct Vga {
    col: usize,
    row: usize,
    attr: u8,
    reverse: bool,
    /// Set after writing the last column; the next character wraps first.
    wrap_pending: bool,
    cursor_visible: bool,
    saved: (usize, usize),
    escape: Escape,
}

fn buffer() -> *mut u16 {
    phys_to_virt(BUFFER_PHYS) as *mut u16
}

/// Maps a Unicode character to code page 437 (the VGA font).
fn cp437(c: char) -> u8 {
    if c.is_ascii() {
        return c as u8;
    }
    match c {
        '─' | '━' => 0xC4,
        '│' | '┃' => 0xB3,
        '┌' | '┏' => 0xDA,
        '┐' | '┓' => 0xBF,
        '└' | '┗' => 0xC0,
        '┘' | '┛' => 0xD9,
        '├' => 0xC3,
        '┤' => 0xB4,
        '┬' => 0xC2,
        '┴' => 0xC1,
        '┼' => 0xC5,
        '═' => 0xCD,
        '║' => 0xBA,
        '╔' => 0xC9,
        '╗' => 0xBB,
        '╚' => 0xC8,
        '╝' => 0xBC,
        '╠' => 0xCC,
        '╣' => 0xB9,
        '╦' => 0xCB,
        '╩' => 0xCA,
        '╬' => 0xCE,
        '░' => 0xB0,
        '▒' => 0xB1,
        '▓' => 0xB2,
        '█' => 0xDB,
        '▄' => 0xDC,
        '▌' => 0xDD,
        '▐' => 0xDE,
        '▀' => 0xDF,
        '■' => 0xFE,
        '·' => 0xFA,
        '•' => 0x07,
        '°' => 0xF8,
        '±' => 0xF1,
        '×' => b'x',
        '←' => 0x1B,
        '→' => 0x1A,
        '↑' => 0x18,
        '↓' => 0x19,
        '▲' => 0x1E,
        '▼' => 0x1F,
        '►' | '▶' => 0x10,
        '◄' | '◀' => 0x11,
        '✓' | '√' => 0xFB,
        '…' => b'~',
        'é' => 0x82,
        'ü' => 0x81,
        'ö' => 0x94,
        'ä' => 0x84,
        'ß' => 0xE1,
        'µ' => 0xE6,
        _ => 0xFE, // ■ for anything the font does not have
    }
}

impl Vga {
    pub const fn new() -> Self {
        Vga {
            col: 0,
            row: 0,
            attr: DEFAULT_ATTR,
            reverse: false,
            wrap_pending: false,
            cursor_visible: true,
            saved: (0, 0),
            escape: Escape::None,
        }
    }

    fn effective_attr(&self) -> u8 {
        if self.reverse {
            (self.attr << 4) | (self.attr >> 4)
        } else {
            self.attr
        }
    }

    fn cell(&self, c: u8) -> u16 {
        (self.effective_attr() as u16) << 8 | c as u16
    }

    fn write_at(&self, index: usize, value: u16) {
        unsafe { buffer().add(index).write_volatile(value) }
    }

    fn read_at(&self, index: usize) -> u16 {
        unsafe { buffer().add(index).read_volatile() }
    }

    fn clear_range(&self, from: usize, to: usize) {
        let blank = self.cell(b' ');
        for i in from..to.min(WIDTH * HEIGHT) {
            self.write_at(i, blank);
        }
    }

    /// Scrolls rows `top..=bottom` up (n > 0) or down (n < 0).
    fn scroll(&self, top: usize, bottom: usize, n: isize) {
        let rows = bottom + 1 - top;
        let k = n.unsigned_abs().min(rows);
        if n > 0 {
            for r in top..=bottom - k {
                for c in 0..WIDTH {
                    self.write_at(r * WIDTH + c, self.read_at((r + k) * WIDTH + c));
                }
            }
            self.clear_range((bottom + 1 - k) * WIDTH, (bottom + 1) * WIDTH);
        } else {
            for r in (top + k..=bottom).rev() {
                for c in 0..WIDTH {
                    self.write_at(r * WIDTH + c, self.read_at((r - k) * WIDTH + c));
                }
            }
            self.clear_range(top * WIDTH, (top + k) * WIDTH);
        }
    }

    pub fn put_char(&mut self, c: char) {
        match core::mem::replace(&mut self.escape, Escape::None) {
            Escape::None if c == '\x1b' => self.escape = Escape::Esc,
            Escape::None => self.put_byte(cp437(c)),
            Escape::Esc => match c {
                '[' => self.escape = Escape::Csi { params: [0; 8], count: 0, private: false },
                '7' => self.saved = (self.row, self.col),
                '8' => (self.row, self.col) = self.saved,
                'c' => {
                    *self = Vga::new();
                    self.clear_range(0, WIDTH * HEIGHT);
                }
                _ => {}
            },
            Escape::Csi { mut params, mut count, mut private } => match c {
                '0'..='9' => {
                    let i = count.min(7);
                    params[i] = params[i].saturating_mul(10).saturating_add(c as u16 - '0' as u16);
                    self.escape = Escape::Csi { params, count, private };
                }
                ';' => {
                    count += 1;
                    self.escape = Escape::Csi { params, count, private };
                }
                '?' => {
                    private = true;
                    self.escape = Escape::Csi { params, count, private };
                }
                _ => self.csi(c, &params[..=count.min(7)], private),
            },
        }
    }

    fn csi(&mut self, command: char, params: &[u16], private: bool) {
        let n = params[0].max(1) as usize;
        self.wrap_pending = false;
        match command {
            'm' => {
                for &p in params {
                    self.sgr(p);
                }
            }
            'A' => self.row = self.row.saturating_sub(n),
            'B' => self.row = (self.row + n).min(HEIGHT - 1),
            'C' => self.col = (self.col + n).min(WIDTH - 1),
            'D' => self.col = self.col.saturating_sub(n),
            'E' => {
                self.row = (self.row + n).min(HEIGHT - 1);
                self.col = 0;
            }
            'F' => {
                self.row = self.row.saturating_sub(n);
                self.col = 0;
            }
            'G' => self.col = (n - 1).min(WIDTH - 1),
            'd' => self.row = (n - 1).min(HEIGHT - 1),
            'H' | 'f' => {
                self.row = (params[0].max(1) as usize - 1).min(HEIGHT - 1);
                self.col = (params.get(1).copied().unwrap_or(1).max(1) as usize - 1).min(WIDTH - 1);
            }
            'J' => match params[0] {
                1 => self.clear_range(0, self.row * WIDTH + self.col + 1),
                2 | 3 => self.clear_range(0, WIDTH * HEIGHT),
                _ => self.clear_range(self.row * WIDTH + self.col, WIDTH * HEIGHT),
            },
            'K' => {
                let line = self.row * WIDTH;
                match params[0] {
                    1 => self.clear_range(line, line + self.col + 1),
                    2 => self.clear_range(line, line + WIDTH),
                    _ => self.clear_range(line + self.col, line + WIDTH),
                }
            }
            'L' => self.scroll(self.row, HEIGHT - 1, -(n as isize)),
            'M' => self.scroll(self.row, HEIGHT - 1, n as isize),
            'S' => self.scroll(0, HEIGHT - 1, n as isize),
            'T' => self.scroll(0, HEIGHT - 1, -(n as isize)),
            'P' => {
                // Delete characters, shifting the rest of the line left.
                let line = self.row * WIDTH;
                for c in self.col..WIDTH {
                    let v = if c + n < WIDTH { self.read_at(line + c + n) } else { self.cell(b' ') };
                    self.write_at(line + c, v);
                }
            }
            '@' => {
                let line = self.row * WIDTH;
                for c in (self.col..WIDTH).rev() {
                    let v = if c >= self.col + n { self.read_at(line + c - n) } else { self.cell(b' ') };
                    self.write_at(line + c, v);
                }
            }
            'X' => self.clear_range(self.row * WIDTH + self.col, self.row * WIDTH + (self.col + n).min(WIDTH)),
            's' => self.saved = (self.row, self.col),
            'u' => (self.row, self.col) = self.saved,
            'h' | 'l' if private && params[0] == 25 => self.cursor_visible = command == 'h',
            _ => {}
        }
    }

    fn sgr(&mut self, p: u16) {
        let (fg, bg) = (self.attr & 0x0F, self.attr >> 4);
        match p {
            0 => {
                self.attr = DEFAULT_ATTR;
                self.reverse = false;
            }
            1 => self.attr |= 0x08,
            22 => self.attr &= !0x08,
            7 => self.reverse = true,
            27 => self.reverse = false,
            30..=37 => self.attr = (bg << 4) | ANSI_TO_VGA[(p - 30) as usize] | (fg & 0x08),
            39 => self.attr = (bg << 4) | 0x07,
            40..=47 => self.attr = (ANSI_TO_VGA[(p - 40) as usize] << 4) | fg,
            49 => self.attr = fg,
            90..=97 => self.attr = (bg << 4) | ANSI_TO_VGA[(p - 90) as usize] | 0x08,
            100..=107 => self.attr = ((ANSI_TO_VGA[(p - 100) as usize] | 0x08) << 4) | fg,
            _ => {}
        }
    }

    pub fn put_byte(&mut self, b: u8) {
        match b {
            b'\n' => {
                self.wrap_pending = false;
                self.newline();
            }
            b'\r' => {
                self.wrap_pending = false;
                self.col = 0;
            }
            b'\t' => {
                let next = ((self.col + 8) & !7).min(WIDTH - 1);
                while self.col < next {
                    self.put_byte(b' ');
                }
            }
            0x08 => {
                self.wrap_pending = false;
                self.col = self.col.saturating_sub(1);
            }
            0x07 => {} // bell
            _ => {
                if self.wrap_pending {
                    self.wrap_pending = false;
                    self.col = 0;
                    self.newline();
                }
                self.write_at(self.row * WIDTH + self.col, self.cell(b));
                if self.col + 1 == WIDTH {
                    self.wrap_pending = true;
                } else {
                    self.col += 1;
                }
            }
        }
    }

    fn newline(&mut self) {
        self.col = 0;
        if self.row + 1 < HEIGHT {
            self.row += 1;
        } else {
            self.scroll(0, HEIGHT - 1, 1);
        }
    }

    pub fn update_cursor(&self) {
        let pos = if self.cursor_visible { (self.row * WIDTH + self.col) as u16 } else { (WIDTH * HEIGHT) as u16 };
        unsafe {
            outb(0x3D4, 0x0F);
            outb(0x3D5, pos as u8);
            outb(0x3D4, 0x0E);
            outb(0x3D5, (pos >> 8) as u8);
        }
    }
}
