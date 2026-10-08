//! Tokenizer. Words keep their quoting structure so expansion can happen
//! when the command runs (variables inside loops must see current values).

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    /// Unquoted text (subject to globbing).
    Lit(String),
    /// Quoted or escaped text (taken literally).
    Quoted(String),
    /// `$name`, `${name}`, `$?`, `$1`, ...; `quoted` disables field splitting.
    Var { name: String, quoted: bool },
    /// `$(...)` or `` `...` ``.
    Command { source: String, quoted: bool },
    /// `$((...))`
    Arith(String),
}

pub type Word = Vec<Part>;

#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Word(Word),
    /// `|`, `||`, `&`, `&&`, `;`, `(`, `)`, `<`, `>`, `>>`, `2>`, `2>>`, `2>&1`, `>&2`, `!`
    Op(&'static str),
    Newline,
}

#[derive(Debug, PartialEq)]
pub enum LexError {
    /// Input ended inside a quote or `$(`: more lines are needed.
    Incomplete,
    Bad(String),
}

impl Token {
    /// The word as plain text if it has no quoting or expansions.
    pub fn plain(&self) -> Option<&str> {
        match self {
            Token::Word(w) if w.len() == 1 => match &w[0] {
                Part::Lit(s) => Some(s),
                _ => None,
            },
            _ => None,
        }
    }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

struct Lexer<'a> {
    chars: Vec<char>,
    pos: usize,
    _src: &'a str,
}

impl Lexer<'_> {
    fn peek(&self, off: usize) -> Option<char> {
        self.chars.get(self.pos + off).copied()
    }

    /// Reads `$...` after the `$` has been consumed.
    fn dollar(&mut self, quoted: bool) -> Result<Part, LexError> {
        match self.peek(0) {
            Some('(') if self.peek(1) == Some('(') => {
                self.pos += 2;
                let start = self.pos;
                let mut depth = 0;
                while let Some(c) = self.peek(0) {
                    match c {
                        '(' => depth += 1,
                        ')' if depth > 0 => depth -= 1,
                        ')' if self.peek(1) == Some(')') => {
                            let expr: String = self.chars[start..self.pos].iter().collect();
                            self.pos += 2;
                            return Ok(Part::Arith(expr));
                        }
                        _ => {}
                    }
                    self.pos += 1;
                }
                Err(LexError::Incomplete)
            }
            Some('(') => {
                self.pos += 1;
                let start = self.pos;
                let mut depth = 1;
                let mut in_single = false;
                while let Some(c) = self.peek(0) {
                    self.pos += 1;
                    match c {
                        '\'' => in_single = !in_single,
                        '(' if !in_single => depth += 1,
                        ')' if !in_single => {
                            depth -= 1;
                            if depth == 0 {
                                let source: String = self.chars[start..self.pos - 1].iter().collect();
                                return Ok(Part::Command { source, quoted });
                            }
                        }
                        _ => {}
                    }
                }
                Err(LexError::Incomplete)
            }
            Some('{') => {
                self.pos += 1;
                let start = self.pos;
                while let Some(c) = self.peek(0) {
                    self.pos += 1;
                    if c == '}' {
                        let name: String = self.chars[start..self.pos - 1].iter().collect();
                        return Ok(Part::Var { name, quoted });
                    }
                }
                Err(LexError::Incomplete)
            }
            Some(c) if matches!(c, '?' | '$' | '#' | '@' | '*' | '!') || c.is_ascii_digit() => {
                self.pos += 1;
                Ok(Part::Var { name: c.into(), quoted })
            }
            Some(c) if is_name_char(c) => {
                let start = self.pos;
                while self.peek(0).is_some_and(is_name_char) {
                    self.pos += 1;
                }
                Ok(Part::Var { name: self.chars[start..self.pos].iter().collect(), quoted })
            }
            _ => Ok(if quoted { Part::Quoted("$".into()) } else { Part::Lit("$".into()) }),
        }
    }

    fn backtick(&mut self, quoted: bool) -> Result<Part, LexError> {
        let start = self.pos;
        while let Some(c) = self.peek(0) {
            self.pos += 1;
            if c == '`' {
                let source: String = self.chars[start..self.pos - 1].iter().collect();
                return Ok(Part::Command { source, quoted });
            }
        }
        Err(LexError::Incomplete)
    }

    fn word(&mut self) -> Result<Word, LexError> {
        let mut parts: Word = Vec::new();
        let mut lit = String::new();
        macro_rules! flush {
            () => {
                if !lit.is_empty() {
                    parts.push(Part::Lit(core::mem::take(&mut lit)));
                }
            };
        }
        while let Some(c) = self.peek(0) {
            match c {
                ' ' | '\t' | '\n' | ';' | '&' | '|' | '<' | '>' | '(' | ')' => break,
                '\\' => {
                    self.pos += 1;
                    match self.peek(0) {
                        Some('\n') => self.pos += 1, // line continuation
                        Some(n) => {
                            flush!();
                            parts.push(Part::Quoted(n.into()));
                            self.pos += 1;
                        }
                        None => return Err(LexError::Incomplete),
                    }
                }
                '\'' => {
                    flush!();
                    self.pos += 1;
                    let start = self.pos;
                    loop {
                        match self.peek(0) {
                            Some('\'') => break,
                            Some(_) => self.pos += 1,
                            None => return Err(LexError::Incomplete),
                        }
                    }
                    parts.push(Part::Quoted(self.chars[start..self.pos].iter().collect()));
                    self.pos += 1;
                }
                '"' => {
                    flush!();
                    self.pos += 1;
                    let mut text = String::new();
                    // An empty "" still produces an (empty) argument.
                    parts.push(Part::Quoted(String::new()));
                    loop {
                        match self.peek(0) {
                            None => return Err(LexError::Incomplete),
                            Some('"') => {
                                self.pos += 1;
                                break;
                            }
                            Some('\\') if matches!(self.peek(1), Some('"' | '\\' | '$' | '`' | '\n')) => {
                                if self.peek(1) != Some('\n') {
                                    text.push(self.peek(1).unwrap());
                                }
                                self.pos += 2;
                            }
                            Some('$') => {
                                self.pos += 1;
                                if !text.is_empty() {
                                    parts.push(Part::Quoted(core::mem::take(&mut text)));
                                }
                                parts.push(self.dollar(true)?);
                            }
                            Some('`') => {
                                self.pos += 1;
                                if !text.is_empty() {
                                    parts.push(Part::Quoted(core::mem::take(&mut text)));
                                }
                                parts.push(self.backtick(true)?);
                            }
                            Some(ch) => {
                                text.push(ch);
                                self.pos += 1;
                            }
                        }
                    }
                    if !text.is_empty() {
                        parts.push(Part::Quoted(text));
                    }
                }
                '$' => {
                    flush!();
                    self.pos += 1;
                    parts.push(self.dollar(false)?);
                }
                '`' => {
                    flush!();
                    self.pos += 1;
                    parts.push(self.backtick(false)?);
                }
                _ => {
                    lit.push(c);
                    self.pos += 1;
                }
            }
        }
        flush!();
        Ok(parts)
    }
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, LexError> {
    let mut lx = Lexer { chars: src.chars().collect(), pos: 0, _src: src };
    let mut tokens = Vec::new();
    while let Some(c) = lx.peek(0) {
        match c {
            ' ' | '\t' => lx.pos += 1,
            '\n' => {
                tokens.push(Token::Newline);
                lx.pos += 1;
            }
            '#' => {
                while lx.peek(0).is_some_and(|c| c != '\n') {
                    lx.pos += 1;
                }
            }
            '|' | '&' | ';' | '<' | '>' | '(' | ')' => {
                let two = (c, lx.peek(1));
                let (op, len): (&'static str, usize) = match two {
                    ('|', Some('|')) => ("||", 2),
                    ('|', _) => ("|", 1),
                    ('&', Some('&')) => ("&&", 2),
                    ('&', _) => ("&", 1),
                    (';', _) => (";", 1),
                    ('(', _) => ("(", 1),
                    (')', _) => (")", 1),
                    ('<', _) => ("<", 1),
                    ('>', Some('>')) => (">>", 2),
                    ('>', Some('&')) if lx.peek(2) == Some('2') => (">&2", 3),
                    _ => (">", 1),
                };
                tokens.push(Token::Op(op));
                lx.pos += len;
            }
            _ => {
                // `2>` / `2>>` / `2>&1` directly after whitespace.
                if c == '2' && lx.peek(1) == Some('>') {
                    if lx.peek(2) == Some('&') && lx.peek(3) == Some('1') {
                        tokens.push(Token::Op("2>&1"));
                        lx.pos += 4;
                        continue;
                    }
                    if lx.peek(2) == Some('>') {
                        tokens.push(Token::Op("2>>"));
                        lx.pos += 3;
                        continue;
                    }
                    tokens.push(Token::Op("2>"));
                    lx.pos += 2;
                    continue;
                }
                if c == '!' && matches!(lx.peek(1), Some(' ' | '\t')) {
                    tokens.push(Token::Op("!"));
                    lx.pos += 1;
                    continue;
                }
                let w = lx.word()?;
                if w.is_empty() {
                    return Err(LexError::Bad(alloc::format!("unexpected character '{}'", c)));
                }
                tokens.push(Token::Word(w));
            }
        }
    }
    Ok(tokens)
}
