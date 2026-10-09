//! poweroff: flush the disks and turn the machine off.

#![no_std]
#![no_main]

use huldra_user::abi::LINUX_REBOOT_CMD_POWER_OFF;
use huldra_user::{eprintln, println, sys};

huldra_user::main!(main);

fn main() -> i32 {
    println!("System is going down.");
    sys::sync();
    let e = sys::reboot(LINUX_REBOOT_CMD_POWER_OFF).err();
    eprintln!("poweroff: {:?}", e);
    1
}
