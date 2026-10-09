//! find [path...] [-name PATTERN] [-type f|d] [-maxdepth N] [-size +N|-N[k]]
//! [-newer FILE] [-delete] [-exec CMD {} ;]: search for files in a directory
//! tree.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use huldra_user::abi::fs::*;
use huldra_user::{env, eprintln, fs, println, process};

huldra_user::main!(main);

struct Filters {
    name: Option<Vec<char>>,
    kind: Option<char>,
    max_depth: usize,
    size: Option<(char, i64)>,
    newer: Option<i64>,
    delete: bool,
    exec: Option<Vec<String>>,
}

/// Glob matching for -name (`*`, `?`).
fn glob(p: &[char], t: &[char]) -> bool {
    match (p.first(), t.first()) {
        (None, None) => true,
        (Some('*'), _) => glob(&p[1..], t) || (!t.is_empty() && glob(p, &t[1..])),
        (Some('?'), Some(_)) => glob(&p[1..], &t[1..]),
        (Some(a), Some(b)) if a == b => glob(&p[1..], &t[1..]),
        _ => false,
    }
}

fn matches(f: &Filters, path: &str, st: &Stat) -> bool {
    if let Some(pat) = &f.name {
        let base: Vec<char> = path.rsplit('/').next().unwrap_or(path).chars().collect();
        if !glob(pat, &base) {
            return false;
        }
    }
    if let Some(k) = f.kind {
        let is_dir = fs::is_dir(st);
        if (k == 'd') != is_dir || (k == 'f' && st.st_mode & S_IFMT != S_IFREG) {
            return false;
        }
    }
    if let Some((op, n)) = f.size {
        let ok = match op {
            '+' => st.st_size > n,
            '-' => st.st_size < n,
            _ => st.st_size == n,
        };
        if !ok {
            return false;
        }
    }
    if let Some(t) = f.newer {
        if st.st_mtime <= t {
            return false;
        }
    }
    true
}

fn visit(f: &Filters, path: &str, depth: usize, status: &mut i32) {
    let st = match fs::metadata(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("find: {}: {}", path, e);
            *status = 1;
            return;
        }
    };
    if fs::is_dir(&st) && depth < f.max_depth {
        match fs::read_dir(path) {
            Ok(entries) => {
                for e in entries {
                    visit(f, &fs::join(path, &e.name), depth + 1, status);
                }
            }
            Err(e) => {
                eprintln!("find: {}: {}", path, e);
                *status = 1;
            }
        }
    }
    if !matches(f, path, &st) {
        return;
    }
    if let Some(cmd) = &f.exec {
        let args: Vec<String> = cmd.iter().map(|a| a.replace("{}", path)).collect();
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        match process::find_in_path(argv[0]) {
            Some(p) => {
                let _ = process::run(&p, &argv);
            }
            None => eprintln!("find: {}: command not found", argv[0]),
        }
    } else if f.delete {
        if let Err(e) = fs::remove_all(path) {
            eprintln!("find: cannot delete {}: {}", path, e);
            *status = 1;
        }
    } else {
        println!("{}", path);
    }
}

fn main() -> i32 {
    let args = env::args();
    let mut paths: Vec<String> = Vec::new();
    let mut f = Filters { name: None, kind: None, max_depth: usize::MAX, size: None, newer: None, delete: false, exec: None };
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        let next = args.get(i + 1).cloned();
        match a {
            "-name" => {
                f.name = next.map(|s| s.chars().collect());
                i += 1;
            }
            "-type" => {
                f.kind = next.and_then(|s| s.chars().next());
                i += 1;
            }
            "-maxdepth" => {
                f.max_depth = next.and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
                i += 1;
            }
            "-size" => {
                if let Some(s) = next {
                    let (op, rest) = match s.chars().next() {
                        Some(c @ ('+' | '-')) => (c, &s[1..]),
                        _ => ('=', s.as_str()),
                    };
                    let (num, mult) = match rest.strip_suffix('k') {
                        Some(n) => (n, 1024),
                        None => match rest.strip_suffix('M') {
                            Some(n) => (n, 1 << 20),
                            None => (rest.strip_suffix('c').unwrap_or(rest), 1),
                        },
                    };
                    f.size = num.parse::<i64>().ok().map(|n| (op, n * mult));
                }
                i += 1;
            }
            "-newer" => {
                f.newer = next.and_then(|p| fs::metadata(&p).ok()).map(|s| s.st_mtime);
                i += 1;
            }
            "-delete" => f.delete = true,
            "-print" => {}
            "-exec" => {
                let mut cmd = Vec::new();
                i += 1;
                while i < args.len() && args[i] != ";" && args[i] != "+" {
                    cmd.push(args[i].clone());
                    i += 1;
                }
                f.exec = Some(cmd);
            }
            _ if a.starts_with('-') => {
                eprintln!("find: unknown option {}", a);
                return 2;
            }
            _ => paths.push(String::from(a)),
        }
        i += 1;
    }
    if paths.is_empty() {
        paths.push(String::from("."));
    }
    let mut status = 0;
    for p in &paths {
        visit(&f, p, 0, &mut status);
    }
    status
}
