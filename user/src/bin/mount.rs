//! mount [-t type] source target   |   mount (list)

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, fs, print, sys, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let args = &env::args()[1..];
    if args.is_empty() {
        print!("{}", fs::read_to_string("/proc/mounts").unwrap_or_default());
        return 0;
    }
    let mut fstype = "ext2";
    let mut rest: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-t" && i + 1 < args.len() {
            fstype = &args[i + 1];
            i += 2;
        } else {
            rest.push(&args[i]);
            i += 1;
        }
    }
    if rest.len() != 2 {
        eprintln!("usage: mount [-t type] source target");
        return 2;
    }
    match sys::mount(rest[0], rest[1], fstype, 0) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("mount: {} on {}: {}", rest[0], rest[1], e);
            1
        }
    }
}
