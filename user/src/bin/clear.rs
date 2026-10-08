#![no_std]
#![no_main]

huldra_user::main!(main);

fn main() -> i32 {
    huldra_user::print!("\x1b[2J\x1b[H");
    0
}
