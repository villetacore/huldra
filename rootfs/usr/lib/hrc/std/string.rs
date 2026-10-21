//! `String`, a growable UTF-8 string.

use crate::fmt;
use crate::option::Option::{self, None, Some};
use crate::vec::Vec;

#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct String {
    vec: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FromUtf8Error {
    bytes: Vec<u8>,
    error: crate::str::Utf8Error,
}

impl FromUtf8Error {
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
    pub fn utf8_error(&self) -> crate::str::Utf8Error {
        self.error
    }
}

impl fmt::Display for FromUtf8Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.error, f)
    }
}

impl String {
    pub fn new() -> String {
        String { vec: Vec::new() }
    }
    pub fn with_capacity(n: usize) -> String {
        String { vec: Vec::with_capacity(n) }
    }
    pub fn from_utf8(vec: Vec<u8>) -> Result<String, FromUtf8Error> {
        match crate::str::validate_utf8(&vec) {
            Ok(()) => Ok(String { vec }),
            Err(error) => Err(FromUtf8Error { bytes: vec, error }),
        }
    }
    pub fn from_utf8_lossy(v: &[u8]) -> String {
        let mut s = String::new();
        let mut rest = v;
        loop {
            match crate::str::validate_utf8(rest) {
                Ok(()) => {
                    s.push_str(unsafe { crate::str::from_utf8_unchecked(rest) });
                    return s;
                }
                Err(e) => {
                    let good = e.valid_up_to();
                    s.push_str(unsafe { crate::str::from_utf8_unchecked(&rest[..good]) });
                    s.push('\u{FFFD}');
                    let skip = match e.error_len() {
                        Some(n) => n,
                        None => rest.len() - good,
                    };
                    rest = &rest[good + skip..];
                }
            }
        }
    }
    pub unsafe fn from_utf8_unchecked(vec: Vec<u8>) -> String {
        String { vec }
    }
    pub fn as_str(&self) -> &str {
        unsafe { crate::str::from_utf8_unchecked(self.vec.as_slice()) }
    }
    pub fn as_mut_str(&mut self) -> &mut str {
        unsafe { crate::str::from_utf8_unchecked_mut(self.vec.as_mut_slice()) }
    }
    pub fn as_bytes(&self) -> &[u8] {
        self.vec.as_slice()
    }
    pub unsafe fn as_mut_vec(&mut self) -> &mut Vec<u8> {
        &mut self.vec
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.vec
    }
    pub fn len(&self) -> usize {
        self.vec.len()
    }
    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }
    pub fn capacity(&self) -> usize {
        self.vec.capacity()
    }
    pub fn reserve(&mut self, n: usize) {
        self.vec.reserve(n);
    }
    pub fn clear(&mut self) {
        self.vec.clear();
    }
    pub fn push_str(&mut self, s: &str) {
        let b = s.as_bytes();
        let n = b.len();
        self.vec.reserve(n);
        unsafe {
            crate::intrinsics::ptr_copy(b.as_ptr(), self.vec.as_mut_ptr().add(self.vec.len()), n);
            let l = self.vec.len();
            self.vec.set_len(l + n);
        }
    }
    pub fn push(&mut self, c: char) {
        let v = c as u32;
        if v < 0x80 {
            self.vec.push(v as u8);
        } else {
            let mut b = [0u8; 4];
            let s = c.encode_utf8(&mut b);
            self.push_str(s);
        }
    }
    pub fn pop(&mut self) -> Option<char> {
        let c = self.as_str().chars().next_back()?;
        let n = self.len() - c.len_utf8();
        self.vec.truncate(n);
        Some(c)
    }
    #[track_caller]
    pub fn truncate(&mut self, n: usize) {
        if n < self.len() {
            if !self.is_char_boundary(n) {
                panic!("assertion failed: self.is_char_boundary(new_len)");
            }
            self.vec.truncate(n);
        }
    }
    #[track_caller]
    pub fn remove(&mut self, idx: usize) -> char {
        let c = match self[idx..].chars().next() {
            Some(c) => c,
            None => panic!("cannot remove a char from the end of a string"),
        };
        let n = c.len_utf8();
        let mut i = idx;
        while i + n < self.len() {
            let b = self.vec[i + n];
            self.vec[i] = b;
            i += 1;
        }
        let l = self.len();
        self.vec.truncate(l - n);
        c
    }
    #[track_caller]
    pub fn insert(&mut self, idx: usize, c: char) {
        let mut b = [0u8; 4];
        let s = c.encode_utf8(&mut b);
        self.insert_str(idx, s);
    }
    #[track_caller]
    pub fn insert_str(&mut self, idx: usize, s: &str) {
        if !self.is_char_boundary(idx) {
            panic!("assertion failed: self.is_char_boundary(idx)");
        }
        let tail = self.vec.split_off(idx);
        self.push_str(s);
        self.vec.extend_from_slice(&tail);
    }
    pub fn retain<F: FnMut(char) -> bool>(&mut self, mut f: F) {
        let mut out = String::with_capacity(self.len());
        for c in self.chars() {
            if f(c) {
                out.push(c);
            }
        }
        *self = out;
    }
    pub fn split_off(&mut self, at: usize) -> String {
        if !self.is_char_boundary(at) {
            panic!("assertion failed: self.is_char_boundary(at)");
        }
        String { vec: self.vec.split_off(at) }
    }
    pub fn drain<R: crate::vec::RangeArg>(&mut self, range: R) -> crate::vec::IntoIter<char> {
        let (lo, hi) = range.bounds(self.len());
        let removed: Vec<char> = self[lo..hi].chars().collect();
        let tail = self.vec.split_off(hi);
        self.vec.truncate(lo);
        self.vec.extend_from_slice(&tail);
        removed.into_iter()
    }
    pub fn replace_range<R: crate::vec::RangeArg>(&mut self, range: R, with: &str) {
        let (lo, hi) = range.bounds(self.len());
        let tail = self.vec.split_off(hi);
        self.vec.truncate(lo);
        self.push_str(with);
        self.vec.extend_from_slice(&tail);
    }
    pub fn leak<'a>(self) -> &'a mut str {
        let v = self.vec.leak();
        unsafe { crate::str::from_utf8_unchecked_mut(v) }
    }
}

impl crate::ops::Deref for String {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl crate::ops::DerefMut for String {
    fn deref_mut(&mut self) -> &mut str {
        self.as_mut_str()
    }
}

impl fmt::Display for String {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.pad(self.as_str())
    }
}

impl fmt::Debug for String {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Write for String {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.push_str(s);
        Ok(())
    }
    fn write_char(&mut self, c: char) -> fmt::Result {
        self.push(c);
        Ok(())
    }
}

impl From<&str> for String {
    fn from(s: &str) -> String {
        let mut r = String::with_capacity(s.len());
        r.push_str(s);
        r
    }
}

impl From<&mut str> for String {
    fn from(s: &mut str) -> String {
        String::from(&*s)
    }
}

impl From<&String> for String {
    fn from(s: &String) -> String {
        s.clone()
    }
}

impl From<char> for String {
    fn from(c: char) -> String {
        let mut s = String::new();
        s.push(c);
        s
    }
}

impl From<String> for Vec<u8> {
    fn from(s: String) -> Vec<u8> {
        s.vec
    }
}

impl crate::ops::Add<&str> for String {
    type Output = String;
    fn add(mut self, other: &str) -> String {
        self.push_str(other);
        self
    }
}

impl crate::ops::AddAssign<&str> for String {
    fn add_assign(&mut self, other: &str) {
        self.push_str(other);
    }
}

impl crate::ops::AddAssign<char> for String {
    fn add_assign(&mut self, c: char) {
        self.push(c);
    }
}

impl PartialEq<str> for String {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}
impl<'a> PartialEq<&'a str> for String {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}
impl PartialEq<String> for str {
    fn eq(&self, other: &String) -> bool {
        self == other.as_str()
    }
}
impl<'a> PartialEq<String> for &'a str {
    fn eq(&self, other: &String) -> bool {
        *self == other.as_str()
    }
}

impl AsRef<str> for String {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<[u8]> for String {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl crate::borrow::Borrow<str> for String {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl crate::iter::FromIterator<char> for String {
    fn from_iter<I: IntoIterator<Item = char>>(iter: I) -> String {
        let mut s = String::new();
        for c in iter {
            s.push(c);
        }
        s
    }
}

impl<'a> crate::iter::FromIterator<&'a char> for String {
    fn from_iter<I: IntoIterator<Item = &'a char>>(iter: I) -> String {
        let mut s = String::new();
        for c in iter {
            s.push(*c);
        }
        s
    }
}

impl<'a> crate::iter::FromIterator<&'a str> for String {
    fn from_iter<I: IntoIterator<Item = &'a str>>(iter: I) -> String {
        let mut s = String::new();
        for x in iter {
            s.push_str(x);
        }
        s
    }
}

impl crate::iter::FromIterator<String> for String {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> String {
        let mut s = String::new();
        for x in iter {
            s.push_str(&x);
        }
        s
    }
}

impl crate::iter::Extend<char> for String {
    fn extend<I: IntoIterator<Item = char>>(&mut self, iter: I) {
        for c in iter {
            self.push(c);
        }
    }
    fn extend_one(&mut self, c: char) {
        self.push(c);
    }
}

impl<'a> crate::iter::Extend<&'a char> for String {
    fn extend<I: IntoIterator<Item = &'a char>>(&mut self, iter: I) {
        for c in iter {
            self.push(*c);
        }
    }
}

impl<'a> crate::iter::Extend<&'a str> for String {
    fn extend<I: IntoIterator<Item = &'a str>>(&mut self, iter: I) {
        for s in iter {
            self.push_str(s);
        }
    }
}

impl crate::iter::Extend<String> for String {
    fn extend<I: IntoIterator<Item = String>>(&mut self, iter: I) {
        for s in iter {
            self.push_str(&s);
        }
    }
}

/// `to_string()` for everything that implements `Display`.
pub trait ToString {
    fn to_string(&self) -> String;
}

impl<T: fmt::Display + ?Sized> ToString for T {
    fn to_string(&self) -> String {
        let mut f = fmt::Formatter::new();
        let _ = fmt::Display::fmt(self, &mut f);
        f.into_string()
    }
}

