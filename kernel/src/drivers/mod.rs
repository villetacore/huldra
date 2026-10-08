pub mod keyboard;
pub mod serial;
pub mod vga;

use crate::task::wait::WaitQueue;

/// Woken whenever keyboard or serial input arrives.
pub static INPUT: WaitQueue = WaitQueue::new();

pub fn input_ready() {
    INPUT.wake_all();
}

/// Blocks until a character arrives from the keyboard or the serial port.
pub fn read_char() -> u8 {
    INPUT.wait_uninterruptible(|| keyboard::read_char().or_else(serial::read_char))
}
