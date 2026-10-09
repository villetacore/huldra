//! man NAME: the same as help NAME (command help and documentation).

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, process, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let args = env::args();
    let mut argv: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    argv[0] = "help";
    let e = process::exec("/bin/help", &argv, &env::envp());
    eprintln!("man: /bin/help: {}", e);
    127
}
