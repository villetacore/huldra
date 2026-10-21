//! The process environment: arguments and variables.

use crate::fmt;
use crate::option::Option::{self, None, Some};
use crate::string::String;

extern "C" {
    fn getenv(name: *const u8) -> *const u8;
    fn strlen(s: *const u8) -> usize;
}

pub struct Args {
    i: usize,
    n: usize,
}

pub fn args() -> Args {
    Args {
        i: 0,
        n: unsafe { crate::sys::hrc_arg_count() },
    }
}

impl Iterator for Args {
    type Item = String;
    fn next(&mut self) -> Option<String> {
        if self.i < self.n {
            let s = unsafe { crate::sys::hrc_arg(self.i) };
            self.i += 1;
            Some(String::from(s))
        } else {
            None
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.n - self.i, Some(self.n - self.i))
    }
}

impl ExactSizeIterator for Args {}

impl DoubleEndedIterator for Args {
    fn next_back(&mut self) -> Option<String> {
        if self.i < self.n {
            self.n -= 1;
            Some(String::from(unsafe { crate::sys::hrc_arg(self.n) }))
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VarError {
    NotPresent,
    NotUnicode(String),
}

impl fmt::Display for VarError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            VarError::NotPresent => f.write_str("environment variable not found"),
            VarError::NotUnicode(_) => f.write_str("environment variable was not valid unicode"),
        }
    }
}

pub fn var<K: AsRef<str>>(key: K) -> Result<String, VarError> {
    let mut name = String::from(<K as AsRef<str>>::as_ref(&key));
    name.push('\0');
    unsafe {
        let p = getenv(name.as_ptr());
        if p.is_null() {
            return Err(VarError::NotPresent);
        }
        let n = strlen(p);
        let bytes = crate::slice::from_raw_parts(p, n);
        match crate::str::from_utf8(bytes) {
            Ok(s) => Ok(String::from(s)),
            Err(_) => Err(VarError::NotUnicode(String::from_utf8_lossy(bytes))),
        }
    }
}

pub fn var_os<K: AsRef<str>>(key: K) -> Option<String> {
    var(key).ok()
}
