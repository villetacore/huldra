//! head [-n N | -c N] [file...]: print the first lines (or bytes) of files.

#![no_std]
#![no_main]

use huldra_user::io::{write_all, Reader, STDIN, STDOUT};
use huldra_user::{env, eprintln, println, sys, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let args = &env::args()[1..];
    let mut n = 10usize;
    let mut bytes: Option<usize> = None;
    let mut files: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-n" && i + 1 < args.len() {
            n = args[i + 1].parse().unwrap_or(10);
            i += 2;
        } else if args[i] == "-c" && i + 1 < args.len() {
            bytes = args[i + 1].parse().ok();
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
        if let Some(mut left) = bytes {
            let mut buf = [0u8; 4096];
            while left > 0 {
                match sys::read(fd, &mut buf[..left.min(4096)]) {
                    Ok(0) | Err(_) => break,
                    Ok(k) => {
                        let _ = write_all(STDOUT, &buf[..k]);
                        left -= k;
                    }
                }
            }
            continue;
        }
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
