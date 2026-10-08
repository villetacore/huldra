//! sed: a small stream editor.
//!
//! Supports `-n`, `-i`, `-e`, and commands `s/re/rep/[g][p]`, `p`, `d`,
//! `q`, `=` with addresses `N`, `$`, `/text/`, `N,M`. Patterns are plain
//! text with `^`/`$` anchors and `.` wildcard (no full regular expressions).

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use huldra_user::io::input_lines;
use huldra_user::{env, eprintln, format, fs, print};

huldra_user::main!(main);

#[derive(Clone)]
enum Addr {
    Line(usize),
    Last,
    Pattern(String),
}

#[derive(Clone)]
enum Cmd {
    Subst { pat: String, rep: String, global: bool, print: bool },
    Print,
    Delete,
    Quit,
    LineNumber,
}

struct Command {
    from: Option<Addr>,
    to: Option<Addr>,
    cmd: Cmd,
}

/// Matches `pat` (with `.`, `^`, `$`) at byte position `at`; returns match length.
fn match_at(pat: &[char], text: &[char], at: usize) -> Option<usize> {
    let mut p = 0;
    let mut t = at;
    while p < pat.len() {
        if pat[p] == '$' && p + 1 == pat.len() {
            return (t == text.len()).then_some(t - at);
        }
        if t >= text.len() || (pat[p] != '.' && pat[p] != text[t]) {
            return None;
        }
        p += 1;
        t += 1;
    }
    Some(t - at)
}

/// Finds the first match at or after `from`: (start, len).
fn find(pat: &str, text: &[char], from: usize) -> Option<(usize, usize)> {
    let (anchored, body) = match pat.strip_prefix('^') {
        Some(b) => (true, b),
        None => (false, pat),
    };
    let p: Vec<char> = body.chars().collect();
    if anchored {
        return if from == 0 { match_at(&p, text, 0).map(|l| (0, l)) } else { None };
    }
    (from..=text.len()).find_map(|i| match_at(&p, text, i).map(|l| (i, l)))
}

fn parse_addr(s: &[char], i: &mut usize) -> Option<Addr> {
    match s.get(*i)? {
        '$' => {
            *i += 1;
            Some(Addr::Last)
        }
        '/' => {
            let start = *i + 1;
            let end = start + s[start..].iter().position(|&c| c == '/')?;
            *i = end + 1;
            Some(Addr::Pattern(s[start..end].iter().collect()))
        }
        c if c.is_ascii_digit() => {
            let start = *i;
            while s.get(*i).is_some_and(|c| c.is_ascii_digit()) {
                *i += 1;
            }
            s[start..*i].iter().collect::<String>().parse().ok().map(Addr::Line)
        }
        _ => None,
    }
}

fn parse(script: &str) -> Result<Vec<Command>, String> {
    let mut cmds = Vec::new();
    for part in script.split([';', '\n']).map(str::trim).filter(|p| !p.is_empty()) {
        let s: Vec<char> = part.chars().collect();
        let mut i = 0;
        let from = parse_addr(&s, &mut i);
        let to = if s.get(i) == Some(&',') {
            i += 1;
            Some(parse_addr(&s, &mut i).ok_or("bad address")?)
        } else {
            None
        };
        let cmd = match s.get(i) {
            Some('p') => Cmd::Print,
            Some('d') => Cmd::Delete,
            Some('q') => Cmd::Quit,
            Some('=') => Cmd::LineNumber,
            Some('s') => {
                let delim = *s.get(i + 1).ok_or("bad s command")?;
                let rest: String = s[i + 2..].iter().collect();
                let fields: Vec<&str> = rest.splitn(3, delim).collect();
                if fields.len() < 3 {
                    return Err(format!("unterminated s command: {}", part));
                }
                Cmd::Subst {
                    pat: String::from(fields[0]),
                    rep: String::from(fields[1]),
                    global: fields[2].contains('g'),
                    print: fields[2].contains('p'),
                }
            }
            _ => return Err(format!("unknown command: {}", part)),
        };
        cmds.push(Command { from, to, cmd });
    }
    Ok(cmds)
}

fn addr_matches(a: &Addr, n: usize, last: bool, line: &str) -> bool {
    match a {
        Addr::Line(x) => *x == n,
        Addr::Last => last,
        Addr::Pattern(p) => find(p, &line.chars().collect::<Vec<_>>(), 0).is_some(),
    }
}

fn main() -> i32 {
    let args = env::args();
    let mut quiet = false;
    let mut in_place = false;
    let mut script = String::new();
    let mut files: Vec<&str> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-n" => quiet = true,
            "-i" => in_place = true,
            "-e" => {
                i += 1;
                script.push_str(args.get(i).map_or("", |s| s.as_str()));
                script.push(';');
            }
            a if script.is_empty() => script = String::from(a),
            a => files.push(a),
        }
        i += 1;
    }
    let cmds = match parse(&script) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sed: {}", e);
            return 1;
        }
    };
    let targets: Vec<Vec<&str>> = if in_place { files.iter().map(|f| alloc::vec![*f]).collect() } else { alloc::vec![files.clone()] };
    for target in targets {
        let (lines, _) = input_lines(&target);
        let mut out = String::new();
        let mut active = alloc::vec![false; cmds.len()];
        'lines: for (idx, line) in lines.iter().enumerate() {
            let n = idx + 1;
            let last = n == lines.len();
            let mut text = line.clone();
            let mut deleted = false;
            for (ci, c) in cmds.iter().enumerate() {
                let selected = match (&c.from, &c.to) {
                    (None, _) => true,
                    (Some(a), None) => addr_matches(a, n, last, &text),
                    (Some(a), Some(b)) => {
                        if active[ci] {
                            if addr_matches(b, n, last, &text) || matches!(b, Addr::Line(x) if *x <= n) {
                                active[ci] = false;
                            }
                            true
                        } else if addr_matches(a, n, last, &text) {
                            active[ci] = !matches!(b, Addr::Line(x) if *x <= n);
                            true
                        } else {
                            false
                        }
                    }
                };
                if !selected {
                    continue;
                }
                match &c.cmd {
                    Cmd::Print => {
                        out.push_str(&text);
                        out.push('\n');
                    }
                    Cmd::Delete => {
                        deleted = true;
                        break;
                    }
                    Cmd::LineNumber => out.push_str(&format!("{}\n", n)),
                    Cmd::Quit => {
                        if !quiet {
                            out.push_str(&text);
                            out.push('\n');
                        }
                        break 'lines;
                    }
                    Cmd::Subst { pat, rep, global, print } => {
                        let chars: Vec<char> = text.chars().collect();
                        let mut result = String::new();
                        let mut pos = 0;
                        let mut changed = false;
                        while let Some((start, len)) = find(pat, &chars, pos) {
                            result.extend(&chars[pos..start]);
                            let matched: String = chars[start..start + len].iter().collect();
                            result.push_str(&rep.replace('&', &matched));
                            changed = true;
                            pos = start + len.max(1);
                            if len == 0 && start < chars.len() {
                                result.push(chars[start]);
                            }
                            if !global || pos > chars.len() {
                                break;
                            }
                        }
                        if pos <= chars.len() {
                            result.extend(&chars[pos.min(chars.len())..]);
                        }
                        if changed {
                            text = result;
                            if *print {
                                out.push_str(&text);
                                out.push('\n');
                            }
                        }
                    }
                }
            }
            if !deleted && !quiet {
                out.push_str(&text);
                out.push('\n');
            }
        }
        if in_place {
            if let Err(e) = fs::write(target[0], out.as_bytes()) {
                eprintln!("sed: {}: {}", target[0], e);
                return 1;
            }
        } else {
            print!("{}", out);
        }
    }
    0
}
