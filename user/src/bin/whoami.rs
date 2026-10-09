//! whoami: print the user name.

#![no_std]
#![no_main]

huldra_user::main!(main);

fn main() -> i32 {
    // Everything runs as root until users exist.
    huldra_user::println!("root");
    0
}
