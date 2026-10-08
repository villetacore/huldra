//! VGA text mode (80x25) driver.

use crate::arch::port::outb;

const WIDTH: usize = 80;
const HEIGHT: usize = 25;
const BUFFER: *mut u16 = 0xB8000 as *mut u16;

#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

impl Color {
    /// Matching ANSI SGR foreground code, for mirroring colors to serial.
    pub fn ansi(self) -> u8 {
        const MAP: [u8; 16] = [30, 34, 32, 36, 31, 35, 33, 37, 90, 94, 92, 96, 91, 95, 93, 97];
        MAP[self as usize]
    }
}

pub struct Vga {
    col: usize,
    row: usize,
    attr: u8,
}

impl Vga {
    pub const fn new() -> Self {
        Vga { col: 0, row: 0, attr: 0x07 }
    }

    fn cell(&self, c: u8) -> u16 {
        (self.attr as u16) << 8 | c as u16
    }

    fn write_at(&self, index: usize, value: u16) {
        unsafe { BUFFER.add(index).write_volatile(value) }
    }

    pub fn set_color(&mut self, fg: Color, bg: Color) {
        self.attr = (bg as u8) << 4 | fg as u8;
    }

    pub fn clear(&mut self) {
        let blank = self.cell(b' ');
        for i in 0..WIDTH * HEIGHT {
            self.write_at(i, blank);
        }
        self.col = 0;
        self.row = 0;
        self.update_cursor();
    }

    pub fn put_char(&mut self, c: char) {
        if c.is_ascii() {
            self.put_byte(c as u8);
        } else {
            self.put_byte(0xFE); // ■ for anything outside code page 437 ASCII
        }
    }

    pub fn put_byte(&mut self, b: u8) {
        match b {
            b'\n' => self.newline(),
            b'\r' => self.col = 0,
            b'\t' => {
                let next = (self.col + 8) & !7;
                while self.col < next.min(WIDTH) {
                    self.put_byte(b' ');
                }
            }
            0x08 => self.backspace(),
            _ => {
                self.write_at(self.row * WIDTH + self.col, self.cell(b));
                self.col += 1;
                if self.col == WIDTH {
                    self.newline();
                }
            }
        }
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = WIDTH - 1;
        }
        self.write_at(self.row * WIDTH + self.col, self.cell(b' '));
    }

    fn newline(&mut self) {
        self.col = 0;
        if self.row + 1 < HEIGHT {
            self.row += 1;
            return;
        }
        for i in 0..WIDTH * (HEIGHT - 1) {
            let v = unsafe { BUFFER.add(i + WIDTH).read_volatile() };
            self.write_at(i, v);
        }
        let blank = self.cell(b' ');
        for i in WIDTH * (HEIGHT - 1)..WIDTH * HEIGHT {
            self.write_at(i, blank);
        }
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
