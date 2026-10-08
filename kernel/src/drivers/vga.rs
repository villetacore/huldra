//! VGA text mode (80x25) driver with a small ANSI escape interpreter
//! (SGR colors, `J` erase display, `K` erase line, `H` cursor position).

use crate::arch::port::outb;
use crate::mm::phys_to_virt;

const WIDTH: usize = 80;
const HEIGHT: usize = 25;
const BUFFER_PHYS: u64 = 0xB8000;
const DEFAULT_ATTR: u8 = 0x07;

/// ANSI color index (0..8) to VGA color index.
const ANSI_TO_VGA: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

enum Escape {
    None,
    Esc,
    Csi { params: [u16; 4], count: usize },
}

pub struct Vga {
    col: usize,
    row: usize,
    attr: u8,
    escape: Escape,
}

fn buffer() -> *mut u16 {
    phys_to_virt(BUFFER_PHYS) as *mut u16
}

impl Vga {
    pub const fn new() -> Self {
        Vga {
            col: 0,
            row: 0,
            attr: DEFAULT_ATTR,
            escape: Escape::None,
        }
    }

    fn cell(&self, c: u8) -> u16 {
        (self.attr as u16) << 8 | c as u16
    }

    fn write_at(&self, index: usize, value: u16) {
        unsafe { buffer().add(index).write_volatile(value) }
    }

    fn clear_range(&self, from: usize, to: usize) {
        let blank = self.cell(b' ');
        for i in from..to {
            self.write_at(i, blank);
        }
    }

    pub fn put_char(&mut self, c: char) {
        match core::mem::replace(&mut self.escape, Escape::None) {
            Escape::None if c == '\x1b' => self.escape = Escape::Esc,
            Escape::None if c.is_ascii() => self.put_byte(c as u8),
            Escape::None => self.put_byte(0xFE), // ■ for anything outside ASCII
            Escape::Esc if c == '[' => {
                self.escape = Escape::Csi {
                    params: [0; 4],
                    count: 0,
                }
            }
            Escape::Esc => {}
            Escape::Csi {
                mut params,
                mut count,
            } => match c {
                '0'..='9' => {
                    let i = count.min(3);
                    params[i] = params[i]
                        .saturating_mul(10)
                        .saturating_add(c as u16 - '0' as u16);
                    self.escape = Escape::Csi { params, count };
                }
                ';' => {
                    count += 1;
                    self.escape = Escape::Csi { params, count };
                }
                '?' => self.escape = Escape::Csi { params, count },
                _ => self.csi(c, &params[..=count.min(3)]),
            },
        }
    }

    fn csi(&mut self, command: char, params: &[u16]) {
        match command {
            'm' => {
                for &p in params {
                    self.sgr(p);
                }
            }
            'J' => match params[0] {
                2 | 3 => {
                    self.clear_range(0, WIDTH * HEIGHT);
                    self.row = 0;
                    self.col = 0;
                }
                _ => self.clear_range(self.row * WIDTH + self.col, WIDTH * HEIGHT),
            },
            'K' => self.clear_range(self.row * WIDTH + self.col, (self.row + 1) * WIDTH),
            'H' | 'f' => {
                self.row = (params[0].max(1) as usize - 1).min(HEIGHT - 1);
                self.col = (params.get(1).copied().unwrap_or(1).max(1) as usize - 1).min(WIDTH - 1);
            }
            _ => {}
        }
    }

    fn sgr(&mut self, p: u16) {
        let (fg, bg) = (self.attr & 0x0F, self.attr >> 4);
        self.attr = match p {
            0 => DEFAULT_ATTR,
            1 => self.attr | 0x08,
            30..=37 => (bg << 4) | ANSI_TO_VGA[(p - 30) as usize] | (fg & 0x08),
            39 => (bg << 4) | 0x07,
            40..=47 => (ANSI_TO_VGA[(p - 40) as usize] << 4) | fg,
            49 => fg,
            90..=97 => (bg << 4) | ANSI_TO_VGA[(p - 90) as usize] | 0x08,
            100..=107 => ((ANSI_TO_VGA[(p - 100) as usize] | 0x08) << 4) | fg,
            _ => self.attr,
        };
    }

    pub fn put_byte(&mut self, b: u8) {
        match b {
            b'\n' => self.newline(),
            b'\r' => self.col = 0,
            b'\t' => {
                let next = ((self.col + 8) & !7).min(WIDTH);
                while self.col < next {
                    self.put_byte(b' ');
                }
            }
            0x08 => {
                if self.col > 0 {
                    self.col -= 1;
                }
            }
            0x07 => {} // bell
            _ => {
                self.write_at(self.row * WIDTH + self.col, self.cell(b));
                self.col += 1;
                if self.col == WIDTH {
                    self.newline();
                }
            }
        }
    }

    fn newline(&mut self) {
        self.col = 0;
        if self.row + 1 < HEIGHT {
            self.row += 1;
            return;
        }
        for i in 0..WIDTH * (HEIGHT - 1) {
            let v = unsafe { buffer().add(i + WIDTH).read_volatile() };
            self.write_at(i, v);
        }
        self.clear_range(WIDTH * (HEIGHT - 1), WIDTH * HEIGHT);
    }

    pub fn update_cursor(&self) {
        let pos = (self.row * WIDTH + self.col) as u16;
        unsafe {
            outb(0x3D4, 0x0F);
            outb(0x3D5, pos as u8);
            outb(0x3D4, 0x0E);
            outb(0x3D5, (pos >> 8) as u8);
        }
    }
}
