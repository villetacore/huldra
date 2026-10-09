//! touch FILE...: create empty files (existing files are left alone).

#![no_std]
#![no_main]

use huldra_user::abi::fs::{O_CREAT, O_WRONLY};
use huldra_user::{env, eprintln, sys};

huldra_user::main!(main);

fn main() -> i32 {
    let mut status = 0;
    for path in &env::args()[1..] {
        match sys::open(path, O_WRONLY | O_CREAT, 0o644) {
            Ok(fd) => {
                let _ = sys::close(fd);
            }
            Err(e) => {
                eprintln!("touch: {}: {}", path, e);
                status = 1;
            }
        }
    }
    status
}
