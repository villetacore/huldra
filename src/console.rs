//! Kernel console: VGA text screen mirrored to the COM1 serial port.

use crate::drivers::serial;
use crate::drivers::vga::{Color, Vga};
use crate::sync::SpinLock;
use core::fmt::{self, Write};

static VGA: SpinLock<Vga> = SpinLock::new(Vga::new());

struct Sink<'a>(&'a mut Vga);

impl Write for Sink<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                serial::write_byte(b'\r');
            }
            serial::write_byte(b);
        }
        for c in s.chars() {
            self.0.put_char(c);
        }
        Ok(())
    }
}

pub fn _print(args: fmt::Arguments) {
    let mut vga = VGA.lock();
    let _ = Sink(&mut vga).write_fmt(args);
    vga.update_cursor();
}

pub fn clear() {
    VGA.lock().clear();
    serial::write_str("\x1b[2J\x1b[H");
}

pub fn set_color(fg: Color, bg: Color) {
    VGA.lock().set_color(fg, bg);
    if fg == Color::LightGray && bg == Color::Black {
        serial::write_str("\x1b[0m");
    } else {
        // "\x1b[NNm"
        let code = fg.ansi();
        for b in [0x1b, b'[', b'0' + code / 10, b'0' + code % 10, b'm'] {
            serial::write_byte(b);
        }
    }
}

pub fn reset_color() {
    set_color(Color::LightGray, Color::Black);
}

/// Erases the character before the cursor (line editing).
pub fn backspace() {
    let mut vga = VGA.lock();
    vga.backspace();
    vga.update_cursor();
    serial::write_str("\x08 \x08");
}

/// Releases the console lock unconditionally. Only for panic/fatal paths.
pub unsafe fn force_unlock() {
    VGA.force_unlock();
}

macro_rules! print {
    ($($arg:tt)*) => ($crate::console::_print(format_args!($($arg)*)));
}

macro_rules! println {
    () => (print!("\n"));
    ($($arg:tt)*) => ($crate::console::_print(format_args!("{}\n", format_args!($($arg)*))));
}
