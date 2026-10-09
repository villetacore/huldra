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

/// The start of a UTF-8 sequence that a write cut in the middle (buffered
/// output splits text at arbitrary bytes); completed by the next write.
static PARTIAL: crate::sync::SpinLock<([u8; 4], usize)> = crate::sync::SpinLock::new(([0; 4], 0));

/// Length of the UTF-8 sequence a lead byte starts (0 if not a lead byte).
fn utf8_len(lead: u8) -> usize {
    match lead {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 0,
    }
}

/// Writes raw bytes (e.g. from a user process); invalid UTF-8 is replaced.
pub fn write_bytes(bytes: &[u8]) {
    let mut vga = VGA.lock();
    let mut partial = PARTIAL.lock();
    let joined: alloc::vec::Vec<u8>;
    let data = if partial.1 > 0 {
        joined = [&partial.0[..partial.1], bytes].concat();
        partial.1 = 0;
        &joined[..]
    } else {
        bytes
    };
    let mut sink = Sink(&mut vga);
    let mut chunks = data.utf8_chunks().peekable();
    while let Some(chunk) = chunks.next() {
        let _ = sink.write_str(chunk.valid());
        let bad = chunk.invalid();
        if bad.is_empty() {
            continue;
        }
        let at_end = chunks.peek().is_none();
        if at_end && bad.len() < utf8_len(bad[0]) {
            partial.0[..bad.len()].copy_from_slice(bad);
            partial.1 = bad.len();
        } else {
            let _ = sink.write_str("\u{FFFD}");
        }
    }
    drop(partial);
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
