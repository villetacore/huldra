//! Kernel log: leveled messages kept in a ring buffer (`dmesg`) and echoed
//! to the console when at or above the console log level.

use crate::sync::SpinLock;
use core::fmt::{self, Write};
use core::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Error => "\x1b[91merror\x1b[0m: ",
            Level::Warn => "\x1b[93mwarning\x1b[0m: ",
            Level::Info => "",
            Level::Debug => "\x1b[90mdebug\x1b[0m: ",
        }
    }
}

const RING_SIZE: usize = 64 * 1024;

struct Ring {
    buf: [u8; RING_SIZE],
    /// Total bytes ever written; the ring holds the last RING_SIZE of them.
    written: usize,
}

impl Write for Ring {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            self.buf[self.written % RING_SIZE] = b;
            self.written += 1;
        }
        Ok(())
    }
}

static RING: SpinLock<Ring> = SpinLock::new(Ring { buf: [0; RING_SIZE], written: 0 });
static CONSOLE_LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

pub fn set_console_level(level: Level) {
    CONSOLE_LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn log(level: Level, args: fmt::Arguments) {
    let ms = crate::time::uptime_ms();
    {
        let mut ring = RING.lock();
        let _ = write!(ring, "[{:5}.{:03}] {}{}\n", ms / 1000, ms % 1000, level.tag(), args);
    }
    if level as u8 <= CONSOLE_LEVEL.load(Ordering::Relaxed) {
        crate::console::_print(format_args!("[{:5}.{:03}] {}{}\n", ms / 1000, ms % 1000, level.tag(), args));
    }
}

/// Copies the log contents (oldest first) into a new buffer.
pub fn snapshot() -> alloc::vec::Vec<u8> {
    let ring = RING.lock();
    let len = ring.written.min(RING_SIZE);
    let start = ring.written - len;
    (start..ring.written).map(|i| ring.buf[i % RING_SIZE]).collect()
}

macro_rules! kerror {
    ($($arg:tt)*) => ($crate::klog::log($crate::klog::Level::Error, format_args!($($arg)*)));
}

macro_rules! kwarn {
    ($($arg:tt)*) => ($crate::klog::log($crate::klog::Level::Warn, format_args!($($arg)*)));
}

macro_rules! kinfo {
    ($($arg:tt)*) => ($crate::klog::log($crate::klog::Level::Info, format_args!($($arg)*)));
}

#[allow(unused_macros)]
macro_rules! kdebug {
    ($($arg:tt)*) => ($crate::klog::log($crate::klog::Level::Debug, format_args!($($arg)*)));
}
