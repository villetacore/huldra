//! which NAME...: print the path of each command found in PATH.

#![no_std]
#![no_main]

use huldra_user::{env, println, process};

huldra_user::main!(main);

fn main() -> i32 {
    let mut status = 0;
    for name in &env::args()[1..] {
        match process::find_in_path(name) {
            Some(p) => println!("{}", p),
            None => status = 1,
        }
    }
    status
}
