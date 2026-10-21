//! `macro_rules!`: matching invocations against the rules and
//! transcribing the expansion. Fragments are recognized by running the
//! parser on the input (`$e:expr`, `$t:ty`, ...). There is no hygiene.

use crate::ast::MacroDef;
use crate::lex::{Tok, Token};
use crate::parse::{Parser, PathMode};
use crate::{err, Error, Result, Span};
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Default)]
pub struct Macros {
    defs: Vec<Rc<MacroDef>>,
}

impl Macros {
    pub fn get(&self, name: &str) -> Option<Rc<MacroDef>> {
        self.defs.iter().rev().find(|d| &*d.name == name).cloned()
    }

    pub fn define(&mut self, d: Rc<MacroDef>) {
        self.defs.push(d);
    }
}

fn is(t: &Token, p: &str) -> bool {
    matches!(&t.tok, Tok::Punct(q) if *q == p)
}

fn eof(span: Span) -> Token {
    Token {
        tok: Tok::Eof,
        span,
        joint: false,
    }
}

/// Splits a group's tokens (from an opening delimiter) and returns the
/// index after the matching close.
fn group_end(toks: &[Token], start: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, t) in toks.iter().enumerate().skip(start) {
        match &t.tok {
            Tok::Punct("(" | "[" | "{") => depth += 1,
            Tok::Punct(")" | "]" | "}") => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            Tok::Eof => return None,
            _ => {}
        }
    }
    None
}

/// `(matcher) => {transcriber};` rules.
pub fn parse_rules(name: Rc<str>, body: Vec<Token>, span: Span) -> Result<MacroDef> {
    let mut rules = Vec::new();
    let mut i = 0;
    let n = body.len() - 1; // final Eof
    while i < n {
        let end = group_end(&body, i).ok_or_else(|| Error::new(body[i].span, String::from("expected macro matcher")))?;
        let matcher = body[i + 1..end - 1].to_vec();
        i = end;
        if i >= n || !is(&body[i], "=>") {
            return err(body.get(i).map(|t| t.span).unwrap_or(span), String::from("expected `=>` in macro rule"));
        }
        i += 1;
        let end = group_end(&body, i).ok_or_else(|| Error::new(body[i].span, String::from("expected macro body")))?;
        let trans = body[i + 1..end - 1].to_vec();
        i = end;
        if i < n && is(&body[i], ";") {
            i += 1;
        }
        rules.push((matcher, trans));
    }
    Ok(MacroDef { name, rules })
}

#[derive(Clone, Debug)]
enum M {
    Tok(Tok),
    Frag(Rc<str>, Rc<str>),
    Rep(Vec<M>, Option<Tok>, char),
    Group(&'static str, Vec<M>),
}

fn parse_matcher(t: &[Token]) -> Result<Vec<M>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let tok = &t[i];
        match &tok.tok {
            Tok::Punct("$") => {
                match t.get(i + 1).map(|x| &x.tok) {
                    Some(Tok::Ident(name)) => {
                        if t.get(i + 2).is_some_and(|x| is(x, ":")) {
                            let kind = match t.get(i + 3).map(|x| &x.tok) {
                                Some(Tok::Ident(k)) => k.clone(),
                                _ => return err(tok.span, String::from("missing fragment specifier")),
                            };
                            out.push(M::Frag(name.clone(), kind));
                            i += 4;
                        } else {
                            // `$crate` and friends match literally.
                            out.push(M::Tok(Tok::Ident(name.clone())));
                            i += 2;
                        }
                    }
                    Some(Tok::Punct("(")) => {
                        let end = group_end(t, i + 1).ok_or_else(|| Error::new(tok.span, String::from("unclosed repetition")))?;
                        let inner = parse_matcher(&t[i + 2..end - 1])?;
                        i = end;
                        let mut sep = None;
                        let op;
                        match t.get(i).map(|x| &x.tok) {
                            Some(Tok::Punct(p @ ("*" | "+" | "?"))) => {
                                op = p.chars().next().unwrap();
                                i += 1;
                            }
                            Some(s) => {
                                sep = Some(s.clone());
                                op = match t.get(i + 1).map(|x| &x.tok) {
                                    Some(Tok::Punct(p @ ("*" | "+" | "?"))) => p.chars().next().unwrap(),
                                    _ => return err(tok.span, String::from("expected one of: `*`, `+`, or `?`")),
                                };
                                i += 2;
                            }
                            None => return err(tok.span, String::from("expected one of: `*`, `+`, or `?`")),
                        }
                        out.push(M::Rep(inner, sep, op));
                    }
                    _ => return err(tok.span, String::from("expected identifier after `$`")),
                }
            }
            Tok::Punct(p @ ("(" | "[" | "{")) => {
                let end = group_end(t, i).unwrap_or(t.len());
                out.push(M::Group(p, parse_matcher(&t[i + 1..end - 1])?));
                i = end;
            }
            other => {
                out.push(M::Tok(other.clone()));
                i += 1;
            }
        }
    }
    Ok(out)
}

#[derive(Clone, Debug)]
enum Binding {
    One(Vec<Token>),
    Many(Vec<Binding>),
}

type Bindings = Vec<(Rc<str>, Binding)>;

/// How many tokens a fragment takes at `input[pos..]`.
fn fragment_len(kind: &str, input: &[Token], pos: usize, span: Span) -> Option<usize> {
    let first = input.get(pos)?;
    if matches!(first.tok, Tok::Eof) {
        return None;
    }
    match kind {
        "ident" => match &first.tok {
            Tok::Ident(_) => Some(1),
            _ => None,
        },
        "lifetime" => match &first.tok {
            Tok::Lifetime(_) => Some(1),
            _ => None,
        },
        "literal" => {
            let neg = is(first, "-");
            let t = input.get(pos + neg as usize)?;
            match &t.tok {
                Tok::Int(..) | Tok::Float(..) | Tok::Str(_) | Tok::ByteStr(_) | Tok::Char(_) | Tok::Byte(_) => Some(1 + neg as usize),
                Tok::Ident(b) if !neg && (&**b == "true" || &**b == "false") => Some(1),
                _ => None,
            }
        }
        "tt" => match &first.tok {
            Tok::Punct("(" | "[" | "{") => group_end(input, pos).map(|e| e - pos),
            Tok::Punct(")" | "]" | "}") => None,
            _ => Some(1),
        },
        "vis" => {
            if matches!(&first.tok, Tok::Ident(s) if &**s == "pub") {
                if input.get(pos + 1).is_some_and(|t| is(t, "(")) {
                    group_end(input, pos + 1).map(|e| e - pos)
                } else {
                    Some(1)
                }
            } else {
                Some(0)
            }
        }
        _ => {
            let mut toks: Vec<Token> = input[pos..].to_vec();
            if !matches!(toks.last().map(|t| &t.tok), Some(Tok::Eof)) {
                toks.push(eof(span));
            }
            let mut macros = crate::macros::Macros::default();
            let mut p = Parser::new(toks, &mut macros);
            let ok = match kind {
                "expr" => p.expr().is_ok(),
                "ty" => p.ty().is_ok(),
                "pat" | "pat_param" => p.pat_top().is_ok(),
                "path" => p.path(PathMode::Type).is_ok(),
                "block" => p.block().is_ok(),
                "stmt" | "item" => p.expr().is_ok(),
                _ => false,
            };
            if ok {
                Some(p.position())
            } else {
                None
            }
        }
    }
}

fn match_seq(ms: &[M], input: &[Token], mut pos: usize, span: Span, b: &mut Bindings) -> Option<usize> {
    for m in ms {
        match m {
            M::Tok(t) => {
                let got = input.get(pos)?;
                if !tok_eq(&got.tok, t) {
                    return None;
                }
                pos += 1;
            }
            M::Frag(name, kind) => {
                let n = fragment_len(kind, input, pos, span)?;
                let mut toks = input[pos..pos + n].to_vec();
                if &**kind == "expr" && n > 1 {
                    // Keep precedence: `$e * 2` with `$e = a + b`.
                    let sp = toks[0].span;
                    toks.insert(0, Token { tok: Tok::Punct("("), span: sp, joint: false });
                    toks.push(Token { tok: Tok::Punct(")"), span: sp, joint: false });
                }
                b.push((name.clone(), Binding::One(toks)));
                pos += n;
            }
            M::Group(open, inner) => {
                let got = input.get(pos)?;
                if !is(got, open) {
                    return None;
                }
                let end = group_end(input, pos)?;
                let sub = &input[pos + 1..end - 1];
                let mut sub_toks = sub.to_vec();
                sub_toks.push(eof(input[end - 1].span));
                let used = match_seq(inner, &sub_toks, 0, span, b)?;
                if used != sub.len() {
                    return None;
                }
                pos = end;
            }
            M::Rep(inner, sep, op) => {
                let mut iters: Vec<Bindings> = Vec::new();
                loop {
                    if iters.len() == 1 && *op == '?' {
                        break;
                    }
                    let mut start = pos;
                    if !iters.is_empty() {
                        if let Some(s) = sep {
                            match input.get(pos) {
                                Some(t) if tok_eq(&t.tok, s) => start = pos + 1,
                                _ => break,
                            }
                        }
                    }
                    let mut nb = Vec::new();
                    match match_seq(inner, input, start, span, &mut nb) {
                        Some(p) if p > start || inner.is_empty() => {
                            // Stop if the rest cannot follow and this was optional? Greedy.
                            pos = p;
                            iters.push(nb);
                        }
                        _ => break,
                    }
                    if matches!(input.get(pos).map(|t| &t.tok), Some(Tok::Eof) | None) {
                        break;
                    }
                }
                if *op == '+' && iters.is_empty() {
                    return None;
                }
                // Collect each variable used inside as a `Many`.
                let mut names = Vec::new();
                collect_names(inner, &mut names);
                for n in names {
                    let items = iters
                        .iter()
                        .map(|it| it.iter().rev().find(|(k, _)| *k == n).map(|(_, v)| v.clone()).unwrap_or(Binding::Many(Vec::new())))
                        .collect();
                    b.push((n, Binding::Many(items)));
                }
            }
        }
    }
    Some(pos)
}

fn collect_names(ms: &[M], out: &mut Vec<Rc<str>>) {
    for m in ms {
        match m {
            M::Frag(n, _) => out.push(n.clone()),
            M::Rep(inner, ..) | M::Group(_, inner) => collect_names(inner, out),
            M::Tok(_) => {}
        }
    }
}

fn tok_eq(a: &Tok, b: &Tok) -> bool {
    match (a, b) {
        (Tok::Int(x, _), Tok::Int(y, _)) => x == y,
        _ => a == b,
    }
}

fn lookup<'a>(b: &'a Bindings, name: &str, idx: &[usize]) -> Option<&'a Binding> {
    let mut cur = &b.iter().rev().find(|(k, _)| &**k == name)?.1;
    for &i in idx {
        match cur {
            Binding::Many(v) => cur = v.get(i)?,
            Binding::One(_) => break,
        }
    }
    Some(cur)
}

/// The number of repetitions of a `$(...)` group in the transcriber.
fn rep_count(t: &[Token], b: &Bindings, idx: &[usize]) -> Option<usize> {
    let mut i = 0;
    while i < t.len() {
        if is(&t[i], "$") {
            if let Some(Tok::Ident(n)) = t.get(i + 1).map(|x| &x.tok) {
                if let Some(Binding::Many(v)) = lookup(b, n, idx) {
                    return Some(v.len());
                }
            }
        }
        i += 1;
    }
    None
}

fn transcribe(t: &[Token], b: &Bindings, idx: &mut Vec<usize>, out: &mut Vec<Token>, span: Span) -> Result<()> {
    let mut i = 0;
    while i < t.len() {
        let tok = &t[i];
        if is(tok, "$") {
            match t.get(i + 1).map(|x| &x.tok) {
                Some(Tok::Ident(n)) if &**n == "crate" => {
                    out.push(Token { tok: Tok::Ident(Rc::from("crate")), span: tok.span, joint: false });
                    i += 2;
                    continue;
                }
                Some(Tok::Ident(n)) => {
                    match lookup(b, n, idx) {
                        Some(Binding::One(toks)) => out.extend(toks.iter().cloned()),
                        Some(Binding::Many(_)) => return err(tok.span, format!("variable `{}` is still repeating at this depth", n)),
                        None => {
                            out.push(tok.clone());
                            out.push(t[i + 1].clone());
                        }
                    }
                    i += 2;
                    continue;
                }
                Some(Tok::Punct("(")) => {
                    let end = group_end(t, i + 1).ok_or_else(|| Error::new(tok.span, String::from("unclosed repetition")))?;
                    let inner = &t[i + 2..end - 1];
                    let mut j = end;
                    let mut sep = None;
                    match t.get(j).map(|x| &x.tok) {
                        Some(Tok::Punct("*" | "+" | "?")) => j += 1,
                        Some(_) => {
                            sep = Some(t[j].clone());
                            j += 2;
                        }
                        None => {}
                    }
                    let count = rep_count(inner, b, idx).ok_or_else(|| Error::new(tok.span, String::from("attempted to repeat an expression containing no syntax variables matched as repeating at this depth")))?;
                    for k in 0..count {
                        if k > 0 {
                            if let Some(s) = &sep {
                                out.push(s.clone());
                            }
                        }
                        idx.push(k);
                        transcribe(inner, b, idx, out, span)?;
                        idx.pop();
                    }
                    i = j;
                    continue;
                }
                _ => {}
            }
        }
        out.push(tok.clone());
        i += 1;
    }
    Ok(())
}

pub fn expand(def: &MacroDef, input: Vec<Token>, span: Span) -> Result<Vec<Token>> {
    for (matcher, trans) in &def.rules {
        let ms = parse_matcher(matcher)?;
        let mut b = Vec::new();
        if let Some(end) = match_seq(&ms, &input, 0, span, &mut b) {
            if matches!(input[end].tok, Tok::Eof) {
                let mut out = Vec::new();
                transcribe(trans, &b, &mut Vec::new(), &mut out, span)?;
                out.push(eof(span));
                return Ok(out);
            }
        }
    }
    err(span, format!("no rules expected this token in macro call `{}!`", def.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::tokenize;

    fn toks(s: &str) -> Vec<Token> {
        tokenize(s, 0).unwrap()
    }

    #[test]
    fn repetition() {
        let body = toks("($($x:expr),*) => { 0 $(+ $x)* };");
        let def = parse_rules(Rc::from("sum"), body, Span::default()).unwrap();
        let out = expand(&def, toks("1, 2 * 3, 4"), Span::default()).unwrap();
        let text = crate::parse::stringify(&out);
        assert_eq!(text, "0 + 1 + (2 * 3) + 4");
    }
}
