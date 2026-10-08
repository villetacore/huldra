//! head [-n N] [file...]

#![no_std]
#![no_main]

use huldra_user::io::{Reader, STDIN};
use huldra_user::{env, eprintln, println, sys, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let args = &env::args()[1..];
    let mut n = 10usize;
    let mut files: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-n" && i + 1 < args.len() {
            n = args[i + 1].parse().unwrap_or(10);
            i += 2;
        } else if let Some(v) = args[i].strip_prefix('-').and_then(|v| v.parse().ok()) {
            n = v;
            i += 1;
        } else {
            files.push(&args[i]);
            i += 1;
        }
    }
    if files.is_empty() {
        files.push("-");
    }
    let mut status = 0;
    for f in files {
        let fd = if f == "-" {
            STDIN
        } else {
            match sys::open(f, 0, 0) {
                Ok(fd) => fd,
                Err(e) => {
                    eprintln!("head: {}: {}", f, e);
                    status = 1;
                    continue;
                }
            }
        };
        let mut r = Reader::new(fd);
        for _ in 0..n {
            match r.read_line() {
                Ok(Some(line)) => println!("{}", line),
                _ => break,
            }
        }
    }
    status
}
