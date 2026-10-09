//! nl [file...]: print lines with line numbers.

#![no_std]
#![no_main]

use huldra_user::io::input_lines;
use huldra_user::{env, println, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let files: Vec<&str> = env::args()[1..].iter().map(|s| s.as_str()).collect();
    let (lines, ok) = input_lines(&files);
    let mut n = 0;
    for l in lines {
        if l.trim().is_empty() {
            println!();
        } else {
            n += 1;
            println!("{:>6}\t{}", n, l);
        }
    }
    (!ok) as i32
}
