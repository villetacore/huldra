//! echo [-n] [word...]: print the words separated by spaces; -n omits the
//! newline.

#![no_std]
#![no_main]

use huldra_user::{env, print, String};

huldra_user::main!(main);

fn main() -> i32 {
    let mut args = &env::args()[1..];
    let newline = args.first().map(String::as_str) != Some("-n");
    if !newline {
        args = &args[1..];
    }
    let mut first = true;
    for a in args {
        if !first {
            print!(" ");
        }
        first = false;
        print!("{}", a);
    }
    if newline {
        print!("\n");
    }
    0
}
