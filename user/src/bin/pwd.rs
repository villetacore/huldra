#![no_std]
#![no_main]

use huldra_user::{eprintln, fs, println};

huldra_user::main!(main);

fn main() -> i32 {
    match fs::current_dir() {
        Ok(d) => {
            println!("{}", d);
            0
        }
        Err(e) => {
            eprintln!("pwd: {}", e);
            1
        }
    }
}
