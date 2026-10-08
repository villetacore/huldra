#![no_std]
#![no_main]

use huldra_user::io::{write_all, STDOUT};
use huldra_user::{eprintln, fs};

huldra_user::main!(main);

fn main() -> i32 {
    match fs::read("/proc/kmsg") {
        Ok(log) => {
            let _ = write_all(STDOUT, &log);
            0
        }
        Err(e) => {
            eprintln!("dmesg: {}", e);
            1
        }
    }
}
