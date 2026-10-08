//! grep [-i] [-v] [-n] [-c] PATTERN [file...] (fixed-string matching)

#![no_std]
#![no_main]

use huldra_user::io::{Reader, STDIN};
use huldra_user::{env, eprintln, println, sys, String, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let args = &env::args()[1..];
    let (mut ignore_case, mut invert, mut numbers, mut count_only) = (false, false, false, false);
    let mut rest: Vec<&String> = Vec::new();
    for a in args {
        match a.as_str() {
            "-i" => ignore_case = true,
            "-v" => invert = true,
            "-n" => numbers = true,
            "-c" => count_only = true,
            _ => rest.push(a),
        }
    }
    let Some(pattern) = rest.first() else {
        eprintln!("usage: grep [-i] [-v] [-n] [-c] PATTERN [file...]");
        return 2;
    };
    let pattern = if ignore_case { pattern.to_lowercase() } else { String::from(pattern.as_str()) };
    let files: Vec<&str> = if rest.len() > 1 { rest[1..].iter().map(|s| s.as_str()).collect() } else { Vec::from(["-"]) };
    let show_name = files.len() > 1;
    let mut found = false;
    for f in &files {
        let fd = if *f == "-" {
            STDIN
        } else {
            match sys::open(f, 0, 0) {
                Ok(fd) => fd,
                Err(e) => {
                    eprintln!("grep: {}: {}", f, e);
                    continue;
                }
            }
        };
        let mut r = Reader::new(fd);
        let mut count = 0;
        let mut n = 0;
        while let Ok(Some(line)) = r.read_line() {
            n += 1;
            let hay = if ignore_case { line.to_lowercase() } else { line.clone() };
            if hay.contains(pattern.as_str()) != invert {
                count += 1;
                found = true;
                if !count_only {
                    let prefix = if show_name { huldra_user::format!("{}:", f) } else { String::new() };
                    if numbers {
                        println!("{}{}:{}", prefix, n, line);
                    } else {
                        println!("{}{}", prefix, line);
                    }
                }
            }
        }
        if count_only {
            println!("{}", count);
        }
    }
    if found {
        0
    } else {
        1
    }
}
