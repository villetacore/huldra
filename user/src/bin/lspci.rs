#![no_std]
#![no_main]

use huldra_user::{eprintln, fs, print};

huldra_user::main!(main);

fn main() -> i32 {
    match fs::read_to_string("/proc/pci") {
        Ok(s) => {
            print!("{}", s);
            0
        }
        Err(e) => {
            eprintln!("lspci: {}", e);
            1
        }
    }
}
