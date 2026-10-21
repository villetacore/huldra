//! Input and output: standard streams, `Read`, `Write`, `BufRead`.

use crate::fmt;
use crate::option::Option::{self, None, Some};
use crate::string::String;
use crate::vec::Vec;

pub type Result<T> = crate::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    NotFound,
    PermissionDenied,
    ConnectionRefused,
    AlreadyExists,
    WouldBlock,
    InvalidInput,
    InvalidData,
    TimedOut,
    WriteZero,
    Interrupted,
    Unsupported,
    UnexpectedEof,
    OutOfMemory,
    Other,
}

impl ErrorKind {
    fn as_str(&self) -> &'static str {
        match *self {
            ErrorKind::NotFound => "entity not found",
            ErrorKind::PermissionDenied => "permission denied",
            ErrorKind::ConnectionRefused => "connection refused",
            ErrorKind::AlreadyExists => "entity already exists",
            ErrorKind::WouldBlock => "operation would block",
            ErrorKind::InvalidInput => "invalid input parameter",
            ErrorKind::InvalidData => "invalid data",
            ErrorKind::TimedOut => "timed out",
            ErrorKind::WriteZero => "write zero",
            ErrorKind::Interrupted => "operation interrupted",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::UnexpectedEof => "unexpected end of file",
            ErrorKind::OutOfMemory => "out of memory",
            ErrorKind::Other => "other error",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.pad(self.as_str())
    }
}

pub struct Error {
    kind: ErrorKind,
    code: i32,
    msg: String,
}

impl Error {
    pub fn new<E: fmt::Display>(kind: ErrorKind, error: E) -> Error {
        Error {
            kind,
            code: 0,
            msg: error.to_string(),
        }
    }
    pub fn other<E: fmt::Display>(error: E) -> Error {
        Error::new(ErrorKind::Other, error)
    }
    pub fn from_raw_os_error(code: i32) -> Error {
        let kind = match code {
            2 => ErrorKind::NotFound,
            13 | 1 => ErrorKind::PermissionDenied,
            17 => ErrorKind::AlreadyExists,
            22 => ErrorKind::InvalidInput,
            4 => ErrorKind::Interrupted,
            _ => ErrorKind::Other,
        };
        Error {
            kind,
            code,
            msg: String::new(),
        }
    }
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
    pub fn raw_os_error(&self) -> Option<i32> {
        if self.code != 0 {
            Some(self.code)
        } else {
            None
        }
    }
    fn os_message(&self) -> &'static str {
        match self.code {
            1 => "Operation not permitted",
            2 => "No such file or directory",
            5 => "Input/output error",
            9 => "Bad file descriptor",
            12 => "Cannot allocate memory",
            13 => "Permission denied",
            17 => "File exists",
            20 => "Not a directory",
            21 => "Is a directory",
            22 => "Invalid argument",
            28 => "No space left on device",
            _ => "Unknown error",
        }
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error {
            kind,
            code: 0,
            msg: String::new(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.code != 0 {
            write!(f, "{} (os error {})", self.os_message(), self.code)
        } else if !self.msg.is_empty() {
            f.write_str(&self.msg)
        } else {
            f.write_str(self.kind.as_str())
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.code != 0 {
            f.debug_struct("Os")
                .field("code", &self.code)
                .field("kind", &self.kind)
                .field("message", &self.os_message())
                .finish()
        } else if !self.msg.is_empty() {
            f.debug_struct("Custom").field("kind", &self.kind).field("error", &self.msg).finish()
        } else {
            f.debug_tuple("Kind").field(&self.kind).finish()
        }
    }
}

impl crate::error::Error for Error {}

pub trait Read {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
        let mut chunk = [0u8; 4096];
        let mut total = 0;
        loop {
            let n = self.read(&mut chunk)?;
            if n == 0 {
                return Ok(total);
            }
            buf.extend_from_slice(&chunk[..n]);
            total += n;
        }
    }

    fn read_to_string(&mut self, buf: &mut String) -> Result<usize> {
        let mut v = Vec::new();
        let n = self.read_to_end(&mut v)?;
        match crate::str::from_utf8(&v) {
            Ok(s) => {
                buf.push_str(s);
                Ok(n)
            }
            Err(_) => Err(Error::new(ErrorKind::InvalidData, "stream did not contain valid UTF-8")),
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        let mut i = 0;
        while i < buf.len() {
            let n = self.read(&mut buf[i..])?;
            if n == 0 {
                return Err(Error::new(ErrorKind::UnexpectedEof, "failed to fill whole buffer"));
            }
            i += n;
        }
        Ok(())
    }

    fn bytes(self) -> Bytes<Self>
    where
        Self: Sized,
    {
        Bytes { inner: self }
    }

    fn by_ref(&mut self) -> &mut Self
    where
        Self: Sized,
    {
        self
    }
}

pub struct Bytes<R> {
    inner: R,
}

impl<R: Read> Iterator for Bytes<R> {
    type Item = Result<u8>;
    fn next(&mut self) -> Option<Result<u8>> {
        let mut b = [0u8; 1];
        match self.inner.read(&mut b) {
            Ok(0) => None,
            Ok(_) => Some(Ok(b[0])),
            Err(e) => Some(Err(e)),
        }
    }
}

pub trait Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize>;
    fn flush(&mut self) -> Result<()>;

    fn write_all(&mut self, mut buf: &[u8]) -> Result<()> {
        while !buf.is_empty() {
            let n = self.write(buf)?;
            if n == 0 {
                return Err(Error::new(ErrorKind::WriteZero, "failed to write whole buffer"));
            }
            buf = &buf[n..];
        }
        Ok(())
    }

    fn write_fmt(&mut self, args: fmt::Arguments) -> Result<()> {
        self.write_all(args.as_str().as_bytes())
    }

    fn by_ref(&mut self) -> &mut Self
    where
        Self: Sized,
    {
        self
    }
}

impl<W: Write + ?Sized> Write for &mut W {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        (**self).write(buf)
    }
    fn flush(&mut self) -> Result<()> {
        (**self).flush()
    }
}

impl<R: Read + ?Sized> Read for &mut R {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        (**self).read(buf)
    }
}

pub trait BufRead: Read {
    fn fill_buf(&mut self) -> Result<&[u8]>;
    fn consume(&mut self, amt: usize);

    fn read_until(&mut self, delim: u8, buf: &mut Vec<u8>) -> Result<usize> {
        let mut total = 0;
        loop {
            let (done, used) = {
                let avail = self.fill_buf()?;
                if avail.is_empty() {
                    return Ok(total);
                }
                let mut i = 0;
                let mut found = false;
                while i < avail.len() {
                    if avail[i] == delim {
                        found = true;
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                buf.extend_from_slice(&avail[..i]);
                (found, i)
            };
            self.consume(used);
            total += used;
            if done {
                return Ok(total);
            }
        }
    }

    fn read_line(&mut self, buf: &mut String) -> Result<usize> {
        let mut v = Vec::new();
        let n = self.read_until(b'\n', &mut v)?;
        match crate::str::from_utf8(&v) {
            Ok(s) => {
                buf.push_str(s);
                Ok(n)
            }
            Err(_) => Err(Error::new(ErrorKind::InvalidData, "stream did not contain valid UTF-8")),
        }
    }

    fn lines(self) -> Lines<Self>
    where
        Self: Sized,
    {
        Lines { buf: self }
    }

    fn split(self, byte: u8) -> Split<Self>
    where
        Self: Sized,
    {
        Split { buf: self, delim: byte }
    }
}

pub struct Lines<B> {
    buf: B,
}

impl<B: BufRead> Iterator for Lines<B> {
    type Item = Result<String>;
    fn next(&mut self) -> Option<Result<String>> {
        let mut s = String::new();
        match self.buf.read_line(&mut s) {
            Ok(0) => None,
            Ok(_) => {
                if s.ends_with('\n') {
                    s.pop();
                    if s.ends_with('\r') {
                        s.pop();
                    }
                }
                Some(Ok(s))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

pub struct Split<B> {
    buf: B,
    delim: u8,
}

impl<B: BufRead> Iterator for Split<B> {
    type Item = Result<Vec<u8>>;
    fn next(&mut self) -> Option<Result<Vec<u8>>> {
        let mut v = Vec::new();
        match self.buf.read_until(self.delim, &mut v) {
            Ok(0) => None,
            Ok(_) => {
                if v.last() == Some(&self.delim) {
                    v.pop();
                }
                Some(Ok(v))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

// ---- standard streams ----

/// The process-wide stdin buffer.
struct StdinBuf {
    buf: Vec<u8>,
    pos: usize,
    len: usize,
}

static mut STDIN_BUF: StdinBuf = StdinBuf {
    buf: Vec::new(),
    pos: 0,
    len: 0,
};

fn stdin_fill() -> Result<&'static [u8]> {
    unsafe {
        if STDIN_BUF.pos >= STDIN_BUF.len {
            if STDIN_BUF.buf.len() < 8192 {
                STDIN_BUF.buf.resize(8192, 0);
            }
            let n = crate::sys::read(0, STDIN_BUF.buf.as_mut_ptr(), 8192);
            if n < 0 {
                return Err(Error::from_raw_os_error(5));
            }
            STDIN_BUF.pos = 0;
            STDIN_BUF.len = n as usize;
        }
        Ok(crate::slice::from_raw_parts(STDIN_BUF.buf.as_ptr().add(STDIN_BUF.pos), STDIN_BUF.len - STDIN_BUF.pos))
    }
}

fn stdin_consume(n: usize) {
    unsafe {
        STDIN_BUF.pos += n;
    }
}

pub struct Stdin;

pub struct StdinLock<'a> {
    marker: crate::marker::PhantomData<&'a ()>,
}

pub fn stdin() -> Stdin {
    Stdin
}

impl Stdin {
    pub fn lock(&self) -> StdinLock<'static> {
        StdinLock {
            marker: crate::marker::PhantomData,
        }
    }
    pub fn read_line(&self, buf: &mut String) -> Result<usize> {
        self.lock().read_line(buf)
    }
    pub fn lines(self) -> Lines<StdinLock<'static>> {
        self.lock().lines()
    }
    pub fn read_to_string(&mut self, buf: &mut String) -> Result<usize> {
        self.lock().read_to_string(buf)
    }
}

impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.lock().read(buf)
    }
}

impl<'a> Read for StdinLock<'a> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let avail = stdin_fill()?;
        let n = if avail.len() < buf.len() { avail.len() } else { buf.len() };
        buf[..n].copy_from_slice(&avail[..n]);
        stdin_consume(n);
        Ok(n)
    }
}

impl<'a> BufRead for StdinLock<'a> {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        stdin_fill()
    }
    fn consume(&mut self, amt: usize) {
        stdin_consume(amt);
    }
}

pub struct Stdout;

pub struct StdoutLock<'a> {
    marker: crate::marker::PhantomData<&'a ()>,
}

pub fn stdout() -> Stdout {
    Stdout
}

impl Stdout {
    pub fn lock(&self) -> StdoutLock<'static> {
        StdoutLock {
            marker: crate::marker::PhantomData,
        }
    }
}

impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        unsafe { crate::sys::hrc_write_stdout(buf.as_ptr(), buf.len()) };
        Ok(buf.len())
    }
    fn flush(&mut self) -> Result<()> {
        unsafe { crate::sys::hrc_flush_stdout() };
        Ok(())
    }
}

impl<'a> Write for StdoutLock<'a> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        unsafe { crate::sys::hrc_write_stdout(buf.as_ptr(), buf.len()) };
        Ok(buf.len())
    }
    fn flush(&mut self) -> Result<()> {
        unsafe { crate::sys::hrc_flush_stdout() };
        Ok(())
    }
}

pub struct Stderr;

pub fn stderr() -> Stderr {
    Stderr
}

impl Stderr {
    pub fn lock(&self) -> Stderr {
        Stderr
    }
}

impl Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        unsafe { crate::sys::hrc_write_stderr(buf.as_ptr(), buf.len()) };
        Ok(buf.len())
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

pub fn _print(s: &str) {
    unsafe { crate::sys::hrc_write_stdout(s.as_ptr(), s.len()) }
}

pub fn _eprint(s: &str) {
    unsafe { crate::sys::hrc_write_stderr(s.as_ptr(), s.len()) }
}

pub fn read_to_string<R: Read>(mut r: R) -> Result<String> {
    let mut s = String::new();
    r.read_to_string(&mut s)?;
    Ok(s)
}

/// A buffered reader (any `Read` becomes `BufRead`).
pub struct BufReader<R> {
    inner: R,
    buf: Vec<u8>,
    pos: usize,
    len: usize,
}

impl<R: Read> BufReader<R> {
    pub fn new(inner: R) -> BufReader<R> {
        BufReader {
            inner,
            buf: crate::vec::from_elem(0u8, 8192),
            pos: 0,
            len: 0,
        }
    }
    pub fn with_capacity(cap: usize, inner: R) -> BufReader<R> {
        BufReader {
            inner,
            buf: crate::vec::from_elem(0u8, if cap == 0 { 1 } else { cap }),
            pos: 0,
            len: 0,
        }
    }
    pub fn get_ref(&self) -> &R {
        &self.inner
    }
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for BufReader<R> {
    fn read(&mut self, out: &mut [u8]) -> Result<usize> {
        if self.pos >= self.len {
            return self.inner.read(out);
        }
        let avail = self.len - self.pos;
        let n = if avail < out.len() { avail } else { out.len() };
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl<R: Read> BufRead for BufReader<R> {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        if self.pos >= self.len {
            let n = self.inner.read(&mut self.buf)?;
            self.pos = 0;
            self.len = n;
        }
        Ok(&self.buf[self.pos..self.len])
    }
    fn consume(&mut self, amt: usize) {
        self.pos += amt;
    }
}

/// A buffered writer. hrc has no destructors to flush it at the end, so
/// it writes through (standard output is buffered by the runtime anyway).
pub struct BufWriter<W: Write> {
    inner: W,
}

impl<W: Write> BufWriter<W> {
    pub fn new(inner: W) -> BufWriter<W> {
        BufWriter { inner }
    }
    pub fn with_capacity(_cap: usize, inner: W) -> BufWriter<W> {
        BufWriter { inner }
    }
    pub fn get_ref(&self) -> &W {
        &self.inner
    }
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }
    pub fn into_inner(self) -> crate::result::Result<W, Error> {
        Ok(self.inner)
    }
}

impl<W: Write> Write for BufWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

pub type LineWriter<W> = BufWriter<W>;

pub struct Cursor<T> {
    inner: T,
    pos: u64,
}

impl<T> Cursor<T> {
    pub fn new(inner: T) -> Cursor<T> {
        Cursor { inner, pos: 0 }
    }
    pub fn into_inner(self) -> T {
        self.inner
    }
    pub fn get_ref(&self) -> &T {
        &self.inner
    }
    pub fn position(&self) -> u64 {
        self.pos
    }
    pub fn set_position(&mut self, pos: u64) {
        self.pos = pos;
    }
}

impl<T: AsRef<[u8]>> Read for Cursor<T> {
    fn read(&mut self, out: &mut [u8]) -> Result<usize> {
        let data = <T as AsRef<[u8]>>::as_ref(&self.inner);
        let start = if (self.pos as usize) < data.len() { self.pos as usize } else { data.len() };
        let avail = data.len() - start;
        let n = if avail < out.len() { avail } else { out.len() };
        out[..n].copy_from_slice(&data[start..start + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl<T: AsRef<[u8]>> BufRead for Cursor<T> {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        let data = <T as AsRef<[u8]>>::as_ref(&self.inner);
        let start = if (self.pos as usize) < data.len() { self.pos as usize } else { data.len() };
        Ok(&data[start..])
    }
    fn consume(&mut self, amt: usize) {
        self.pos += amt as u64;
    }
}

impl<'a> Read for &'a [u8] {
    fn read(&mut self, out: &mut [u8]) -> Result<usize> {
        let n = if self.len() < out.len() { self.len() } else { out.len() };
        out[..n].copy_from_slice(&self[..n]);
        *self = &self[n..];
        Ok(n)
    }
}

impl<'a> BufRead for &'a [u8] {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        Ok(*self)
    }
    fn consume(&mut self, amt: usize) {
        *self = &self[amt..];
    }
}

pub struct Sink;

pub fn sink() -> Sink {
    Sink
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

pub mod prelude {
    pub use crate::io::{BufRead, Read, Write};
}
