#![no_std]
#![no_main]

use huldra_user::{env, println};

huldra_user::main!(main);

fn main() -> i32 {
    for (k, v) in env::vars() {
        println!("{}={}", k, v);
    }
    0
}
