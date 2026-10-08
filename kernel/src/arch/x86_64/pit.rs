//! 8253/8254 Programmable Interval Timer, used as the periodic tick
//! (until the local APIC timer takes over) and for calibration.

use super::port::outb;
use crate::time::HZ;

const BASE_FREQUENCY: u64 = 1_193_182;

pub fn init() {
    let divisor = (BASE_FREQUENCY / HZ) as u16;
    unsafe {
        outb(0x43, 0x36); // channel 0, lo/hi byte, square wave
        outb(0x40, divisor as u8);
        outb(0x40, (divisor >> 8) as u8);
    }
}
