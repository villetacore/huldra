//! cut -d SEP -f LIST [file...]  |  cut -c LIST [file...]
//! LIST: N, N-M, N-, -M, comma separated.

#![no_std]
#![no_main]

use huldra_user::io::input_lines;
use huldra_user::{env, eprintln, println, String, Vec};

huldra_user::main!(main);

fn parse_list(s: &str) -> Option<Vec<(usize, usize)>> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (if a.is_empty() { 1 } else { a.parse().ok()? }, if b.is_empty() { usize::MAX } else { b.parse().ok()? }),
            None => {
                let n = part.parse().ok()?;
                (n, n)
            }
        };
        if a == 0 {
            return None;
        }
        out.push((a, b));
    }
    Some(out)
}

fn selected(ranges: &[(usize, usize)], i: usize) -> bool {
    ranges.iter().any(|&(a, b)| i >= a && i <= b)
}

fn main() -> i32 {
    let args = env::args();
    let mut delim = '\t';
    let mut fields = None;
    let mut chars = None;
    let mut files: Vec<&str> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        let value = |flag: &str| -> Option<String> {
            if a.len() > 2 { Some(String::from(&a[2..])) } else { args.get(i + 1).cloned().filter(|_| a == flag) }
        };
        if a.starts_with("-d") {
            let v = value("-d").unwrap_or_default();
            if a.len() == 2 {
                i += 1;
            }
            delim = v.chars().next().unwrap_or('\t');
        } else if a.starts_with("-f") {
            fields = value("-f").and_then(|v| parse_list(&v));
            if a.len() == 2 {
                i += 1;
            }
        } else if a.starts_with("-c") {
            chars = value("-c").and_then(|v| parse_list(&v));
            if a.len() == 2 {
                i += 1;
            }
        } else {
            files.push(a);
        }
        i += 1;
    }
    if fields.is_none() && chars.is_none() {
        eprintln!("usage: cut -d SEP -f LIST [file...] | cut -c LIST [file...]");
        return 2;
    }
    let (lines, ok) = input_lines(&files);
    for l in lines {
        if let Some(r) = &chars {
            let s: String = l.chars().enumerate().filter(|(i, _)| selected(r, i + 1)).map(|(_, c)| c).collect();
            println!("{}", s);
        } else if let Some(r) = &fields {
            if !l.contains(delim) {
                println!("{}", l);
                continue;
            }
            let parts: Vec<&str> = l.split(delim).enumerate().filter(|(i, _)| selected(r, i + 1)).map(|(_, p)| p).collect();
            let mut sep = [0u8; 4];
            println!("{}", parts.join(delim.encode_utf8(&mut sep)));
        }
    }
    (!ok) as i32
}
