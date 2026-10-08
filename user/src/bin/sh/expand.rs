//! Word expansion: parameters, command substitution, field splitting and
//! pathname globbing.

use crate::exec::Shell;
use crate::lexer::{Part, Word};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use huldra_user::fs;

/// A field under construction: characters plus "may glob" flags.
#[derive(Default)]
struct Field {
    chars: Vec<(char, bool)>,
    quoted: bool,
}

impl Field {
    fn push_str(&mut self, s: &str, glob: bool) {
        self.chars.extend(s.chars().map(|c| (c, glob)));
    }

    fn text(&self) -> String {
        self.chars.iter().map(|&(c, _)| c).collect()
    }

    fn has_glob(&self) -> bool {
        self.chars.iter().any(|&(c, g)| g && matches!(c, '*' | '?' | '['))
    }
}

impl Shell {
    fn special_or_var(&mut self, name: &str) -> String {
        match name {
            "?" => self.status.to_string(),
            "$" => huldra_user::process::getpid().to_string(),
            "#" => self.params.len().saturating_sub(1).to_string(),
            "!" => self.last_background.to_string(),
            "@" | "*" => self.params.get(1..).unwrap_or(&[]).join(" "),
            n if n.chars().all(|c| c.is_ascii_digit()) => {
                n.parse::<usize>().ok().and_then(|i| self.params.get(i).cloned()).unwrap_or_default()
            }
            n => self.get_var(n).unwrap_or_default(),
        }
    }

    fn expand_fields(&mut self, word: &Word) -> Vec<Field> {
        let mut fields = alloc::vec![Field::default()];
        for part in word {
            match part {
                Part::Lit(s) => fields.last_mut().unwrap().push_str(s, true),
                Part::Quoted(s) => {
                    let f = fields.last_mut().unwrap();
                    f.push_str(s, false);
                    f.quoted = true;
                }
                Part::Var { name, quoted: true } if name == "@" => {
                    // "$@": one field per positional parameter.
                    let params: Vec<String> = self.params.get(1..).unwrap_or(&[]).to_vec();
                    for (i, p) in params.iter().enumerate() {
                        if i > 0 {
                            fields.push(Field { quoted: true, ..Field::default() });
                        }
                        fields.last_mut().unwrap().push_str(p, false);
                    }
                    fields.last_mut().unwrap().quoted |= !params.is_empty();
                }
                Part::Var { name, quoted } => {
                    let value = self.special_or_var(name);
                    self.append_value(&mut fields, &value, *quoted);
                }
                Part::Command { source, quoted } => {
                    let mut out = self.command_substitution(source);
                    while out.ends_with('\n') {
                        out.pop();
                    }
                    self.append_value(&mut fields, &out, *quoted);
                }
                Part::Arith(expr) => {
                    let v = self.arith(expr);
                    self.append_value(&mut fields, &v, true);
                }
            }
        }
        fields
    }

    fn append_value(&mut self, fields: &mut Vec<Field>, value: &str, quoted: bool) {
        if quoted {
            let f = fields.last_mut().unwrap();
            f.push_str(value, false);
            f.quoted = true;
            return;
        }
        // Unquoted: split on whitespace.
        let starts_with_space = value.starts_with(char::is_whitespace);
        let mut pieces = value.split_whitespace().peekable();
        if starts_with_space && !fields.last().unwrap().chars.is_empty() {
            fields.push(Field::default());
        }
        while let Some(p) = pieces.next() {
            fields.last_mut().unwrap().push_str(p, false);
            if pieces.peek().is_some() {
                fields.push(Field::default());
            }
        }
        if value.ends_with(char::is_whitespace) && !value.trim().is_empty() {
            fields.push(Field::default());
        }
    }

    /// Expands command words into arguments.
    pub fn expand_words(&mut self, words: &[Word]) -> Vec<String> {
        let mut out = Vec::new();
        for w in words {
            for f in self.expand_fields(w) {
                if f.chars.is_empty() && !f.quoted {
                    continue;
                }
                if f.has_glob() {
                    let matches = glob(&f);
                    if !matches.is_empty() {
                        out.extend(matches);
                        continue;
                    }
                }
                out.push(f.text());
            }
        }
        out
    }

    /// Expands a word into one string (assignments, redirection targets).
    pub fn expand_single(&mut self, word: &Word) -> String {
        let mut s = String::new();
        for part in word {
            match part {
                Part::Lit(t) | Part::Quoted(t) => s.push_str(t),
                Part::Var { name, .. } => s.push_str(&self.special_or_var(name)),
                Part::Command { source, .. } => {
                    let mut out = self.command_substitution(source);
                    while out.ends_with('\n') {
                        out.pop();
                    }
                    s.push_str(&out);
                }
                Part::Arith(expr) => s.push_str(&self.arith(expr)),
            }
        }
        s
    }

    fn arith(&mut self, expr: &str) -> String {
        let lookup = |name: &str| -> i64 {
            huldra_user::env::var(name).and_then(|v| v.trim().parse().ok()).unwrap_or(0)
        };
        match crate::arith::eval(expr, &lookup) {
            Ok(v) => v.to_string(),
            Err(e) => {
                huldra_user::eprintln!("sh: $(({})): {}", expr, e);
                self.status = 1;
                String::from("0")
            }
        }
    }
}

/// Shell pattern matching (`*`, `?`, `[a-z]`, `[!x]`) on characters.
pub fn fnmatch(pattern: &[(char, bool)], text: &[char]) -> bool {
    let (mut p, mut t) = (0, 0);
    let (mut star_p, mut star_t) = (usize::MAX, 0);
    while t < text.len() {
        if p < pattern.len() {
            let (pc, special) = pattern[p];
            if special && pc == '*' {
                star_p = p;
                star_t = t;
                p += 1;
                continue;
            }
            if special && pc == '?' {
                p += 1;
                t += 1;
                continue;
            }
            if special && pc == '[' {
                if let Some((matched, len)) = match_class(&pattern[p..], text[t]) {
                    if matched {
                        p += len;
                        t += 1;
                        continue;
                    }
                } else if text[t] == '[' {
                    p += 1;
                    t += 1;
                    continue;
                }
            } else if pc == text[t] {
                p += 1;
                t += 1;
                continue;
            }
        }
        if star_p != usize::MAX {
            p = star_p + 1;
            star_t += 1;
            t = star_t;
            continue;
        }
        return false;
    }
    while p < pattern.len() && pattern[p].1 && pattern[p].0 == '*' {
        p += 1;
    }
    p == pattern.len()
}

/// Matches `c` against a `[...]` class; returns (matched, pattern length).
fn match_class(pattern: &[(char, bool)], c: char) -> Option<(bool, usize)> {
    let mut i = 1;
    let negate = matches!(pattern.get(1), Some(('!' | '^', _)));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while i < pattern.len() {
        let (pc, _) = pattern[i];
        if pc == ']' && !first {
            return Some((matched != negate, i + 1));
        }
        first = false;
        if i + 2 < pattern.len() && pattern[i + 1].0 == '-' && pattern[i + 2].0 != ']' {
            if pc <= c && c <= pattern[i + 2].0 {
                matched = true;
            }
            i += 3;
        } else {
            if pc == c {
                matched = true;
            }
            i += 1;
        }
    }
    None
}

/// Expands a pattern against the file system; sorted, empty if no match.
fn glob(field: &Field) -> Vec<String> {
    let absolute = field.chars.first().is_some_and(|&(c, _)| c == '/');
    let mut components: Vec<Vec<(char, bool)>> = Vec::new();
    let mut cur = Vec::new();
    for &(c, g) in &field.chars {
        if c == '/' {
            if !cur.is_empty() {
                components.push(core::mem::take(&mut cur));
            }
        } else {
            cur.push((c, g));
        }
    }
    if !cur.is_empty() {
        components.push(cur);
    }
    let mut paths = alloc::vec![if absolute { String::from("/") } else { String::new() }];
    for comp in &components {
        let is_pattern = comp.iter().any(|&(c, g)| g && matches!(c, '*' | '?' | '['));
        let mut next = Vec::new();
        for base in &paths {
            if !is_pattern {
                let name: String = comp.iter().map(|&(c, _)| c).collect();
                next.push(join(base, &name));
                continue;
            }
            let dir = if base.is_empty() { "." } else { base.as_str() };
            let Ok(entries) = fs::read_dir(dir) else { continue };
            for e in entries {
                if e.name.starts_with('.') && comp.first().map(|&(c, _)| c) != Some('.') {
                    continue;
                }
                let chars: Vec<char> = e.name.chars().collect();
                if fnmatch(comp, &chars) {
                    next.push(join(base, &e.name));
                }
            }
        }
        paths = next;
    }
    let mut result: Vec<String> = paths.into_iter().filter(|p| !p.is_empty() && fs::exists(p)).collect();
    result.sort();
    result
}

fn join(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else if base.ends_with('/') {
        alloc::format!("{}{}", base, name)
    } else {
        alloc::format!("{}/{}", base, name)
    }
}
