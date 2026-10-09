//! sync: write cached data to the disks.

#![no_std]
#![no_main]

huldra_user::main!(main);

fn main() -> i32 {
    huldra_user::sys::sync();
    0
}
