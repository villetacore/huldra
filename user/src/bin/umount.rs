//! umount DIR...: unmount file systems.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, sys};

huldra_user::main!(main);

fn main() -> i32 {
    let mut status = 0;
    for target in &env::args()[1..] {
        if let Err(e) = sys::umount(target) {
            eprintln!("umount: {}: {}", target, e);
            status = 1;
        }
    }
    status
}
