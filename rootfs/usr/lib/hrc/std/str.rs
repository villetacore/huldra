//! String slices: methods on `str`, patterns, iterators and parsing.

use crate::fmt;
use crate::intrinsics;
use crate::option::Option::{self, None, Some};
use crate::string::String;
use crate::vec::Vec;

pub unsafe fn from_utf8_unchecked(v: &[u8]) -> &str {
    intrinsics::str_from_raw(v.as_ptr(), v.len())
}

pub unsafe fn from_utf8_unchecked_mut(v: &mut [u8]) -> &mut str {
    &mut *(intrinsics::str_from_raw(v.as_ptr(), v.len()) as *const str as *mut str)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Utf8Error {
    valid_up_to: usize,
    error_len: Option<u8>,
}

impl Utf8Error {
    pub fn valid_up_to(&self) -> usize {
        self.valid_up_to
    }
    pub fn error_len(&self) -> Option<usize> {
        match self.error_len {
            Some(n) => Some(n as usize),
            None => None,
        }
    }
}

impl fmt::Display for Utf8Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.error_len {
            Some(n) => write!(f, "invalid utf-8 sequence of {} bytes from index {}", n, self.valid_up_to),
            None => write!(f, "incomplete utf-8 byte sequence from index {}", self.valid_up_to),
        }
    }
}

/// Checks UTF-8; returns the error position on failure.
pub fn validate_utf8(v: &[u8]) -> Result<(), Utf8Error> {
    let n = v.len();
    let mut i = 0;
    while i < n {
        let b = v[i];
        let len = if b < 0x80 {
            1
        } else if b >= 0xC2 && b < 0xE0 {
            2
        } else if b >= 0xE0 && b < 0xF0 {
            3
        } else if b >= 0xF0 && b < 0xF5 {
            4
        } else {
            return Err(Utf8Error {
                valid_up_to: i,
                error_len: Some(1),
            });
        };
        if i + len > n {
            // Check what is there before declaring it incomplete.
            let mut k = 1;
            while i + k < n {
                if v[i + k] & 0xC0 != 0x80 {
                    return Err(Utf8Error {
                        valid_up_to: i,
                        error_len: Some(k as u8),
                    });
                }
                k += 1;
            }
            return Err(Utf8Error {
                valid_up_to: i,
                error_len: None,
            });
        }
        let mut k = 1;
        while k < len {
            let c = v[i + k];
            let ok = if k == 1 {
                match b {
                    0xE0 => c >= 0xA0 && c < 0xC0,
                    0xED => c >= 0x80 && c < 0xA0,
                    0xF0 => c >= 0x90 && c < 0xC0,
                    0xF4 => c >= 0x80 && c < 0x90,
                    _ => c & 0xC0 == 0x80,
                }
            } else {
                c & 0xC0 == 0x80
            };
            if !ok {
                return Err(Utf8Error {
                    valid_up_to: i,
                    error_len: Some(k as u8),
                });
            }
            k += 1;
        }
        i += len;
    }
    Ok(())
}

pub fn from_utf8(v: &[u8]) -> Result<&str, Utf8Error> {
    validate_utf8(v)?;
    Ok(unsafe { from_utf8_unchecked(v) })
}

/// Decodes the character starting at byte `i` (which must be a boundary).
fn decode(b: &[u8], i: usize) -> (char, usize) {
    let c0 = b[i] as u32;
    if c0 < 0x80 {
        return (unsafe { crate::char::from_u32_unchecked(c0) }, 1);
    }
    if c0 < 0xE0 {
        let c = ((c0 & 0x1F) << 6) | (b[i + 1] as u32 & 0x3F);
        return (unsafe { crate::char::from_u32_unchecked(c) }, 2);
    }
    if c0 < 0xF0 {
        let c = ((c0 & 0x0F) << 12) | ((b[i + 1] as u32 & 0x3F) << 6) | (b[i + 2] as u32 & 0x3F);
        return (unsafe { crate::char::from_u32_unchecked(c) }, 3);
    }
    let c = ((c0 & 0x07) << 18) | ((b[i + 1] as u32 & 0x3F) << 12) | ((b[i + 2] as u32 & 0x3F) << 6) | (b[i + 3] as u32 & 0x3F);
    (unsafe { crate::char::from_u32_unchecked(c) }, 4)
}

/// The start of the character ending at byte `j`.
fn char_start_before(b: &[u8], j: usize) -> usize {
    let mut i = j - 1;
    while i > 0 && b[i] & 0xC0 == 0x80 {
        i -= 1;
    }
    i
}

pub trait FromStr: Sized {
    type Err;
    fn from_str(s: &str) -> Result<Self, Self::Err>;
}

impl str {
    pub fn len(&self) -> usize {
        intrinsics::str_len(self)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn as_bytes(&self) -> &[u8] {
        intrinsics::slice_from_raw(intrinsics::str_ptr(self), intrinsics::str_len(self))
    }
    pub unsafe fn as_bytes_mut(&mut self) -> &mut [u8] {
        intrinsics::slice_from_raw_mut(intrinsics::str_ptr(self) as *mut u8, intrinsics::str_len(self))
    }
    pub fn as_ptr(&self) -> *const u8 {
        intrinsics::str_ptr(self)
    }
    pub fn as_str(&self) -> &str {
        self
    }
    pub fn is_char_boundary(&self, i: usize) -> bool {
        if i == 0 || i == self.len() {
            return true;
        }
        if i > self.len() {
            return false;
        }
        self.as_bytes()[i] & 0xC0 != 0x80
    }
    pub fn chars(&self) -> Chars<'_> {
        Chars {
            s: self,
            i: 0,
            j: self.len(),
        }
    }
    pub fn char_indices(&self) -> CharIndices<'_> {
        CharIndices {
            s: self,
            i: 0,
            j: self.len(),
        }
    }
    pub fn bytes(&self) -> crate::iter::Copied<crate::slice::Iter<'_, u8>> {
        self.as_bytes().iter().copied()
    }
    pub fn get<R: crate::str::StrIndex>(&self, r: R) -> Option<&str> {
        let (lo, hi) = r.bounds(self.len());
        if lo <= hi && hi <= self.len() && self.is_char_boundary(lo) && self.is_char_boundary(hi) {
            Some(&self[lo..hi])
        } else {
            None
        }
    }
    pub fn split_at(&self, mid: usize) -> (&str, &str) {
        (&self[..mid], &self[mid..])
    }
    pub fn split_at_checked(&self, mid: usize) -> Option<(&str, &str)> {
        if self.is_char_boundary(mid) {
            Some((&self[..mid], &self[mid..]))
        } else {
            None
        }
    }
    pub fn lines(&self) -> Lines<'_> {
        Lines { s: self, i: 0 }
    }
    pub fn split_whitespace(&self) -> SplitWhitespace<'_> {
        SplitWhitespace { s: self, i: 0 }
    }
    pub fn split_ascii_whitespace(&self) -> SplitWhitespace<'_> {
        SplitWhitespace { s: self, i: 0 }
    }
    pub fn split<P: Pattern>(&self, pat: P) -> Split<'_, P> {
        Split {
            s: self,
            pat,
            i: 0,
            from: 0,
            done: false,
            limit: usize::MAX,
            terminator: false,
        }
    }
    pub fn split_terminator<P: Pattern>(&self, pat: P) -> Split<'_, P> {
        Split {
            s: self,
            pat,
            i: 0,
            from: 0,
            done: false,
            limit: usize::MAX,
            terminator: true,
        }
    }
    pub fn splitn<P: Pattern>(&self, n: usize, pat: P) -> Split<'_, P> {
        Split {
            s: self,
            pat,
            i: 0,
            from: 0,
            done: n == 0,
            limit: n,
            terminator: false,
        }
    }
    pub fn rsplit<P: Pattern>(&self, pat: P) -> crate::vec::IntoIter<&str> {
        let mut v: Vec<&str> = self.split(pat).collect();
        v.reverse();
        v.into_iter()
    }
    pub fn rsplitn<P: Pattern>(&self, n: usize, mut pat: P) -> crate::vec::IntoIter<&str> {
        let mut v: Vec<&str> = Vec::new();
        let mut end = self.len();
        while v.len() + 1 < n {
            match pat.rfind_in(self, end) {
                Some((a, b)) => {
                    v.push(&self[b..end]);
                    end = a;
                }
                None => break,
            }
        }
        if n > 0 {
            v.push(&self[..end]);
        }
        v.into_iter()
    }
    pub fn split_once<P: Pattern>(&self, mut pat: P) -> Option<(&str, &str)> {
        let (a, b) = pat.find_in(self, 0)?;
        Some((&self[..a], &self[b..]))
    }
    pub fn rsplit_once<P: Pattern>(&self, mut pat: P) -> Option<(&str, &str)> {
        let (a, b) = pat.rfind_in(self, self.len())?;
        Some((&self[..a], &self[b..]))
    }
    pub fn split_inclusive<P: Pattern>(&self, mut pat: P) -> crate::vec::IntoIter<&str> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.len() {
            match pat.find_in(self, i) {
                Some((_, b)) if b > i => {
                    v.push(&self[i..b]);
                    i = b;
                }
                _ => {
                    v.push(&self[i..]);
                    break;
                }
            }
        }
        v.into_iter()
    }
    pub fn contains<P: Pattern>(&self, mut pat: P) -> bool {
        pat.find_in(self, 0).is_some()
    }
    pub fn starts_with<P: Pattern>(&self, mut pat: P) -> bool {
        pat.is_prefix_of(self)
    }
    pub fn ends_with<P: Pattern>(&self, mut pat: P) -> bool {
        pat.is_suffix_of(self)
    }
    pub fn find<P: Pattern>(&self, mut pat: P) -> Option<usize> {
        match pat.find_in(self, 0) {
            Some((a, _)) => Some(a),
            None => None,
        }
    }
    pub fn rfind<P: Pattern>(&self, mut pat: P) -> Option<usize> {
        match pat.rfind_in(self, self.len()) {
            Some((a, _)) => Some(a),
            None => None,
        }
    }
    pub fn matches<P: Pattern>(&self, mut pat: P) -> crate::vec::IntoIter<&str> {
        let mut v = Vec::new();
        let mut i = 0;
        while let Some((a, b)) = pat.find_in(self, i) {
            v.push(&self[a..b]);
            i = if b > a { b } else { b + self[b..].chars().next().map(|c| c.len_utf8()).unwrap_or(1) };
            if i > self.len() {
                break;
            }
        }
        v.into_iter()
    }
    pub fn match_indices<P: Pattern>(&self, mut pat: P) -> crate::vec::IntoIter<(usize, &str)> {
        let mut v = Vec::new();
        let mut i = 0;
        while let Some((a, b)) = pat.find_in(self, i) {
            v.push((a, &self[a..b]));
            i = if b > a { b } else { b + 1 };
            if i > self.len() {
                break;
            }
        }
        v.into_iter()
    }
    pub fn replace<P: Pattern>(&self, mut from: P, to: &str) -> String {
        let mut out = String::new();
        let mut i = 0;
        while let Some((a, b)) = from.find_in(self, i) {
            out.push_str(&self[i..a]);
            out.push_str(to);
            if b == a {
                // An empty match: copy one character and move on.
                match self[b..].chars().next() {
                    Some(c) => {
                        out.push(c);
                        i = b + c.len_utf8();
                    }
                    None => {
                        i = b;
                        break;
                    }
                }
            } else {
                i = b;
            }
        }
        if i <= self.len() {
            out.push_str(&self[i..]);
        }
        out
    }
    pub fn replacen<P: Pattern>(&self, mut from: P, to: &str, count: usize) -> String {
        let mut out = String::new();
        let mut i = 0;
        let mut n = 0;
        while n < count {
            match from.find_in(self, i) {
                Some((a, b)) => {
                    out.push_str(&self[i..a]);
                    out.push_str(to);
                    i = b;
                    n += 1;
                }
                None => break,
            }
        }
        out.push_str(&self[i..]);
        out
    }
    pub fn trim(&self) -> &str {
        self.trim_start().trim_end()
    }
    pub fn trim_start(&self) -> &str {
        let mut it = self.char_indices();
        while let Some((i, c)) = it.next() {
            if !c.is_whitespace() {
                return &self[i..];
            }
        }
        ""
    }
    pub fn trim_end(&self) -> &str {
        let mut it = self.char_indices();
        while let Some((i, c)) = it.next_back() {
            if !c.is_whitespace() {
                return &self[..i + c.len_utf8()];
            }
        }
        ""
    }
    pub fn trim_left(&self) -> &str {
        self.trim_start()
    }
    pub fn trim_right(&self) -> &str {
        self.trim_end()
    }
    pub fn trim_matches<P: Pattern>(&self, mut pat: P) -> &str {
        let mut s = self;
        loop {
            match pat.find_in(s, 0) {
                Some((0, b)) if b > 0 => s = &s[b..],
                _ => break,
            }
        }
        loop {
            match pat.rfind_in(s, s.len()) {
                Some((a, b)) if b == s.len() && a < b => s = &s[..a],
                _ => break,
            }
        }
        s
    }
    pub fn trim_start_matches<P: Pattern>(&self, mut pat: P) -> &str {
        let mut s = self;
        loop {
            match pat.find_in(s, 0) {
                Some((0, b)) if b > 0 => s = &s[b..],
                _ => return s,
            }
        }
    }
    pub fn trim_end_matches<P: Pattern>(&self, mut pat: P) -> &str {
        let mut s = self;
        loop {
            match pat.rfind_in(s, s.len()) {
                Some((a, b)) if b == s.len() && a < b => s = &s[..a],
                _ => return s,
            }
        }
    }
    pub fn strip_prefix<P: Pattern>(&self, mut pat: P) -> Option<&str> {
        if !pat.is_prefix_of(self) {
            return None;
        }
        match pat.find_in(self, 0) {
            Some((_, b)) => Some(&self[b..]),
            None => None,
        }
    }
    pub fn strip_suffix<P: Pattern>(&self, mut pat: P) -> Option<&str> {
        if !pat.is_suffix_of(self) {
            return None;
        }
        match pat.rfind_in(self, self.len()) {
            Some((a, b)) if b == self.len() => Some(&self[..a]),
            _ => None,
        }
    }
    pub fn to_uppercase(&self) -> String {
        let mut s = String::with_capacity(self.len());
        for c in self.chars() {
            for u in c.to_uppercase() {
                s.push(u);
            }
        }
        s
    }
    pub fn to_lowercase(&self) -> String {
        let mut s = String::with_capacity(self.len());
        for c in self.chars() {
            for u in c.to_lowercase() {
                s.push(u);
            }
        }
        s
    }
    pub fn to_ascii_uppercase(&self) -> String {
        let mut s = String::with_capacity(self.len());
        for c in self.chars() {
            s.push(c.to_ascii_uppercase());
        }
        s
    }
    pub fn to_ascii_lowercase(&self) -> String {
        let mut s = String::with_capacity(self.len());
        for c in self.chars() {
            s.push(c.to_ascii_lowercase());
        }
        s
    }
    pub fn make_ascii_uppercase(&mut self) {
        let b = unsafe { self.as_bytes_mut() };
        let mut i = 0;
        while i < b.len() {
            b[i] = b[i].to_ascii_uppercase();
            i += 1;
        }
    }
    pub fn make_ascii_lowercase(&mut self) {
        let b = unsafe { self.as_bytes_mut() };
        let mut i = 0;
        while i < b.len() {
            b[i] = b[i].to_ascii_lowercase();
            i += 1;
        }
    }
    pub fn eq_ignore_ascii_case(&self, other: &str) -> bool {
        let (a, b) = (self.as_bytes(), other.as_bytes());
        if a.len() != b.len() {
            return false;
        }
        let mut i = 0;
        while i < a.len() {
            if a[i].to_ascii_lowercase() != b[i].to_ascii_lowercase() {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn is_ascii(&self) -> bool {
        for b in self.as_bytes() {
            if *b >= 0x80 {
                return false;
            }
        }
        true
    }
    pub fn repeat(&self, n: usize) -> String {
        let mut s = String::with_capacity(self.len() * n);
        let mut i = 0;
        while i < n {
            s.push_str(self);
            i += 1;
        }
        s
    }
    pub fn parse<F: FromStr>(&self) -> Result<F, F::Err> {
        F::from_str(self)
    }
    pub fn to_owned(&self) -> String {
        String::from(self)
    }
    pub fn into_string(&self) -> String {
        String::from(self)
    }
    pub fn escape_debug(&self) -> String {
        format!("{:?}", self).trim_matches('"').to_owned()
    }
    pub fn encode_utf16(&self) -> crate::vec::IntoIter<u16> {
        let mut v = Vec::new();
        for c in self.chars() {
            let mut buf = [0u16; 2];
            for u in c.encode_utf16(&mut buf).iter() {
                v.push(*u);
            }
        }
        v.into_iter()
    }
}

/// Ranges that index a `str` (`s.get(1..3)`).
pub trait StrIndex {
    fn bounds(&self, len: usize) -> (usize, usize);
}

impl StrIndex for crate::ops::Range<usize> {
    fn bounds(&self, _len: usize) -> (usize, usize) {
        (self.start, self.end)
    }
}
impl StrIndex for crate::ops::RangeInclusive<usize> {
    fn bounds(&self, _len: usize) -> (usize, usize) {
        (self.start, self.end + 1)
    }
}
impl StrIndex for crate::ops::RangeFrom<usize> {
    fn bounds(&self, len: usize) -> (usize, usize) {
        (self.start, len)
    }
}
impl StrIndex for crate::ops::RangeTo<usize> {
    fn bounds(&self, _len: usize) -> (usize, usize) {
        (0, self.end)
    }
}
impl StrIndex for crate::ops::RangeFull {
    fn bounds(&self, len: usize) -> (usize, usize) {
        (0, len)
    }
}

// ---- patterns ----

/// What `split`, `find`, `contains`, ... search for.
pub trait Pattern {
    /// The first match starting at or after byte `from`: (start, end).
    fn find_in(&mut self, s: &str, from: usize) -> Option<(usize, usize)>;
    /// The last match ending at or before byte `to`.
    fn rfind_in(&mut self, s: &str, to: usize) -> Option<(usize, usize)>;
    fn is_prefix_of(&mut self, s: &str) -> bool;
    fn is_suffix_of(&mut self, s: &str) -> bool;
}

fn find_bytes(h: &[u8], n: &[u8], from: usize) -> Option<usize> {
    if n.len() > h.len() {
        return None;
    }
    let mut i = from;
    while i + n.len() <= h.len() {
        let mut k = 0;
        while k < n.len() && h[i + k] == n[k] {
            k += 1;
        }
        if k == n.len() {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn rfind_bytes(h: &[u8], n: &[u8], to: usize) -> Option<usize> {
    if n.len() > to {
        return None;
    }
    let mut i = to - n.len() + 1;
    while i > 0 {
        i -= 1;
        let mut k = 0;
        while k < n.len() && h[i + k] == n[k] {
            k += 1;
        }
        if k == n.len() {
            return Some(i);
        }
    }
    None
}

impl<'b> Pattern for &'b str {
    fn find_in(&mut self, s: &str, from: usize) -> Option<(usize, usize)> {
        let n = self.len();
        match find_bytes(s.as_bytes(), self.as_bytes(), from) {
            Some(i) => Some((i, i + n)),
            None => None,
        }
    }
    fn rfind_in(&mut self, s: &str, to: usize) -> Option<(usize, usize)> {
        let n = self.len();
        match rfind_bytes(s.as_bytes(), self.as_bytes(), to) {
            Some(i) => Some((i, i + n)),
            None => None,
        }
    }
    fn is_prefix_of(&mut self, s: &str) -> bool {
        s.as_bytes().starts_with(self.as_bytes())
    }
    fn is_suffix_of(&mut self, s: &str) -> bool {
        s.as_bytes().ends_with(self.as_bytes())
    }
}

impl<'b> Pattern for &'b String {
    fn find_in(&mut self, s: &str, from: usize) -> Option<(usize, usize)> {
        let mut p: &str = self.as_str();
        p.find_in(s, from)
    }
    fn rfind_in(&mut self, s: &str, to: usize) -> Option<(usize, usize)> {
        let mut p: &str = self.as_str();
        p.rfind_in(s, to)
    }
    fn is_prefix_of(&mut self, s: &str) -> bool {
        let mut p: &str = self.as_str();
        p.is_prefix_of(s)
    }
    fn is_suffix_of(&mut self, s: &str) -> bool {
        let mut p: &str = self.as_str();
        p.is_suffix_of(s)
    }
}

impl<'b, 'c> Pattern for &'c &'b str {
    fn find_in(&mut self, s: &str, from: usize) -> Option<(usize, usize)> {
        let mut p: &str = *self;
        p.find_in(s, from)
    }
    fn rfind_in(&mut self, s: &str, to: usize) -> Option<(usize, usize)> {
        let mut p: &str = *self;
        p.rfind_in(s, to)
    }
    fn is_prefix_of(&mut self, s: &str) -> bool {
        let mut p: &str = *self;
        p.is_prefix_of(s)
    }
    fn is_suffix_of(&mut self, s: &str) -> bool {
        let mut p: &str = *self;
        p.is_suffix_of(s)
    }
}

/// A pattern of characters matching a predicate.
fn find_char_by<F: FnMut(char) -> bool>(s: &str, from: usize, f: &mut F) -> Option<(usize, usize)> {
    let b = s.as_bytes();
    let mut i = from;
    while i < b.len() {
        let (c, n) = decode(b, i);
        if f(c) {
            return Some((i, i + n));
        }
        i += n;
    }
    None
}

fn rfind_char_by<F: FnMut(char) -> bool>(s: &str, to: usize, f: &mut F) -> Option<(usize, usize)> {
    let b = s.as_bytes();
    let mut j = to;
    while j > 0 {
        let i = char_start_before(b, j);
        let (c, _) = decode(b, i);
        if f(c) {
            return Some((i, j));
        }
        j = i;
    }
    None
}

impl Pattern for char {
    fn find_in(&mut self, s: &str, from: usize) -> Option<(usize, usize)> {
        let me = *self;
        find_char_by(s, from, &mut |c: char| c == me)
    }
    fn rfind_in(&mut self, s: &str, to: usize) -> Option<(usize, usize)> {
        let me = *self;
        rfind_char_by(s, to, &mut |c: char| c == me)
    }
    fn is_prefix_of(&mut self, s: &str) -> bool {
        s.chars().next() == Some(*self)
    }
    fn is_suffix_of(&mut self, s: &str) -> bool {
        s.chars().next_back() == Some(*self)
    }
}

impl<F: FnMut(char) -> bool> Pattern for F {
    fn find_in(&mut self, s: &str, from: usize) -> Option<(usize, usize)> {
        find_char_by(s, from, self)
    }
    fn rfind_in(&mut self, s: &str, to: usize) -> Option<(usize, usize)> {
        rfind_char_by(s, to, self)
    }
    fn is_prefix_of(&mut self, s: &str) -> bool {
        match s.chars().next() {
            Some(c) => self(c),
            None => false,
        }
    }
    fn is_suffix_of(&mut self, s: &str) -> bool {
        match s.chars().next_back() {
            Some(c) => self(c),
            None => false,
        }
    }
}

impl<const N: usize> Pattern for [char; N] {
    fn find_in(&mut self, s: &str, from: usize) -> Option<(usize, usize)> {
        let set = *self;
        find_char_by(s, from, &mut |c: char| set.contains(&c))
    }
    fn rfind_in(&mut self, s: &str, to: usize) -> Option<(usize, usize)> {
        let set = *self;
        rfind_char_by(s, to, &mut |c: char| set.contains(&c))
    }
    fn is_prefix_of(&mut self, s: &str) -> bool {
        match s.chars().next() {
            Some(c) => self.contains(&c),
            None => false,
        }
    }
    fn is_suffix_of(&mut self, s: &str) -> bool {
        match s.chars().next_back() {
            Some(c) => self.contains(&c),
            None => false,
        }
    }
}

impl<'b> Pattern for &'b [char] {
    fn find_in(&mut self, s: &str, from: usize) -> Option<(usize, usize)> {
        let set = *self;
        find_char_by(s, from, &mut |c: char| set.contains(&c))
    }
    fn rfind_in(&mut self, s: &str, to: usize) -> Option<(usize, usize)> {
        let set = *self;
        rfind_char_by(s, to, &mut |c: char| set.contains(&c))
    }
    fn is_prefix_of(&mut self, s: &str) -> bool {
        match s.chars().next() {
            Some(c) => self.contains(&c),
            None => false,
        }
    }
    fn is_suffix_of(&mut self, s: &str) -> bool {
        match s.chars().next_back() {
            Some(c) => self.contains(&c),
            None => false,
        }
    }
}

// ---- iterators ----

#[derive(Clone)]
pub struct Chars<'a> {
    s: &'a str,
    i: usize,
    j: usize,
}

impl<'a> Chars<'a> {
    pub fn as_str(&self) -> &'a str {
        &self.s[self.i..self.j]
    }
}

impl<'a> Iterator for Chars<'a> {
    type Item = char;
    fn next(&mut self) -> Option<char> {
        if self.i < self.j {
            let (c, n) = decode(self.s.as_bytes(), self.i);
            self.i += n;
            Some(c)
        } else {
            None
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.j - self.i;
        ((n + 3) / 4, Some(n))
    }
    fn count(self) -> usize {
        let b = self.s.as_bytes();
        let mut n = 0;
        let mut i = self.i;
        while i < self.j {
            if b[i] & 0xC0 != 0x80 {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

impl<'a> DoubleEndedIterator for Chars<'a> {
    fn next_back(&mut self) -> Option<char> {
        if self.i < self.j {
            let start = char_start_before(self.s.as_bytes(), self.j);
            let (c, _) = decode(self.s.as_bytes(), start);
            self.j = start;
            Some(c)
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub struct CharIndices<'a> {
    s: &'a str,
    i: usize,
    j: usize,
}

impl<'a> CharIndices<'a> {
    pub fn as_str(&self) -> &'a str {
        &self.s[self.i..self.j]
    }
    pub fn offset(&self) -> usize {
        self.i
    }
}

impl<'a> Iterator for CharIndices<'a> {
    type Item = (usize, char);
    fn next(&mut self) -> Option<(usize, char)> {
        if self.i < self.j {
            let at = self.i;
            let (c, n) = decode(self.s.as_bytes(), self.i);
            self.i += n;
            Some((at, c))
        } else {
            None
        }
    }
}

impl<'a> DoubleEndedIterator for CharIndices<'a> {
    fn next_back(&mut self) -> Option<(usize, char)> {
        if self.i < self.j {
            let start = char_start_before(self.s.as_bytes(), self.j);
            let (c, _) = decode(self.s.as_bytes(), start);
            self.j = start;
            Some((start, c))
        } else {
            None
        }
    }
}

pub struct Split<'a, P> {
    s: &'a str,
    pat: P,
    /// Start of the next piece.
    i: usize,
    /// Where to search for the next separator.
    from: usize,
    done: bool,
    limit: usize,
    terminator: bool,
}

impl<'a, P: Pattern> Iterator for Split<'a, P> {
    type Item = &'a str;
    fn next(&mut self) -> Option<&'a str> {
        if self.done {
            return None;
        }
        if self.limit == 1 {
            self.done = true;
            return Some(&self.s[self.i..]);
        }
        match self.pat.find_in(self.s, self.from) {
            Some((a, b)) => {
                let part = &self.s[self.i..a];
                self.i = b;
                self.from = if b > a {
                    b
                } else {
                    // An empty match: search again after the next character.
                    match self.s[b..].chars().next() {
                        Some(c) => b + c.len_utf8(),
                        None => self.s.len() + 1,
                    }
                };
                self.limit = self.limit.saturating_sub(1);
                Some(part)
            }
            None => {
                self.done = true;
                let rest = &self.s[self.i..];
                if self.terminator && rest.is_empty() {
                    return None;
                }
                Some(rest)
            }
        }
    }
}

#[derive(Clone)]
pub struct SplitWhitespace<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Iterator for SplitWhitespace<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<&'a str> {
        let b = self.s.as_bytes();
        let n = b.len();
        // Skip whitespace.
        loop {
            if self.i >= n {
                return None;
            }
            let (c, k) = decode(b, self.i);
            if !c.is_whitespace() {
                break;
            }
            self.i += k;
        }
        let start = self.i;
        while self.i < n {
            let (c, k) = decode(b, self.i);
            if c.is_whitespace() {
                break;
            }
            self.i += k;
        }
        Some(&self.s[start..self.i])
    }
}

impl<'a> DoubleEndedIterator for SplitWhitespace<'a> {
    fn next_back(&mut self) -> Option<&'a str> {
        let rest = self.s[self.i..].trim_end();
        if rest.is_empty() {
            self.i = self.s.len();
            return None;
        }
        let end = self.i + rest.len();
        let mut start = end;
        let b = self.s.as_bytes();
        while start > self.i {
            let p = char_start_before(b, start);
            let (c, _) = decode(b, p);
            if c.is_whitespace() {
                break;
            }
            start = p;
        }
        let word = &self.s[start..end];
        self.s = &self.s[..start];
        Some(word)
    }
}

#[derive(Clone)]
pub struct Lines<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Iterator for Lines<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<&'a str> {
        let n = self.s.len();
        if self.i >= n {
            return None;
        }
        let b = self.s.as_bytes();
        let start = self.i;
        let mut j = start;
        while j < n && b[j] != b'\n' {
            j += 1;
        }
        self.i = if j < n { j + 1 } else { n };
        let mut end = j;
        if end > start && b[end - 1] == b'\r' {
            end -= 1;
        }
        Some(&self.s[start..end])
    }
}

// ---- parsing ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntErrorKind {
    Empty,
    InvalidDigit,
    PosOverflow,
    NegOverflow,
    Zero,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseIntError {
    kind: IntErrorKind,
}

impl ParseIntError {
    pub fn kind(&self) -> &IntErrorKind {
        &self.kind
    }
}

impl fmt::Display for ParseIntError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.pad(match self.kind {
            IntErrorKind::Empty => "cannot parse integer from empty string",
            IntErrorKind::InvalidDigit => "invalid digit found in string",
            IntErrorKind::PosOverflow => "number too large to fit in target type",
            IntErrorKind::NegOverflow => "number too small to fit in target type",
            IntErrorKind::Zero => "number would be zero for non-zero type",
        })
    }
}

/// Parses an integer of `bits` bits into an i64/u64 pair.
pub fn parse_int(s: &str, radix: u32, signed: bool, min: i64, max: u64) -> Result<u64, ParseIntError> {
    let b = s.as_bytes();
    if b.is_empty() {
        return Err(ParseIntError { kind: IntErrorKind::Empty });
    }
    let mut i = 0;
    let mut neg = false;
    if b[0] == b'+' || b[0] == b'-' {
        if b.len() == 1 {
            return Err(ParseIntError {
                kind: IntErrorKind::InvalidDigit,
            });
        }
        neg = b[0] == b'-';
        if neg && !signed {
            return Err(ParseIntError {
                kind: IntErrorKind::InvalidDigit,
            });
        }
        i = 1;
    }
    let limit: u64 = if neg { (min as u64).wrapping_neg() } else { max };
    let mut v: u64 = 0;
    while i < b.len() {
        let d = match (b[i] as char).to_digit(radix) {
            Some(d) => d as u64,
            None => {
                return Err(ParseIntError {
                    kind: IntErrorKind::InvalidDigit,
                })
            }
        };
        let (m, o1) = v.overflowing_mul(radix as u64);
        let (a, o2) = m.overflowing_add(d);
        if o1 || o2 || a > limit {
            return Err(ParseIntError {
                kind: if neg { IntErrorKind::NegOverflow } else { IntErrorKind::PosOverflow },
            });
        }
        v = a;
        i += 1;
    }
    Ok(if neg { v.wrapping_neg() } else { v })
}

macro_rules! int_from_str {
    ($($t:ty, $signed:expr;)*) => {$(
        impl FromStr for $t {
            type Err = ParseIntError;
            fn from_str(s: &str) -> Result<$t, ParseIntError> {
                match parse_int(s, 10, $signed, <$t>::MIN as i64, <$t>::MAX as u64) {
                    Ok(v) => Ok(v as $t),
                    Err(e) => Err(e),
                }
            }
        }
    )*};
}
int_from_str! {
    i8, true; i16, true; i32, true; i64, true; isize, true;
    u8, false; u16, false; u32, false; u64, false; usize, false;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseFloatError {
    kind: FloatErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FloatErrorKind {
    Empty,
    Invalid,
}

impl fmt::Display for ParseFloatError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.pad(match self.kind {
            FloatErrorKind::Empty => "cannot parse float from empty string",
            FloatErrorKind::Invalid => "invalid float literal",
        })
    }
}

impl FromStr for f64 {
    type Err = ParseFloatError;
    fn from_str(s: &str) -> Result<f64, ParseFloatError> {
        if s.is_empty() {
            return Err(ParseFloatError { kind: FloatErrorKind::Empty });
        }
        let mut v: f64 = 0.0;
        let ok = unsafe { crate::sys::hrc_parse_float(s.as_ptr(), s.len(), 0, &mut v as *mut f64) };
        if ok != 0 {
            Ok(v)
        } else {
            Err(ParseFloatError {
                kind: FloatErrorKind::Invalid,
            })
        }
    }
}

impl FromStr for f32 {
    type Err = ParseFloatError;
    fn from_str(s: &str) -> Result<f32, ParseFloatError> {
        if s.is_empty() {
            return Err(ParseFloatError { kind: FloatErrorKind::Empty });
        }
        let mut v: f64 = 0.0;
        let ok = unsafe { crate::sys::hrc_parse_float(s.as_ptr(), s.len(), 1, &mut v as *mut f64) };
        if ok != 0 {
            Ok(v as f32)
        } else {
            Err(ParseFloatError {
                kind: FloatErrorKind::Invalid,
            })
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseBoolError;

impl fmt::Display for ParseBoolError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.pad("provided string was not `true` or `false`")
    }
}

impl FromStr for bool {
    type Err = ParseBoolError;
    fn from_str(s: &str) -> Result<bool, ParseBoolError> {
        match s {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(ParseBoolError),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseCharError {
    empty: bool,
}

impl fmt::Display for ParseCharError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.pad(if self.empty { "cannot parse char from empty string" } else { "too many characters in string" })
    }
}

impl FromStr for char {
    type Err = ParseCharError;
    fn from_str(s: &str) -> Result<char, ParseCharError> {
        let mut it = s.chars();
        match (it.next(), it.next()) {
            (None, _) => Err(ParseCharError { empty: true }),
            (Some(c), None) => Ok(c),
            _ => Err(ParseCharError { empty: false }),
        }
    }
}

impl FromStr for String {
    type Err = crate::convert::Infallible;
    fn from_str(s: &str) -> Result<String, crate::convert::Infallible> {
        Ok(String::from(s))
    }
}

impl Default for &str {
    fn default() -> &'static str {
        ""
    }
}

impl AsRef<str> for str {
    fn as_ref(&self) -> &str {
        self
    }
}

impl AsRef<[u8]> for str {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl crate::ops::Index<crate::ops::RangeFull> for str {
    type Output = str;
    fn index(&self, _r: crate::ops::RangeFull) -> &str {
        self
    }
}
