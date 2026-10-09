//! rmdir DIR...: remove empty directories.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, fs};

huldra_user::main!(main);

fn main() -> i32 {
    let mut status = 0;
    for dir in &env::args()[1..] {
        if let Err(e) = fs::remove_dir(dir) {
            eprintln!("rmdir: {}: {}", dir, e);
            status = 1;
        }
    }
    status
}
