//! tr [-d] [-s] SET1 [SET2]  (ranges like a-z, escapes \n \t)

#![no_std]
#![no_main]

use huldra_user::io::{read_input, write_all, STDOUT};
use huldra_user::{env, eprintln, String, Vec};

huldra_user::main!(main);

fn expand(set: &str) -> Vec<char> {
    let mut raw: Vec<char> = Vec::new();
    let mut it = set.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            raw.push(match it.next() {
                Some('n') => '\n',
                Some('t') => '\t',
                Some('r') => '\r',
                Some('\\') | None => '\\',
                Some(o) => o,
            });
        } else {
            raw.push(c);
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        if i + 2 < raw.len() && raw[i + 1] == '-' && raw[i] <= raw[i + 2] {
            for c in raw[i]..=raw[i + 2] {
                out.push(c);
            }
            i += 3;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    match set {
        "[:upper:]" => ('A'..='Z').collect(),
        "[:lower:]" => ('a'..='z').collect(),
        "[:digit:]" => ('0'..='9').collect(),
        "[:space:]" => Vec::from([' ', '\t', '\n', '\r']),
        _ => out,
    }
}

fn main() -> i32 {
    let mut delete = false;
    let mut squeeze = false;
    let mut sets: Vec<&str> = Vec::new();
    for a in &env::args()[1..] {
        match a.as_str() {
            "-d" => delete = true,
            "-s" => squeeze = true,
            "-ds" | "-sd" => {
                delete = true;
                squeeze = true;
            }
            s => sets.push(s),
        }
    }
    if sets.is_empty() || (!delete && !squeeze && sets.len() < 2) {
        eprintln!("usage: tr [-d] [-s] SET1 [SET2]");
        return 2;
    }
    let s1 = expand(sets[0]);
    let s2 = sets.get(1).map(|s| expand(s)).unwrap_or_default();
    let input = String::from_utf8_lossy(&read_input("-").unwrap_or_default()).into_owned();
    let mut out = String::new();
    let mut last: Option<char> = None;
    for c in input.chars() {
        let mapped = if delete && s1.contains(&c) {
            continue;
        } else if !delete && !s2.is_empty() {
            match s1.iter().position(|&x| x == c) {
                Some(i) => *s2.get(i).unwrap_or(s2.last().unwrap()),
                None => c,
            }
        } else {
            c
        };
        let squeeze_set = if delete { &s2 } else if s2.is_empty() { &s1 } else { &s2 };
        if squeeze && last == Some(mapped) && squeeze_set.contains(&mapped) {
            continue;
        }
        out.push(mapped);
        last = Some(mapped);
    }
    let _ = write_all(STDOUT, out.as_bytes());
    0
}
