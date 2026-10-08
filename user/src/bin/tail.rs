//! tail [-n N] [file]

#![no_std]
#![no_main]

use huldra_user::io::{Reader, STDIN};
use huldra_user::{env, eprintln, println, sys, String};
use huldra_user::Vec;

huldra_user::main!(main);

fn main() -> i32 {
    let args = &env::args()[1..];
    let mut n = 10usize;
    let mut file = "-";
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-n" && i + 1 < args.len() {
            n = args[i + 1].parse().unwrap_or(10);
            i += 2;
        } else {
            file = &args[i];
            i += 1;
        }
    }
    let fd = if file == "-" {
        STDIN
    } else {
        match sys::open(file, 0, 0) {
            Ok(fd) => fd,
            Err(e) => {
                eprintln!("tail: {}: {}", file, e);
                return 1;
            }
        }
    };
    let mut lines: Vec<String> = Vec::new();
    let mut r = Reader::new(fd);
    while let Ok(Some(line)) = r.read_line() {
        lines.push(line);
        if lines.len() > n {
            lines.remove(0);
        }
    }
    for l in lines {
        println!("{}", l);
    }
    0
}
