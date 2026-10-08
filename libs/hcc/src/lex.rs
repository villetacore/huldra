//! Tokenizer for C source text.

use crate::Error;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(Rc<str>),
    /// Numeric literal, unparsed (the preprocessor and parser decide).
    Num(Rc<str>),
    /// Character constant value.
    Char(i64),
    /// String literal contents (escapes already decoded).
    Str(Rc<[u8]>),
    Punct(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub file: u16,
    pub line: u32,
    /// First token on its line (directives start with such a `#`).
    pub bol: bool,
    /// Preceded by whitespace (matters for `#` stringification).
    pub space: bool,
    /// Macros that may not be expanded again inside this token's expansion.
    pub hideset: Rc<[Rc<str>]>,
}

impl Token {
    pub fn is(&self, p: &str) -> bool {
        matches!(&self.tok, Tok::Punct(q) if *q == p)
    }

    pub fn ident(&self) -> Option<&str> {
        match &self.tok {
            Tok::Ident(s) => Some(s),
            _ => None,
        }
    }

    /// Source spelling (used for stringification and token pasting).
    pub fn spelling(&self) -> String {
        match &self.tok {
            Tok::Ident(s) | Tok::Num(s) => String::from(&**s),
            Tok::Punct(p) => String::from(*p),
            Tok::Char(c) => {
                let mut s = String::from("'");
                push_escaped(&mut s, *c as u8);
                s.push('\'');
                s
            }
            Tok::Str(b) => {
                let mut s = String::from("\"");
                for &c in b.iter() {
                    push_escaped(&mut s, c);
                }
                s.push('"');
                s
            }
            Tok::Eof => String::new(),
        }
    }
}

fn push_escaped(s: &mut String, c: u8) {
    match c {
        b'\n' => s.push_str("\\n"),
        b'\t' => s.push_str("\\t"),
        b'\\' => s.push_str("\\\\"),
        b'"' => s.push_str("\\\""),
        b'\'' => s.push_str("\\'"),
        0 => s.push_str("\\0"),
        32..=126 => s.push(c as char),
        _ => s.push_str(&alloc::format!("\\x{:02x}", c)),
    }
}

const PUNCTS: &[&str] = &[
    "<<=", ">>=", "...", "->", "++", "--", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "+=",
    "-=", "*=", "/=", "%=", "&=", "|=", "^=", "##", "[", "]", "(", ")", "{", "}", ".", "&", "*",
    "+", "-", "~", "!", "/", "%", "<", ">", "^", "|", "?", ":", ";", "=", ",", "#",
];

fn escape(b: &[u8], i: &mut usize) -> Result<u8, String> {
    // b[*i] is the character after the backslash.
    let c = b[*i];
    *i += 1;
    Ok(match c {
        b'n' => b'\n',
        b't' => b'\t',
        b'r' => b'\r',
        b'0'..=b'7' => {
            let mut v = (c - b'0') as u32;
            for _ in 0..2 {
                if *i < b.len() && (b'0'..=b'7').contains(&b[*i]) {
                    v = v * 8 + (b[*i] - b'0') as u32;
                    *i += 1;
                }
            }
            v as u8
        }
        b'x' => {
            let mut v = 0u32;
            let start = *i;
            while *i < b.len() && b[*i].is_ascii_hexdigit() {
                v = v * 16 + (b[*i] as char).to_digit(16).unwrap();
                *i += 1;
            }
            if *i == start {
                return Err(String::from("\\x used with no following hex digits"));
            }
            v as u8
        }
        b'a' => 7,
        b'b' => 8,
        b'f' => 12,
        b'v' => 11,
        b'e' => 27,
        other => other, // \\ \' \" \?
    })
}

/// Splits `src` into tokens. Comments are removed and line continuations
/// joined; the result always ends with an `Eof` token.
pub fn tokenize(src: &str, file: u16, file_name: &str) -> Result<Vec<Token>, Error> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    let mut bol = true;
    let mut space = false;
    let empty: Rc<[Rc<str>]> = Rc::from(Vec::new());
    let err = |line: u32, msg: String| Error {
        file: String::from(file_name),
        line,
        message: msg,
    };
    while i < b.len() {
        let c = b[i];
        if c == b'\\' && b.get(i + 1) == Some(&b'\n') {
            i += 2;
            line += 1;
            continue;
        }
        if c == b'\\' && b.get(i + 1) == Some(&b'\r') && b.get(i + 2) == Some(&b'\n') {
            i += 3;
            line += 1;
            continue;
        }
        if c == b'\n' {
            i += 1;
            line += 1;
            bol = true;
            space = false;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            space = true;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            space = true;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            loop {
                if i + 1 >= b.len() {
                    return Err(err(line, String::from("unterminated comment")));
                }
                if b[i] == b'*' && b[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                if b[i] == b'\n' {
                    line += 1;
                }
                i += 1;
            }
            space = true;
            continue;
        }
        let tok_line = line;
        let tok = if c.is_ascii_digit()
            || (c == b'.' && b.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            let start = i;
            loop {
                let ch = *b.get(i).unwrap_or(&0);
                let prev = if i > start { b[i - 1] } else { 0 };
                if ch.is_ascii_alphanumeric()
                    || ch == b'.'
                    || ch == b'_'
                    || ((ch == b'+' || ch == b'-') && matches!(prev, b'e' | b'E' | b'p' | b'P'))
                {
                    i += 1;
                } else {
                    break;
                }
            }
            Tok::Num(Rc::from(&src[start..i]))
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            Tok::Ident(Rc::from(&src[start..i]))
        } else if c == b'"' {
            i += 1;
            let mut s = Vec::new();
            loop {
                match b.get(i) {
                    None | Some(b'\n') => {
                        return Err(err(line, String::from("unterminated string literal")))
                    }
                    Some(b'"') => {
                        i += 1;
                        break;
                    }
                    Some(b'\\') => {
                        i += 1;
                        s.push(escape(b, &mut i).map_err(|m| err(line, m))?);
                    }
                    Some(&ch) => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            Tok::Str(Rc::from(s))
        } else if c == b'\'' {
            i += 1;
            let v = match b.get(i) {
                Some(b'\\') => {
                    i += 1;
                    escape(b, &mut i).map_err(|m| err(line, m))? as i8 as i64
                }
                Some(&ch) => {
                    i += 1;
                    ch as i8 as i64
                }
                None => return Err(err(line, String::from("unterminated character constant"))),
            };
            if b.get(i) != Some(&b'\'') {
                return Err(err(line, String::from("unterminated character constant")));
            }
            i += 1;
            Tok::Char(v)
        } else {
            let rest = &src[i..];
            let p = PUNCTS
                .iter()
                .find(|p| rest.starts_with(**p))
                .ok_or_else(|| err(line, alloc::format!("stray '{}' in program", c as char)))?;
            i += p.len();
            Tok::Punct(p)
        };
        out.push(Token {
            tok,
            file,
            line: tok_line,
            bol,
            space,
            hideset: empty.clone(),
        });
        bol = false;
        space = false;
    }
    out.push(Token {
        tok: Tok::Eof,
        file,
        line,
        bol: true,
        space: false,
        hideset: empty,
    });
    Ok(out)
}
