//! wc [file...]: lines, words, bytes

#![no_std]
#![no_main]

use huldra_user::io::{Reader, STDIN};
use huldra_user::{env, eprintln, println, sys};

huldra_user::main!(main);

fn count(fd: i32) -> (usize, usize, usize) {
    let data = Reader::new(fd).read_to_end().unwrap_or_default();
    let lines = data.iter().filter(|&&b| b == b'\n').count();
    let words = data
        .split(|b| b.is_ascii_whitespace())
        .filter(|w| !w.is_empty())
        .count();
    (lines, words, data.len())
}

fn main() -> i32 {
    let files = &env::args()[1..];
    if files.is_empty() {
        let (l, w, b) = count(STDIN);
        println!("{:>7} {:>7} {:>7}", l, w, b);
        return 0;
    }
    let mut status = 0;
    let mut total = (0, 0, 0);
    for f in files {
        match sys::open(f, 0, 0) {
            Ok(fd) => {
                let (l, w, b) = count(fd);
                let _ = sys::close(fd);
                total = (total.0 + l, total.1 + w, total.2 + b);
                println!("{:>7} {:>7} {:>7} {}", l, w, b, f);
            }
            Err(e) => {
                eprintln!("wc: {}: {}", f, e);
                status = 1;
            }
        }
    }
    if files.len() > 1 {
        println!("{:>7} {:>7} {:>7} total", total.0, total.1, total.2);
    }
    status
}
