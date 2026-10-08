//! sort [-r] [-n] [-u] [-f] [-k N] [-t SEP] [file...]

#![no_std]
#![no_main]

use core::cmp::Ordering;
use huldra_user::io::input_lines;
use huldra_user::{env, println, String, Vec};

huldra_user::main!(main);

fn key<'a>(line: &'a str, field: Option<usize>, sep: Option<char>) -> &'a str {
    match field {
        None => line,
        Some(k) => {
            let fields: Vec<&str> = match sep {
                Some(c) => line.split(c).collect(),
                None => line.split_whitespace().collect(),
            };
            fields.get(k - 1).copied().unwrap_or("")
        }
    }
}

fn number(s: &str) -> f64 {
    let s = s.trim();
    let end = s.char_indices().find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || (i == 0 && c == '-'))).map_or(s.len(), |(i, _)| i);
    s[..end].parse().unwrap_or(0.0)
}

fn main() -> i32 {
    let args = env::args();
    let (mut reverse, mut numeric, mut unique, mut fold) = (false, false, false, false);
    let mut field = None;
    let mut sep = None;
    let mut files: Vec<&str> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-k" => {
                i += 1;
                field = args.get(i).and_then(|v| v.split(',').next()?.parse().ok());
            }
            "-t" => {
                i += 1;
                sep = args.get(i).and_then(|v| v.chars().next());
            }
            _ if a.starts_with('-') && a.len() > 1 => {
                for c in a[1..].chars() {
                    match c {
                        'r' => reverse = true,
                        'n' => numeric = true,
                        'u' => unique = true,
                        'f' => fold = true,
                        _ => {}
                    }
                }
            }
            _ => files.push(a),
        }
        i += 1;
    }
    let (mut lines, ok) = input_lines(&files);
    let cmp = |a: &String, b: &String| -> Ordering {
        let (ka, kb) = (key(a, field, sep), key(b, field, sep));
        let o = if numeric {
            number(ka).partial_cmp(&number(kb)).unwrap_or(Ordering::Equal)
        } else if fold {
            ka.to_lowercase().cmp(&kb.to_lowercase())
        } else {
            ka.cmp(kb)
        };
        o.then_with(|| a.cmp(b))
    };
    lines.sort_by(cmp);
    if reverse {
        lines.reverse();
    }
    if unique {
        lines.dedup_by(|a, b| cmp(a, b) == Ordering::Equal || (field.is_some() && key(a, field, sep) == key(b, field, sep)));
    }
    for l in lines {
        println!("{}", l);
    }
    if ok {
        0
    } else {
        2
    }
}
