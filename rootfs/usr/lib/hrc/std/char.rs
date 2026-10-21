//! `char`: Unicode scalar values.

use crate::option::Option::{self, None, Some};

pub const MAX: char = '\u{10FFFF}';
pub const REPLACEMENT_CHARACTER: char = '\u{FFFD}';

extern "rust-intrinsic" {
    fn char_from_u32_unchecked(v: u32) -> char;
}

pub unsafe fn from_u32_unchecked(v: u32) -> char {
    char_from_u32_unchecked(v)
}

pub fn from_u32(v: u32) -> Option<char> {
    if v > 0x10FFFF || (v >= 0xD800 && v <= 0xDFFF) {
        None
    } else {
        Some(unsafe { from_u32_unchecked(v) })
    }
}

pub fn from_digit(num: u32, radix: u32) -> Option<char> {
    if radix < 2 || radix > 36 {
        panic!("from_digit: radix is too high (maximum 36)");
    }
    if num < radix {
        let b = if num < 10 { b'0' + num as u8 } else { b'a' + (num - 10) as u8 };
        Some(b as char)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharTryFromError(pub(crate) ());

impl crate::fmt::Display for CharTryFromError {
    fn fmt(&self, f: &mut crate::fmt::Formatter) -> crate::fmt::Result {
        f.pad("converted integer out of range for `char`")
    }
}

/// An iterator over the characters of a case mapping.
#[derive(Clone)]
pub struct CaseMapping {
    chars: [char; 3],
    i: usize,
    n: usize,
}

impl Iterator for CaseMapping {
    type Item = char;
    fn next(&mut self) -> Option<char> {
        if self.i < self.n {
            let c = self.chars[self.i];
            self.i += 1;
            Some(c)
        } else {
            None
        }
    }
}

impl crate::fmt::Display for CaseMapping {
    fn fmt(&self, f: &mut crate::fmt::Formatter) -> crate::fmt::Result {
        let mut i = self.i;
        while i < self.n {
            f.write_char(self.chars[i])?;
            i += 1;
        }
        Ok(())
    }
}

fn one(c: char) -> CaseMapping {
    CaseMapping {
        chars: [c, '\0', '\0'],
        i: 0,
        n: 1,
    }
}

/// Simple case mappings for Latin, Greek and Cyrillic letters.
fn upper(c: char) -> CaseMapping {
    let v = c as u32;
    let u = if (v >= 0x61 && v <= 0x7A) || (v >= 0xE0 && v <= 0xFE && v != 0xF7) {
        v - 32
    } else if v == 0xFF {
        0x178
    } else if v >= 0x100 && v <= 0x17F && v != 0x130 && v != 0x131 && v != 0x138 && v != 0x149 && v != 0x17F {
        if (v >= 0x139 && v <= 0x148) || (v >= 0x179 && v <= 0x17E) {
            if v % 2 == 0 { v - 1 } else { v }
        } else if v % 2 == 1 {
            v - 1
        } else {
            v
        }
    } else if v >= 0x3B1 && v <= 0x3C9 && v != 0x3C2 {
        v - 32
    } else if v == 0x3C2 {
        0x3A3
    } else if v >= 0x430 && v <= 0x44F {
        v - 32
    } else if v >= 0x450 && v <= 0x45F {
        v - 80
    } else if v == 0xDF {
        return CaseMapping {
            chars: ['S', 'S', '\0'],
            i: 0,
            n: 2,
        };
    } else {
        v
    };
    one(from_u32(u).unwrap_or(c))
}

fn lower(c: char) -> CaseMapping {
    let v = c as u32;
    let l = if (v >= 0x41 && v <= 0x5A) || (v >= 0xC0 && v <= 0xDE && v != 0xD7) {
        v + 32
    } else if v == 0x178 {
        0xFF
    } else if v >= 0x100 && v <= 0x17F && v != 0x130 && v != 0x131 && v != 0x138 && v != 0x149 && v != 0x17F {
        if (v >= 0x139 && v <= 0x148) || (v >= 0x179 && v <= 0x17E) {
            if v % 2 == 1 { v + 1 } else { v }
        } else if v % 2 == 0 {
            v + 1
        } else {
            v
        }
    } else if v == 0x130 {
        0x69
    } else if v >= 0x391 && v <= 0x3A9 && v != 0x3A2 {
        v + 32
    } else if v >= 0x410 && v <= 0x42F {
        v + 32
    } else if v >= 0x400 && v <= 0x40F {
        v + 80
    } else {
        v
    };
    one(from_u32(l).unwrap_or(c))
}

impl char {
    pub const MAX: char = '\u{10FFFF}';
    pub const REPLACEMENT_CHARACTER: char = '\u{FFFD}';

    pub fn from_u32(v: u32) -> Option<char> {
        from_u32(v)
    }
    pub fn from_digit(num: u32, radix: u32) -> Option<char> {
        from_digit(num, radix)
    }
    pub fn len_utf8(self) -> usize {
        let v = self as u32;
        if v < 0x80 {
            1
        } else if v < 0x800 {
            2
        } else if v < 0x10000 {
            3
        } else {
            4
        }
    }
    pub fn len_utf16(self) -> usize {
        if (self as u32) < 0x10000 { 1 } else { 2 }
    }
    pub fn encode_utf8(self, dst: &mut [u8]) -> &mut str {
        let v = self as u32;
        let n = self.len_utf8();
        if dst.len() < n {
            panic!("encode_utf8: need {} bytes to encode U+{:X}, but the buffer has {}", n, v, dst.len());
        }
        if n == 1 {
            dst[0] = v as u8;
        } else if n == 2 {
            dst[0] = (0xC0 | (v >> 6)) as u8;
            dst[1] = (0x80 | (v & 0x3F)) as u8;
        } else if n == 3 {
            dst[0] = (0xE0 | (v >> 12)) as u8;
            dst[1] = (0x80 | ((v >> 6) & 0x3F)) as u8;
            dst[2] = (0x80 | (v & 0x3F)) as u8;
        } else {
            dst[0] = (0xF0 | (v >> 18)) as u8;
            dst[1] = (0x80 | ((v >> 12) & 0x3F)) as u8;
            dst[2] = (0x80 | ((v >> 6) & 0x3F)) as u8;
            dst[3] = (0x80 | (v & 0x3F)) as u8;
        }
        unsafe { crate::str::from_utf8_unchecked_mut(&mut dst[..n]) }
    }
    pub fn encode_utf16(self, dst: &mut [u16]) -> &mut [u16] {
        let v = self as u32;
        if v < 0x10000 {
            dst[0] = v as u16;
            &mut dst[..1]
        } else {
            let w = v - 0x10000;
            dst[0] = (0xD800 | (w >> 10)) as u16;
            dst[1] = (0xDC00 | (w & 0x3FF)) as u16;
            &mut dst[..2]
        }
    }
    pub fn to_digit(self, radix: u32) -> Option<u32> {
        let v = self as u32;
        let d = if v >= 0x30 && v <= 0x39 {
            v - 0x30
        } else if v >= 0x61 && v <= 0x7A {
            v - 0x61 + 10
        } else if v >= 0x41 && v <= 0x5A {
            v - 0x41 + 10
        } else {
            return None;
        };
        if d < radix {
            Some(d)
        } else {
            None
        }
    }
    pub fn is_digit(self, radix: u32) -> bool {
        self.to_digit(radix).is_some()
    }
    pub fn is_ascii(&self) -> bool {
        (*self as u32) < 0x80
    }
    pub fn is_ascii_alphabetic(&self) -> bool {
        let c = *self;
        (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
    }
    pub fn is_ascii_uppercase(&self) -> bool {
        *self >= 'A' && *self <= 'Z'
    }
    pub fn is_ascii_lowercase(&self) -> bool {
        *self >= 'a' && *self <= 'z'
    }
    pub fn is_ascii_digit(&self) -> bool {
        *self >= '0' && *self <= '9'
    }
    pub fn is_ascii_hexdigit(&self) -> bool {
        self.is_ascii_digit() || (*self >= 'a' && *self <= 'f') || (*self >= 'A' && *self <= 'F')
    }
    pub fn is_ascii_alphanumeric(&self) -> bool {
        self.is_ascii_alphabetic() || self.is_ascii_digit()
    }
    pub fn is_ascii_whitespace(&self) -> bool {
        matches!(*self, ' ' | '\t' | '\n' | '\r' | '\u{0C}')
    }
    pub fn is_ascii_punctuation(&self) -> bool {
        let v = *self as u32;
        (v >= 33 && v <= 47) || (v >= 58 && v <= 64) || (v >= 91 && v <= 96) || (v >= 123 && v <= 126)
    }
    pub fn is_ascii_graphic(&self) -> bool {
        let v = *self as u32;
        v >= 33 && v <= 126
    }
    pub fn is_ascii_control(&self) -> bool {
        let v = *self as u32;
        v < 32 || v == 127
    }
    pub fn to_ascii_uppercase(&self) -> char {
        if self.is_ascii_lowercase() {
            ((*self as u8) - 32) as char
        } else {
            *self
        }
    }
    pub fn to_ascii_lowercase(&self) -> char {
        if self.is_ascii_uppercase() {
            ((*self as u8) + 32) as char
        } else {
            *self
        }
    }
    pub fn make_ascii_uppercase(&mut self) {
        *self = self.to_ascii_uppercase();
    }
    pub fn make_ascii_lowercase(&mut self) {
        *self = self.to_ascii_lowercase();
    }
    pub fn eq_ignore_ascii_case(&self, other: &char) -> bool {
        self.to_ascii_lowercase() == other.to_ascii_lowercase()
    }
    pub fn is_alphabetic(self) -> bool {
        let v = self as u32;
        self.is_ascii_alphabetic()
            || (v >= 0xC0 && v <= 0x24F && v != 0xD7 && v != 0xF7)
            || (v >= 0x370 && v <= 0x3FF)
            || (v >= 0x400 && v <= 0x52F)
            || (v >= 0x4E00 && v <= 0x9FFF)
            || (v >= 0x3040 && v <= 0x30FF)
            || (v >= 0xAC00 && v <= 0xD7AF)
            || (v >= 0x5D0 && v <= 0x5EA)
            || (v >= 0x620 && v <= 0x64A)
    }
    pub fn is_lowercase(self) -> bool {
        let u = upper(self);
        self.is_alphabetic() && (u.chars[0] != self || u.n > 1)
    }
    pub fn is_uppercase(self) -> bool {
        let l = lower(self);
        self.is_alphabetic() && l.chars[0] != self
    }
    pub fn is_whitespace(self) -> bool {
        matches!(
            self as u32,
            0x09..=0x0D | 0x20 | 0x85 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000
        )
    }
    pub fn is_numeric(self) -> bool {
        let v = self as u32;
        self.is_ascii_digit() || v == 0xB2 || v == 0xB3 || v == 0xB9 || (v >= 0xBC && v <= 0xBE) || (v >= 0x660 && v <= 0x669)
    }
    pub fn is_alphanumeric(self) -> bool {
        self.is_alphabetic() || self.is_numeric()
    }
    pub fn is_control(self) -> bool {
        let v = self as u32;
        v < 0x20 || (v >= 0x7F && v < 0xA0)
    }
    pub fn to_uppercase(self) -> CaseMapping {
        upper(self)
    }
    pub fn to_lowercase(self) -> CaseMapping {
        lower(self)
    }
    pub fn escape_default(self) -> crate::string::String {
        let mut s = crate::string::String::new();
        match self {
            '\t' => s.push_str("\\t"),
            '\r' => s.push_str("\\r"),
            '\n' => s.push_str("\\n"),
            '\'' => s.push_str("\\'"),
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            c if (c as u32) >= 0x20 && (c as u32) < 0x7F => s.push(c),
            c => {
                s.push_str("\\u{");
                s.push_str(&crate::fmt::hex_digits(c as u64, false));
                s.push('}');
            }
        }
        s
    }
    pub fn escape_debug(self) -> crate::string::String {
        let f = format!("{:?}", self);
        crate::string::String::from(&f[1..f.len() - 1])
    }
}

impl Default for char {
    fn default() -> char {
        '\0'
    }
}
