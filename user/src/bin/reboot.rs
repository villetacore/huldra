//! reboot: flush the disks and restart the machine.

#![no_std]
#![no_main]

use huldra_user::abi::LINUX_REBOOT_CMD_RESTART;
use huldra_user::{eprintln, sys};

huldra_user::main!(main);

fn main() -> i32 {
    sys::sync();
    let e = sys::reboot(LINUX_REBOOT_CMD_RESTART).err();
    eprintln!("reboot: {:?}", e);
    1
}
