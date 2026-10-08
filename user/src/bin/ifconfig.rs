//! ifconfig: shows the network interfaces (from /proc/net/if).

#![no_std]
#![no_main]

use huldra_user::{eprintln, fs, print};

huldra_user::main!(main);

fn main() -> i32 {
    match fs::read_to_string("/proc/net/if") {
        Ok(text) => {
            print!("{}", text);
            0
        }
        Err(e) => {
            eprintln!("ifconfig: /proc/net/if: {}", e);
            1
        }
    }
}
