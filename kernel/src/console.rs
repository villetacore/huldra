//! Kernel console: the VGA text screen mirrored to the COM1 serial port.
//! Text may contain ANSI escapes; both outputs understand them.

use crate::drivers::serial;
use crate::drivers::vga::Vga;
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

/// Writes raw bytes (e.g. from a user process); invalid UTF-8 is replaced.
pub fn write_bytes(bytes: &[u8]) {
    let mut vga = VGA.lock();
    let mut sink = Sink(&mut vga);
    for chunk in bytes.utf8_chunks() {
        let _ = sink.write_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            let _ = sink.write_str("\u{FFFD}");
        }
    }
    vga.update_cursor();
}

pub fn clear() {
    _print(format_args!("\x1b[2J\x1b[H"));
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
