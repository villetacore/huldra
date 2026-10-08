#![no_std]
#![no_main]

use huldra_user::io::{write_all, STDOUT};
use huldra_user::{env, String};

huldra_user::main!(main);

fn main() -> i32 {
    let args = &env::args()[1..];
    let mut line = if args.is_empty() {
        String::from("y")
    } else {
        args.join(" ")
    };
    line.push('\n');
    let chunk = line.repeat(4096 / line.len() + 1);
    // Stops with EPIPE/SIGPIPE when the reader goes away.
    while write_all(STDOUT, chunk.as_bytes()).is_ok() {}
    0
}
