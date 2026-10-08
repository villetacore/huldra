//! `$(( ... ))` integer arithmetic (C precedence, 64-bit).

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(i64),
    Name(String),
    Op(&'static str),
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    const OPS: [&str; 22] = [
        "<=", ">=", "==", "!=", "&&", "||", "<<", ">>", "+", "-", "*", "/", "%", "<", ">", "(", ")", "!", "~", "&",
        "|", "^",
    ];
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let v = if let Some(hex) = text.strip_prefix("0x") {
                i64::from_str_radix(hex, 16)
            } else {
                text.parse()
            };
            out.push(Tok::Num(v.map_err(|_| alloc::format!("bad number '{}'", text))?));
        } else if c.is_ascii_alphabetic() || c == '_' || c == '$' {
            let start = if c == '$' { i + 1 } else { i };
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Name(chars[start..i].iter().collect()));
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let op = OPS.iter().find(|op| rest.starts_with(**op)).ok_or_else(|| alloc::format!("unexpected '{}'", c))?;
            out.push(Tok::Op(op));
            i += op.len();
        }
    }
    Ok(out)
}

struct P<'a> {
    toks: Vec<Tok>,
    pos: usize,
    var: &'a dyn Fn(&str) -> i64,
}

fn prec(op: &str) -> Option<u8> {
    Some(match op {
        "||" => 1,
        "&&" => 2,
        "|" => 3,
        "^" => 4,
        "&" => 5,
        "==" | "!=" => 6,
        "<" | ">" | "<=" | ">=" => 7,
        "<<" | ">>" => 8,
        "+" | "-" => 9,
        "*" | "/" | "%" => 10,
        _ => return None,
    })
}

impl P<'_> {
    fn unary(&mut self) -> Result<i64, String> {
        match self.toks.get(self.pos).cloned() {
            Some(Tok::Num(n)) => {
                self.pos += 1;
                Ok(n)
            }
            Some(Tok::Name(n)) => {
                self.pos += 1;
                Ok((self.var)(&n))
            }
            Some(Tok::Op("(")) => {
                self.pos += 1;
                let v = self.binary(0)?;
                if self.toks.get(self.pos) != Some(&Tok::Op(")")) {
                    return Err("missing ')'".into());
                }
                self.pos += 1;
                Ok(v)
            }
            Some(Tok::Op("-")) => {
                self.pos += 1;
                Ok(self.unary()?.wrapping_neg())
            }
            Some(Tok::Op("+")) => {
                self.pos += 1;
                self.unary()
            }
            Some(Tok::Op("!")) => {
                self.pos += 1;
                Ok((self.unary()? == 0) as i64)
            }
            Some(Tok::Op("~")) => {
                self.pos += 1;
                Ok(!self.unary()?)
            }
            _ => Err("syntax error in expression".into()),
        }
    }

    fn binary(&mut self, min: u8) -> Result<i64, String> {
        let mut lhs = self.unary()?;
        while let Some(Tok::Op(op)) = self.toks.get(self.pos).cloned() {
            let Some(p) = prec(op) else { break };
            if p <= min {
                break;
            }
            self.pos += 1;
            let rhs = self.binary(p)?;
            lhs = match op {
                "+" => lhs.wrapping_add(rhs),
                "-" => lhs.wrapping_sub(rhs),
                "*" => lhs.wrapping_mul(rhs),
                "/" | "%" if rhs == 0 => return Err("division by zero".into()),
                "/" => lhs.wrapping_div(rhs),
                "%" => lhs.wrapping_rem(rhs),
                "<<" => lhs.wrapping_shl(rhs as u32),
                ">>" => lhs.wrapping_shr(rhs as u32),
                "<" => (lhs < rhs) as i64,
                ">" => (lhs > rhs) as i64,
                "<=" => (lhs <= rhs) as i64,
                ">=" => (lhs >= rhs) as i64,
                "==" => (lhs == rhs) as i64,
                "!=" => (lhs != rhs) as i64,
                "&" => lhs & rhs,
                "|" => lhs | rhs,
                "^" => lhs ^ rhs,
                "&&" => (lhs != 0 && rhs != 0) as i64,
                "||" => (lhs != 0 || rhs != 0) as i64,
                _ => unreachable!(),
            };
        }
        Ok(lhs)
    }
}

pub fn eval(expr: &str, var: &dyn Fn(&str) -> i64) -> Result<i64, String> {
    let toks = lex(expr)?;
    if toks.is_empty() {
        return Ok(0);
    }
    let mut p = P { toks, pos: 0, var };
    let v = p.binary(0)?;
    if p.pos != p.toks.len() {
        return Err("syntax error in expression".into());
    }
    Ok(v)
}
