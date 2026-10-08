#![no_std]
#![no_main]

use huldra_user::{env, eprintln, fs};

huldra_user::main!(main);

fn main() -> i32 {
    let args = &env::args()[1..];
    let parents = args.iter().any(|a| a == "-p");
    let mut status = 0;
    for dir in args.iter().filter(|a| !a.starts_with('-')) {
        let r = if parents { fs::create_dir_all(dir) } else { fs::create_dir(dir) };
        if let Err(e) = r {
            eprintln!("mkdir: {}: {}", dir, e);
            status = 1;
        }
    }
    status
}
