//! Tokenizer for Rust source text.

use crate::{Error, Span};
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// Identifiers and keywords (`r#name` is unescaped).
    Ident(Rc<str>),
    /// `'a` (without the quote).
    Lifetime(Rc<str>),
    Int(u128, Option<Rc<str>>),
    /// The literal's text without underscores or suffix.
    Float(Rc<str>, Option<Rc<str>>),
    Str(Rc<str>),
    ByteStr(Rc<[u8]>),
    Char(char),
    Byte(u8),
    Punct(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
    /// No whitespace between this token and the previous one (`a.0.1`
    /// and joint punctuation such as `>>` split in generics).
    pub joint: bool,
}

const PUNCTS: &[&str] = &[
    "<<=", ">>=", "...", "..=", "::", "->", "=>", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=",
    "*=", "/=", "%=", "^=", "&=", "|=", "<<", ">>", "..", "+", "-", "*", "/", "%", "^", "!", "&",
    "|", "=", "<", ">", "@", ".", ",", ";", ":", "#", "$", "?", "~", "{", "}", "[", "]", "(", ")",
];

/// Single-character punctuation by spelling, for splitting `>>` and friends.
pub fn punct(s: &str) -> Option<&'static str> {
    PUNCTS.iter().copied().find(|p| *p == s)
}

struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
    line_start: usize,
    file: u16,
}

impl<'a> Lexer<'a> {
    fn peek(&self, k: usize) -> u8 {
        self.src.get(self.pos + k).copied().unwrap_or(0)
    }

    fn span(&self) -> Span {
        Span {
            file: self.file,
            line: self.line,
            col: (self.pos - self.line_start + 1) as u32,
        }
    }

    fn err(&self, span: Span, msg: &str) -> Error {
        Error::new(span, String::from(msg))
    }

    fn bump(&mut self) -> u8 {
        let c = self.peek(0);
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
            self.line_start = self.pos;
        }
        c
    }

    /// Skips whitespace and comments; returns whether anything was skipped.
    fn skip_trivia(&mut self) -> Result<bool, Error> {
        let start = self.pos;
        loop {
            let c = self.peek(0);
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                self.bump();
            } else if c == b'/' && self.peek(1) == b'/' {
                while self.peek(0) != b'\n' && self.pos < self.src.len() {
                    self.bump();
                }
            } else if c == b'/' && self.peek(1) == b'*' {
                let span = self.span();
                self.bump();
                self.bump();
                let mut depth = 1;
                while depth > 0 {
                    if self.pos >= self.src.len() {
                        return Err(self.err(span, "unterminated block comment"));
                    }
                    if self.peek(0) == b'/' && self.peek(1) == b'*' {
                        self.bump();
                        self.bump();
                        depth += 1;
                    } else if self.peek(0) == b'*' && self.peek(1) == b'/' {
                        self.bump();
                        self.bump();
                        depth -= 1;
                    } else {
                        self.bump();
                    }
                }
            } else {
                return Ok(self.pos != start);
            }
        }
    }

    /// Decodes one UTF-8 character at the current position.
    fn next_char(&mut self) -> char {
        let s = &self.src[self.pos..];
        let len = match s[0] {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            _ => 4,
        }
        .min(s.len());
        let c = core::str::from_utf8(&s[..len])
            .ok()
            .and_then(|t| t.chars().next())
            .unwrap_or('\u{FFFD}');
        for _ in 0..len {
            self.bump();
        }
        c
    }

    fn escape(&mut self, byte: bool) -> Result<char, Error> {
        let span = self.span();
        self.bump(); // backslash
        let c = self.bump();
        Ok(match c {
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'\\' => '\\',
            b'0' => '\0',
            b'\'' => '\'',
            b'"' => '"',
            b'x' => {
                let h = [self.bump(), self.bump()];
                let v = core::str::from_utf8(&h)
                    .ok()
                    .and_then(|s| u8::from_str_radix(s, 16).ok())
                    .ok_or_else(|| self.err(span, "invalid \\x escape"))?;
                if !byte && v > 0x7F {
                    return Err(self.err(span, "out of range hex escape"));
                }
                v as char
            }
            b'u' if !byte => {
                if self.bump() != b'{' {
                    return Err(self.err(span, "invalid unicode escape"));
                }
                let mut v: u32 = 0;
                loop {
                    let d = self.bump();
                    if d == b'}' {
                        break;
                    }
                    if d == b'_' {
                        continue;
                    }
                    let digit = (d as char)
                        .to_digit(16)
                        .ok_or_else(|| self.err(span, "invalid unicode escape"))?;
                    v = v.wrapping_mul(16) + digit;
                }
                char::from_u32(v).ok_or_else(|| self.err(span, "invalid unicode character escape"))?
            }
            _ => return Err(self.err(span, "unknown character escape")),
        })
    }

    /// Body of a (non-raw) string literal after the opening quote.
    fn string(&mut self, byte: bool, span: Span) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        loop {
            if self.pos >= self.src.len() {
                return Err(self.err(span, "unterminated double quote string"));
            }
            match self.peek(0) {
                b'"' => {
                    self.bump();
                    return Ok(out);
                }
                b'\\' if self.peek(1) == b'\n' || (self.peek(1) == b'\r' && self.peek(2) == b'\n') => {
                    self.bump();
                    while matches!(self.peek(0), b' ' | b'\t' | b'\n' | b'\r') {
                        self.bump();
                    }
                }
                b'\\' => {
                    let c = self.escape(byte)?;
                    if byte {
                        out.push(c as u32 as u8);
                    } else {
                        let mut buf = [0; 4];
                        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    }
                }
                b'\r' if self.peek(1) == b'\n' => {
                    self.bump();
                }
                _ => {
                    let c = self.bump();
                    out.push(c);
                }
            }
        }
    }

    /// `r"..."` / `r#"..."#` after the `r` (or `br`).
    fn raw_string(&mut self, span: Span) -> Result<Vec<u8>, Error> {
        let mut hashes = 0;
        while self.peek(0) == b'#' {
            self.bump();
            hashes += 1;
        }
        if self.bump() != b'"' {
            return Err(self.err(span, "expected '\"' in raw string"));
        }
        let mut out = Vec::new();
        loop {
            if self.pos >= self.src.len() {
                return Err(self.err(span, "unterminated raw string"));
            }
            if self.peek(0) == b'"' && (1..=hashes).all(|k| self.peek(k) == b'#') {
                self.bump();
                for _ in 0..hashes {
                    self.bump();
                }
                return Ok(out);
            }
            let c = self.bump();
            if c != b'\r' {
                out.push(c);
            }
        }
    }

    fn suffix(&mut self) -> Option<Rc<str>> {
        if is_ident_start(self.peek(0)) {
            let start = self.pos;
            while is_ident_continue(self.peek(0)) {
                self.bump();
            }
            Some(Rc::from(core::str::from_utf8(&self.src[start..self.pos]).unwrap()))
        } else {
            None
        }
    }

    fn number(&mut self, span: Span) -> Result<Tok, Error> {
        let radix = if self.peek(0) == b'0' {
            match self.peek(1) {
                b'x' => 16,
                b'o' => 8,
                b'b' => 2,
                _ => 10,
            }
        } else {
            10
        };
        if radix != 10 {
            self.bump();
            self.bump();
            let mut v: u128 = 0;
            let mut any = false;
            loop {
                let c = self.peek(0);
                if c == b'_' {
                    self.bump();
                    continue;
                }
                // `0b1i32`: the suffix starts with a letter that is not a digit.
                let d = match (c as char).to_digit(radix) {
                    Some(d) if !(radix == 16 && (c == b'i' || c == b'u')) => d,
                    _ => break,
                };
                v = v
                    .checked_mul(radix as u128)
                    .and_then(|v| v.checked_add(d as u128))
                    .ok_or_else(|| self.err(span, "integer literal is too large"))?;
                any = true;
                self.bump();
            }
            if !any {
                return Err(self.err(span, "no valid digits found for number"));
            }
            return Ok(Tok::Int(v, self.suffix()));
        }
        let mut text = String::new();
        let mut float = false;
        while self.peek(0).is_ascii_digit() || self.peek(0) == b'_' {
            let c = self.bump();
            if c != b'_' {
                text.push(c as char);
            }
        }
        // `1.0` and `1.` are floats; `1..2`, `1.foo()` and `t.0.1` are not.
        if self.peek(0) == b'.' && self.peek(1) != b'.' && !is_ident_start(self.peek(1)) {
            float = true;
            self.bump();
            text.push('.');
            while self.peek(0).is_ascii_digit() || self.peek(0) == b'_' {
                let c = self.bump();
                if c != b'_' {
                    text.push(c as char);
                }
            }
        }
        if (self.peek(0) == b'e' || self.peek(0) == b'E')
            && (self.peek(1).is_ascii_digit()
                || ((self.peek(1) == b'+' || self.peek(1) == b'-') && self.peek(2).is_ascii_digit()))
        {
            float = true;
            text.push(self.bump() as char);
            if self.peek(0) == b'+' || self.peek(0) == b'-' {
                text.push(self.bump() as char);
            }
            while self.peek(0).is_ascii_digit() || self.peek(0) == b'_' {
                let c = self.bump();
                if c != b'_' {
                    text.push(c as char);
                }
            }
        }
        let suffix = self.suffix();
        let float_suffix = matches!(suffix.as_deref(), Some("f32") | Some("f64"));
        if float || float_suffix {
            if text.ends_with('.') {
                text.push('0');
            }
            return Ok(Tok::Float(Rc::from(text.as_str()), suffix));
        }
        let mut v: u128 = 0;
        for c in text.bytes() {
            v = v
                .checked_mul(10)
                .and_then(|v| v.checked_add((c - b'0') as u128))
                .ok_or_else(|| self.err(span, "integer literal is too large"))?;
        }
        Ok(Tok::Int(v, suffix))
    }

    fn token(&mut self) -> Result<Tok, Error> {
        let span = self.span();
        let c = self.peek(0);
        if c == b'r' && self.peek(1) == b'#' && is_ident_start(self.peek(2)) {
            self.bump();
            self.bump();
            let start = self.pos;
            while is_ident_continue(self.peek(0)) {
                self.bump();
            }
            return Ok(Tok::Ident(Rc::from(core::str::from_utf8(&self.src[start..self.pos]).unwrap())));
        }
        if c == b'r' && (self.peek(1) == b'"' || (self.peek(1) == b'#' && matches!(self.peek(2), b'#' | b'"'))) {
            self.bump();
            let s = self.raw_string(span)?;
            let s = String::from_utf8(s).map_err(|_| self.err(span, "invalid UTF-8"))?;
            return Ok(Tok::Str(Rc::from(s.as_str())));
        }
        if c == b'b' && self.peek(1) == b'r' && matches!(self.peek(2), b'"' | b'#') {
            self.bump();
            self.bump();
            return Ok(Tok::ByteStr(Rc::from(self.raw_string(span)?)));
        }
        if c == b'b' && self.peek(1) == b'"' {
            self.bump();
            self.bump();
            return Ok(Tok::ByteStr(Rc::from(self.string(true, span)?)));
        }
        if c == b'b' && self.peek(1) == b'\'' {
            self.bump();
            self.bump();
            let v = if self.peek(0) == b'\\' {
                self.escape(true)? as u32 as u8
            } else {
                self.bump()
            };
            if self.bump() != b'\'' {
                return Err(self.err(span, "unterminated byte constant"));
            }
            return Ok(Tok::Byte(v));
        }
        if is_ident_start(c) || c >= 0x80 {
            let start = self.pos;
            while is_ident_continue(self.peek(0)) || (self.peek(0) >= 0x80 && self.pos < self.src.len()) {
                self.bump();
            }
            let s = core::str::from_utf8(&self.src[start..self.pos]).map_err(|_| self.err(span, "invalid UTF-8"))?;
            return Ok(Tok::Ident(Rc::from(s)));
        }
        if c.is_ascii_digit() {
            return self.number(span);
        }
        if c == b'"' {
            self.bump();
            let s = self.string(false, span)?;
            let s = String::from_utf8(s).map_err(|_| self.err(span, "invalid UTF-8"))?;
            return Ok(Tok::Str(Rc::from(s.as_str())));
        }
        if c == b'\'' {
            // A character literal, or a lifetime / loop label.
            self.bump();
            if self.peek(0) == b'\\' {
                let ch = self.escape(false)?;
                if self.bump() != b'\'' {
                    return Err(self.err(span, "unterminated character literal"));
                }
                return Ok(Tok::Char(ch));
            }
            let save = (self.pos, self.line, self.line_start);
            let ch = self.next_char();
            if self.peek(0) == b'\'' {
                self.bump();
                return Ok(Tok::Char(ch));
            }
            (self.pos, self.line, self.line_start) = save;
            if !is_ident_start(self.peek(0)) {
                return Err(self.err(span, "unterminated character literal"));
            }
            let start = self.pos;
            while is_ident_continue(self.peek(0)) {
                self.bump();
            }
            return Ok(Tok::Lifetime(Rc::from(core::str::from_utf8(&self.src[start..self.pos]).unwrap())));
        }
        for p in PUNCTS {
            if self.src[self.pos..].starts_with(p.as_bytes()) {
                for _ in 0..p.len() {
                    self.bump();
                }
                return Ok(Tok::Punct(p));
            }
        }
        Err(self.err(span, &alloc::format!("unknown start of token: {}", c as char)))
    }
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_continue(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

pub fn tokenize(src: &str, file: u16) -> Result<Vec<Token>, Error> {
    let mut lx = Lexer {
        src: src.as_bytes(),
        pos: 0,
        line: 1,
        line_start: 0,
        file,
    };
    // A `#!` line that is not an attribute (`#![...]`).
    if src.starts_with("#!") && !src.starts_with("#![") {
        while lx.peek(0) != b'\n' && lx.pos < lx.src.len() {
            lx.bump();
        }
    }
    let mut out = Vec::new();
    loop {
        let space = lx.skip_trivia()?;
        let span = lx.span();
        if lx.pos >= lx.src.len() {
            out.push(Token {
                tok: Tok::Eof,
                span,
                joint: false,
            });
            return Ok(out);
        }
        let tok = lx.token()?;
        out.push(Token {
            tok,
            span,
            joint: !space,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        tokenize(s, 0).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn numbers() {
        assert_eq!(toks("1_000u8")[0], Tok::Int(1000, Some(Rc::from("u8"))));
        assert_eq!(toks("0xff")[0], Tok::Int(255, None));
        assert_eq!(toks("1.5e3")[0], Tok::Float(Rc::from("1.5e3"), None));
        assert_eq!(toks("2f64")[0], Tok::Float(Rc::from("2"), Some(Rc::from("f64"))));
        assert_eq!(toks("1..2")[1], Tok::Punct(".."));
        assert_eq!(toks("1.max(2)")[1], Tok::Punct("."));
    }

    #[test]
    fn chars_and_lifetimes() {
        assert_eq!(toks("'a'")[0], Tok::Char('a'));
        assert_eq!(toks("'\\n'")[0], Tok::Char('\n'));
        assert_eq!(toks("'a")[0], Tok::Lifetime(Rc::from("a")));
        assert_eq!(toks("'ж'")[0], Tok::Char('ж'));
        assert_eq!(toks("b'x'")[0], Tok::Byte(b'x'));
    }

    #[test]
    fn strings() {
        assert_eq!(toks(r#""a\tb""#)[0], Tok::Str(Rc::from("a\tb")));
        assert_eq!(toks(r###"r#"a"b"#"###)[0], Tok::Str(Rc::from("a\"b")));
        assert_eq!(toks("\"a\\\n    b\"")[0], Tok::Str(Rc::from("ab")));
        assert_eq!(toks("\"\\u{1F600}\"")[0], Tok::Str(Rc::from("\u{1F600}")));
    }

    #[test]
    fn comments() {
        assert_eq!(toks("/* a /* b */ c */ x // y")[0], Tok::Ident(Rc::from("x")));
    }
}
