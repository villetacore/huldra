#![no_std]
#![no_main]

use huldra_user::io::input_lines;
use huldra_user::{env, println, String, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let files: Vec<&str> = env::args()[1..].iter().map(|s| s.as_str()).collect();
    let (lines, ok) = input_lines(&files);
    for l in lines {
        println!("{}", l.chars().rev().collect::<String>());
    }
    (!ok) as i32
}
