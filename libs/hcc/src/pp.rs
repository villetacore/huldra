//! The C preprocessor: directives, conditional compilation and macro
//! expansion (object-like and function-like, `#`, `##`, `__VA_ARGS__`).
//! Recursion is prevented with per-token hidesets (Prosser's algorithm,
//! simplified).

use crate::lex::{tokenize, Tok, Token};
use crate::{Error, FileSource};
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone)]
struct Macro {
    params: Option<Vec<Rc<str>>>,
    variadic: bool,
    body: Vec<Token>,
}

struct Cond {
    /// This branch is being compiled.
    active: bool,
    /// Some branch of this #if chain has been taken.
    taken: bool,
    /// The enclosing region is active.
    parent: bool,
}

pub struct Preprocessor<'a> {
    fs: &'a dyn FileSource,
    include_dirs: Vec<String>,
    macros: BTreeMap<Rc<str>, Macro>,
    pub files: Vec<String>,
    once: BTreeSet<String>,
    depth: usize,
    conds: Vec<Cond>,
}

fn num_token(v: i64, like: &Token) -> Token {
    let mut t = like.clone();
    t.tok = Tok::Num(Rc::from(v.to_string().as_str()));
    t
}

impl<'a> Preprocessor<'a> {
    pub fn new(fs: &'a dyn FileSource, include_dirs: Vec<String>) -> Self {
        let mut pp = Preprocessor {
            fs,
            include_dirs,
            macros: BTreeMap::new(),
            files: Vec::new(),
            once: BTreeSet::new(),
            depth: 0,
            conds: Vec::new(),
        };
        for (name, value) in [
            ("__huldra__", "1"),
            ("__hcc__", "1"),
            ("__x86_64__", "1"),
            ("__x86_64", "1"),
            ("__LP64__", "1"),
            ("__STDC__", "1"),
            ("__STDC_VERSION__", "201112L"),
            ("__STDC_HOSTED__", "1"),
            ("__CHAR_BIT__", "8"),
            ("__SIZEOF_POINTER__", "8"),
            ("__SIZEOF_LONG__", "8"),
            ("__SIZEOF_INT__", "4"),
            ("__SIZE_TYPE__", "unsigned long"),
            ("__PTRDIFF_TYPE__", "long"),
            ("__INT_MAX__", "2147483647"),
            ("__LONG_MAX__", "9223372036854775807L"),
        ] {
            pp.define_str(name, value);
        }
        pp
    }

    /// `-D NAME=VALUE`
    pub fn define_str(&mut self, name: &str, value: &str) {
        let body = tokenize(value, 0, "<command line>").map(|mut v| {
            v.pop();
            v
        });
        if let Ok(body) = body {
            self.macros.insert(
                Rc::from(name),
                Macro {
                    params: None,
                    variadic: false,
                    body,
                },
            );
        }
    }

    fn active(&self) -> bool {
        self.conds.last().is_none_or(|c| c.active)
    }

    fn err(&self, t: &Token, msg: impl Into<String>) -> Error {
        Error {
            file: self.files.get(t.file as usize).cloned().unwrap_or_default(),
            line: t.line,
            message: msg.into(),
        }
    }

    fn load(&mut self, path: &str) -> Result<Option<Vec<Token>>, String> {
        if self.once.contains(path) {
            return Ok(None);
        }
        let text = self
            .fs
            .read(path)
            .ok_or_else(|| alloc::format!("{}: no such file", path))?;
        let idx = self.files.len() as u16;
        self.files.push(String::from(path));
        let mut toks = tokenize(&text, idx, path)
            .map_err(|e| alloc::format!("{}:{}: {}", e.file, e.line, e.message))?;
        toks.pop(); // Eof
        Ok(Some(toks))
    }

    /// Preprocesses the file at `path` into a token stream ending with `Eof`.
    pub fn run_file(&mut self, path: &str) -> Result<Vec<Token>, Error> {
        let toks = self
            .load(path)
            .map_err(|m| Error {
                file: String::from(path),
                line: 0,
                message: m,
            })?
            .unwrap_or_default();
        let mut out = self.process(toks.into())?;
        if let Some(c) = self.conds.first() {
            let _ = c;
            return Err(Error {
                file: String::from(path),
                line: 0,
                message: String::from("unterminated #if"),
            });
        }
        let empty: Rc<[Rc<str>]> = Rc::from(Vec::new());
        out.push(Token {
            tok: Tok::Eof,
            file: 0,
            line: 0,
            bol: true,
            space: false,
            hideset: empty,
        });
        Ok(out)
    }

    fn process(&mut self, mut input: VecDeque<Token>) -> Result<Vec<Token>, Error> {
        let mut out = Vec::new();
        while let Some(t) = input.pop_front() {
            if t.bol && t.is("#") {
                let mut line = Vec::new();
                while input.front().is_some_and(|n| !n.bol) {
                    line.push(input.pop_front().unwrap());
                }
                self.directive(&t, line, &mut input)?;
                continue;
            }
            if !self.active() {
                continue;
            }
            if let Some(expanded) = self.try_expand(&t, &mut input)? {
                for e in expanded.into_iter().rev() {
                    input.push_front(e);
                }
                continue;
            }
            out.push(t);
        }
        Ok(out)
    }

    /// Expands the macro named by `t` if there is one; the expansion is
    /// pushed back onto the input by the caller.
    fn try_expand(
        &mut self,
        t: &Token,
        input: &mut VecDeque<Token>,
    ) -> Result<Option<Vec<Token>>, Error> {
        let Some(name) = t.ident() else {
            return Ok(None);
        };
        if t.hideset.iter().any(|h| &**h == name) {
            return Ok(None);
        }
        match name {
            "__LINE__" => return Ok(Some(alloc::vec![num_token(t.line as i64, t)])),
            "__FILE__" => {
                let mut s = t.clone();
                s.tok = Tok::Str(Rc::from(
                    self.files
                        .get(t.file as usize)
                        .cloned()
                        .unwrap_or_default()
                        .into_bytes(),
                ));
                return Ok(Some(alloc::vec![s]));
            }
            _ => {}
        }
        let Some(m) = self.macros.get(name).cloned() else {
            return Ok(None);
        };
        let mut hideset: Vec<Rc<str>> = t.hideset.to_vec();
        hideset.push(Rc::from(name));
        let hideset: Rc<[Rc<str>]> = Rc::from(hideset);

        let Some(params) = &m.params else {
            let body = m
                .body
                .iter()
                .map(|b| self.mark(b, t, &hideset))
                .collect::<Vec<_>>();
            return Ok(Some(self.paste_all(body, t)?));
        };

        // Function-like: only when followed by '('.
        if !input.front().is_some_and(|n| n.is("(")) {
            return Ok(None);
        }
        input.pop_front();
        let mut args: Vec<Vec<Token>> = alloc::vec![Vec::new()];
        let mut depth = 0;
        loop {
            let Some(a) = input.pop_front() else {
                return Err(self.err(t, alloc::format!("unterminated call to macro {}", name)));
            };
            if a.is("(") {
                depth += 1;
            } else if a.is(")") {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            } else if a.is(",") && depth == 0 && !(m.variadic && args.len() > params.len()) {
                args.push(Vec::new());
                continue;
            }
            args.last_mut().unwrap().push(a);
        }
        if params.is_empty() && args.len() == 1 && args[0].is_empty() {
            args.clear();
        }
        if m.variadic && args.len() == params.len() {
            args.push(Vec::new());
        }
        let expected = params.len() + m.variadic as usize;
        if args.len() != expected {
            return Err(self.err(
                t,
                alloc::format!(
                    "macro {} expects {} arguments, got {}",
                    name,
                    expected,
                    args.len()
                ),
            ));
        }
        let arg_of = |n: &str| -> Option<usize> {
            if m.variadic && n == "__VA_ARGS__" {
                return Some(params.len());
            }
            params.iter().position(|p| &**p == n)
        };

        let mut result: Vec<Token> = Vec::new();
        let body = &m.body;
        let mut i = 0;
        while i < body.len() {
            let b = &body[i];
            // #param: stringify.
            if b.is("#") {
                if let Some(idx) = body.get(i + 1).and_then(|n| n.ident()).and_then(arg_of) {
                    let mut s = String::new();
                    for (k, a) in args[idx].iter().enumerate() {
                        if k > 0 && a.space {
                            s.push(' ');
                        }
                        s.push_str(&a.spelling());
                    }
                    let mut st = self.mark(b, t, &hideset);
                    st.tok = Tok::Str(Rc::from(s.into_bytes()));
                    result.push(st);
                    i += 2;
                    continue;
                }
            }
            let next_is_paste = body.get(i + 1).is_some_and(|n| n.is("##"));
            let prev_is_paste = i > 0 && body[i - 1].is("##");
            if let Some(idx) = b.ident().and_then(arg_of) {
                if next_is_paste || prev_is_paste {
                    // GNU: `, ## __VA_ARGS__` drops the comma when empty.
                    if prev_is_paste
                        && args[idx].is_empty()
                        && result.len() >= 2
                        && result[result.len() - 2].is(",")
                    {
                        result.truncate(result.len() - 2);
                        i += 1;
                        continue;
                    }
                    result.extend(args[idx].iter().map(|a| self.mark(a, t, &hideset)));
                } else {
                    let expanded = self.expand_list(args[idx].clone())?;
                    result.extend(expanded.iter().map(|a| self.mark(a, t, &hideset)));
                }
                i += 1;
                continue;
            }
            result.push(self.mark(b, t, &hideset));
            i += 1;
        }
        Ok(Some(self.paste_all(result, t)?))
    }

    /// A body token as it appears at the expansion site.
    fn mark(&self, b: &Token, site: &Token, hideset: &Rc<[Rc<str>]>) -> Token {
        let mut t = b.clone();
        t.file = site.file;
        t.line = site.line;
        t.bol = false;
        let mut hs: Vec<Rc<str>> = hideset.to_vec();
        hs.extend(b.hideset.iter().cloned());
        t.hideset = Rc::from(hs);
        t
    }

    /// Applies `##` token pasting.
    fn paste_all(&self, toks: Vec<Token>, site: &Token) -> Result<Vec<Token>, Error> {
        let mut out: Vec<Token> = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            if toks[i].is("##") && !out.is_empty() && i + 1 < toks.len() {
                let left = out.pop().unwrap();
                let text = left.spelling() + &toks[i + 1].spelling();
                let mut lexed = tokenize(&text, site.file, "<paste>").map_err(|_| {
                    self.err(
                        site,
                        alloc::format!("pasting does not give a valid token: {}", text),
                    )
                })?;
                lexed.pop();
                if lexed.len() != 1 {
                    return Err(self.err(
                        site,
                        alloc::format!("pasting does not give a valid token: {}", text),
                    ));
                }
                let mut t = left.clone();
                t.tok = lexed.remove(0).tok;
                out.push(t);
                i += 2;
                continue;
            }
            out.push(toks[i].clone());
            i += 1;
        }
        Ok(out)
    }

    /// Fully macro-expands a token list (macro arguments, #if lines).
    fn expand_list(&mut self, toks: Vec<Token>) -> Result<Vec<Token>, Error> {
        let mut input: VecDeque<Token> = toks.into();
        let mut out = Vec::new();
        while let Some(t) = input.pop_front() {
            if let Some(expanded) = self.try_expand(&t, &mut input)? {
                for e in expanded.into_iter().rev() {
                    input.push_front(e);
                }
                continue;
            }
            out.push(t);
        }
        Ok(out)
    }

    fn directive(
        &mut self,
        hash: &Token,
        line: Vec<Token>,
        input: &mut VecDeque<Token>,
    ) -> Result<(), Error> {
        let Some(first) = line.first() else {
            return Ok(());
        };
        let name = first.ident().map(String::from).unwrap_or_default();
        let rest = &line[1..];
        // Conditionals are processed even in skipped regions.
        match name.as_str() {
            "if" | "ifdef" | "ifndef" => {
                let parent = self.active();
                let value = if !parent {
                    false
                } else if name == "if" {
                    self.eval_if(hash, rest.to_vec())?
                } else {
                    let n = rest
                        .first()
                        .and_then(|t| t.ident())
                        .ok_or_else(|| self.err(hash, "macro name missing"))?;
                    self.macros.contains_key(n) == (name == "ifdef")
                };
                self.conds.push(Cond {
                    active: parent && value,
                    taken: value,
                    parent,
                });
                return Ok(());
            }
            "elif" => {
                let c = self
                    .conds
                    .last()
                    .ok_or_else(|| self.err(hash, "#elif without #if"))?;
                let (parent, taken) = (c.parent, c.taken);
                let value = parent && !taken && self.eval_if(hash, rest.to_vec())?;
                let c = self.conds.last_mut().unwrap();
                c.active = value;
                c.taken |= value;
                return Ok(());
            }
            "else" => {
                let c = self.conds.last_mut().ok_or_else(|| Error {
                    file: String::new(),
                    line: hash.line,
                    message: String::from("#else without #if"),
                })?;
                c.active = c.parent && !c.taken;
                c.taken = true;
                return Ok(());
            }
            "endif" => {
                self.conds
                    .pop()
                    .ok_or_else(|| self.err(hash, "#endif without #if"))?;
                return Ok(());
            }
            _ => {}
        }
        if !self.active() {
            return Ok(());
        }
        match name.as_str() {
            "define" => {
                let n = rest
                    .first()
                    .and_then(|t| t.ident())
                    .ok_or_else(|| self.err(hash, "macro name missing"))?;
                let name: Rc<str> = Rc::from(n);
                let m = if rest.get(1).is_some_and(|t| t.is("(") && !t.space) {
                    let mut params = Vec::new();
                    let mut variadic = false;
                    let mut i = 2;
                    loop {
                        let t = rest
                            .get(i)
                            .ok_or_else(|| self.err(hash, "unterminated macro parameter list"))?;
                        if t.is(")") {
                            i += 1;
                            break;
                        }
                        if t.is(",") {
                            i += 1;
                            continue;
                        }
                        if t.is("...") {
                            variadic = true;
                        } else if let Some(p) = t.ident() {
                            params.push(Rc::from(p));
                        } else {
                            return Err(self.err(hash, "bad macro parameter"));
                        }
                        i += 1;
                    }
                    Macro {
                        params: Some(params),
                        variadic,
                        body: rest[i..].to_vec(),
                    }
                } else {
                    Macro {
                        params: None,
                        variadic: false,
                        body: rest[1..].to_vec(),
                    }
                };
                self.macros.insert(name, m);
            }
            "undef" => {
                if let Some(n) = rest.first().and_then(|t| t.ident()) {
                    self.macros.remove(n);
                }
            }
            "include" => {
                let (path, quoted) = match rest.first().map(|t| &t.tok) {
                    Some(Tok::Str(s)) => (String::from_utf8_lossy(s).into_owned(), true),
                    Some(Tok::Punct("<")) => {
                        let mut p = String::new();
                        for t in &rest[1..] {
                            if t.is(">") {
                                break;
                            }
                            p.push_str(&t.spelling());
                        }
                        (p, false)
                    }
                    _ => return Err(self.err(hash, "expected \"FILENAME\" or <FILENAME>")),
                };
                let current = self
                    .files
                    .get(hash.file as usize)
                    .cloned()
                    .unwrap_or_default();
                let mut candidates = Vec::new();
                if quoted || path.starts_with('/') {
                    if path.starts_with('/') {
                        candidates.push(path.clone());
                    } else {
                        let dir = current
                            .rfind('/')
                            .map_or(String::new(), |i| String::from(&current[..=i]));
                        candidates.push(alloc::format!("{}{}", dir, path));
                    }
                }
                for d in &self.include_dirs {
                    candidates.push(alloc::format!("{}/{}", d.trim_end_matches('/'), path));
                }
                let found = candidates
                    .into_iter()
                    .find(|c| self.fs.read(c).is_some())
                    .ok_or_else(|| {
                        self.err(hash, alloc::format!("{}: No such file or directory", path))
                    })?;
                if self.depth > 64 {
                    return Err(self.err(hash, "#include nested too deeply"));
                }
                if let Some(toks) = self.load(&found).map_err(|m| self.err(hash, m))? {
                    for t in toks.into_iter().rev() {
                        input.push_front(t);
                    }
                }
            }
            "pragma" => {
                if rest.first().and_then(|t| t.ident()) == Some("once") {
                    let current = self
                        .files
                        .get(hash.file as usize)
                        .cloned()
                        .unwrap_or_default();
                    self.once.insert(current);
                }
            }
            "error" => {
                let msg: Vec<String> = rest.iter().map(|t| t.spelling()).collect();
                return Err(self.err(hash, alloc::format!("#error {}", msg.join(" "))));
            }
            "warning" | "line" | "ident" | "" => {}
            other => {
                return Err(self.err(
                    hash,
                    alloc::format!("invalid preprocessing directive #{}", other),
                ))
            }
        }
        Ok(())
    }

    fn eval_if(&mut self, hash: &Token, line: Vec<Token>) -> Result<bool, Error> {
        // Replace `defined X` / `defined(X)` before macro expansion.
        let mut toks = Vec::new();
        let mut i = 0;
        while i < line.len() {
            if line[i].ident() == Some("defined") {
                let (name, len) = if line.get(i + 1).is_some_and(|t| t.is("(")) {
                    (line.get(i + 2).and_then(|t| t.ident()), 4)
                } else {
                    (line.get(i + 1).and_then(|t| t.ident()), 2)
                };
                let name = name.ok_or_else(|| self.err(hash, "bad defined()"))?;
                toks.push(num_token(self.macros.contains_key(name) as i64, &line[i]));
                i += len;
                continue;
            }
            toks.push(line[i].clone());
            i += 1;
        }
        let toks = self.expand_list(toks)?;
        let mut e = IfEval {
            toks: &toks,
            pos: 0,
        };
        let v = e.ternary().map_err(|m| self.err(hash, m))?;
        Ok(v != 0)
    }
}

/// Integer constant expressions in `#if`.
struct IfEval<'t> {
    toks: &'t [Token],
    pos: usize,
}

pub fn parse_int_literal(s: &str) -> Option<i64> {
    let t = s.trim_end_matches(['u', 'U', 'l', 'L']);
    let v = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).ok()?
    } else if let Some(b) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        u64::from_str_radix(b, 2).ok()?
    } else if t.len() > 1 && t.starts_with('0') {
        u64::from_str_radix(&t[1..], 8).ok()?
    } else {
        t.parse::<u64>().ok()?
    };
    Some(v as i64)
}

impl IfEval<'_> {
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }

    fn eat(&mut self, p: &str) -> bool {
        if self.peek().is_some_and(|t| t.is(p)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn primary(&mut self) -> Result<i64, String> {
        let t = self.peek().cloned().ok_or("missing expression in #if")?;
        self.pos += 1;
        match &t.tok {
            Tok::Num(s) => {
                parse_int_literal(s).ok_or_else(|| alloc::format!("invalid number in #if: {}", s))
            }
            Tok::Char(c) => Ok(*c),
            Tok::Ident(_) => Ok(0), // unknown identifiers are 0
            Tok::Punct("(") => {
                let v = self.ternary()?;
                if !self.eat(")") {
                    return Err("missing ')' in #if".into());
                }
                Ok(v)
            }
            Tok::Punct("-") => Ok(self.primary()?.wrapping_neg()),
            Tok::Punct("+") => self.primary(),
            Tok::Punct("!") => Ok((self.primary()? == 0) as i64),
            Tok::Punct("~") => Ok(!self.primary()?),
            _ => Err(alloc::format!("unexpected '{}' in #if", t.spelling())),
        }
    }

    fn binary(&mut self, level: usize) -> Result<i64, String> {
        const LEVELS: &[&[&str]] = &[
            &["||"],
            &["&&"],
            &["|"],
            &["^"],
            &["&"],
            &["==", "!="],
            &["<", "<=", ">", ">="],
            &["<<", ">>"],
            &["+", "-"],
            &["*", "/", "%"],
        ];
        if level == LEVELS.len() {
            return self.primary();
        }
        let mut l = self.binary(level + 1)?;
        loop {
            let Some(op) = LEVELS[level]
                .iter()
                .find(|op| self.peek().is_some_and(|t| t.is(op)))
            else {
                return Ok(l);
            };
            self.pos += 1;
            let r = self.binary(level + 1)?;
            l = match *op {
                "||" => (l != 0 || r != 0) as i64,
                "&&" => (l != 0 && r != 0) as i64,
                "|" => l | r,
                "^" => l ^ r,
                "&" => l & r,
                "==" => (l == r) as i64,
                "!=" => (l != r) as i64,
                "<" => (l < r) as i64,
                "<=" => (l <= r) as i64,
                ">" => (l > r) as i64,
                ">=" => (l >= r) as i64,
                "<<" => l.wrapping_shl(r as u32),
                ">>" => l.wrapping_shr(r as u32),
                "+" => l.wrapping_add(r),
                "-" => l.wrapping_sub(r),
                "*" => l.wrapping_mul(r),
                "/" | "%" if r == 0 => return Err("division by zero in #if".into()),
                "/" => l.wrapping_div(r),
                _ => l.wrapping_rem(r),
            };
        }
    }

    fn ternary(&mut self) -> Result<i64, String> {
        let c = self.binary(0)?;
        if self.eat("?") {
            let a = self.ternary()?;
            if !self.eat(":") {
                return Err("missing ':' in #if".into());
            }
            let b = self.ternary()?;
            return Ok(if c != 0 { a } else { b });
        }
        Ok(c)
    }
}
