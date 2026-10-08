//! more: alias for less.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, process, String, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let mut args: Vec<&str> = Vec::from(["less"]);
    args.extend(env::args()[1..].iter().map(String::as_str));
    let e = process::exec("/bin/less", &args, &env::envp());
    eprintln!("more: {}", e);
    1
}
