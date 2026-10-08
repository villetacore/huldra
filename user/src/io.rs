//! Standard streams, formatted printing and line reading.

use crate::{sys, Errno, Result};
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::fmt::{self, Write};

pub const STDIN: i32 = 0;
pub const STDOUT: i32 = 1;
pub const STDERR: i32 = 2;

/// Writes all of `data`, retrying short writes and EINTR.
pub fn write_all(fd: i32, mut data: &[u8]) -> Result<()> {
    while !data.is_empty() {
        match sys::write(fd, data) {
            Ok(0) => return Err(Errno::EIO),
            Ok(n) => data = &data[n..],
            Err(Errno::EINTR) => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Stdout is line-buffered when it is a terminal, block-buffered otherwise.
struct StdoutBuffer(UnsafeCell<Vec<u8>>);
unsafe impl Sync for StdoutBuffer {}
static STDOUT_BUF: StdoutBuffer = StdoutBuffer(UnsafeCell::new(Vec::new()));

pub fn flush_stdout() {
    let buf = unsafe { &mut *STDOUT_BUF.0.get() };
    if !buf.is_empty() {
        let _ = write_all(STDOUT, buf);
        buf.clear();
    }
}

struct FdWriter(i32);

impl Write for FdWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.0 == STDOUT {
            let buf = unsafe { &mut *STDOUT_BUF.0.get() };
            buf.extend_from_slice(s.as_bytes());
            if buf.len() >= 4096 || s.contains('\n') {
                flush_stdout();
            }
            Ok(())
        } else {
            write_all(self.0, s.as_bytes()).map_err(|_| fmt::Error)
        }
    }
}

pub fn _print(fd: i32, args: fmt::Arguments) {
    if fd != STDOUT {
        flush_stdout();
    }
    let _ = FdWriter(fd).write_fmt(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::io::_print($crate::io::STDOUT, format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::io::_print($crate::io::STDOUT, format_args!("{}\n", format_args!($($arg)*))));
}

#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => ($crate::io::_print($crate::io::STDERR, format_args!($($arg)*)));
}

#[macro_export]
macro_rules! eprintln {
    () => ($crate::eprint!("\n"));
    ($($arg:tt)*) => ($crate::io::_print($crate::io::STDERR, format_args!("{}\n", format_args!($($arg)*))));
}

/// Buffered reader over a file descriptor.
pub struct Reader {
    fd: i32,
    buf: Vec<u8>,
    pos: usize,
    eof: bool,
}

impl Reader {
    pub fn new(fd: i32) -> Reader {
        Reader {
            fd,
            buf: Vec::new(),
            pos: 0,
            eof: false,
        }
    }

    fn fill(&mut self) -> Result<bool> {
        if self.eof {
            return Ok(false);
        }
        self.buf.drain(..self.pos);
        self.pos = 0;
        let mut chunk = [0u8; 4096];
        loop {
            match sys::read(self.fd, &mut chunk) {
                Ok(0) => {
                    self.eof = true;
                    return Ok(false);
                }
                Ok(n) => {
                    self.buf.extend_from_slice(&chunk[..n]);
                    return Ok(true);
                }
                Err(Errno::EINTR) => return Err(Errno::EINTR),
                Err(e) => return Err(e),
            }
        }
    }

    /// Reads one line (without the newline). `Ok(None)` at end of input.
    pub fn read_line(&mut self) -> Result<Option<String>> {
        loop {
            if let Some(i) = self.buf[self.pos..].iter().position(|&b| b == b'\n') {
                let line = String::from_utf8_lossy(&self.buf[self.pos..self.pos + i]).into_owned();
                self.pos += i + 1;
                return Ok(Some(line));
            }
            if !self.fill()? {
                if self.pos < self.buf.len() {
                    let line = String::from_utf8_lossy(&self.buf[self.pos..]).into_owned();
                    self.pos = self.buf.len();
                    return Ok(Some(line));
                }
                return Ok(None);
            }
        }
    }

    /// Reads everything that is left.
    pub fn read_to_end(&mut self) -> Result<Vec<u8>> {
        while self.fill()? {}
        let rest = self.buf[self.pos..].to_vec();
        self.pos = self.buf.len();
        Ok(rest)
    }

    /// Forgets buffered input (e.g. after an interrupted read).
    pub fn reset(&mut self) {
        self.buf.clear();
        self.pos = 0;
        self.eof = false;
    }
}

/// Reads a whole file, or standard input for `-`.
pub fn read_input(path: &str) -> Result<Vec<u8>> {
    if path == "-" {
        Reader::new(STDIN).read_to_end()
    } else {
        crate::fs::read(path)
    }
}

/// Lines of the named inputs (standard input if none), with the program
/// name used for error messages. Missing files are reported and skipped.
pub fn input_lines(args: &[&str]) -> (Vec<String>, bool) {
    let inputs: Vec<&str> = if args.is_empty() { alloc::vec!["-"] } else { args.to_vec() };
    let mut lines = Vec::new();
    let mut ok = true;
    for p in inputs {
        match read_input(p) {
            Ok(data) => {
                let text = String::from_utf8_lossy(&data);
                let mut v: Vec<String> = text.split('\n').map(String::from).collect();
                if text.ends_with('\n') {
                    v.pop();
                }
                lines.extend(v);
            }
            Err(e) => {
                crate::eprintln!("{}: {}: {}", crate::env::program_name(), p, e);
                ok = false;
            }
        }
    }
    (lines, ok)
}
