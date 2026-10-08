//! 8253/8254 Programmable Interval Timer: the system tick.

use super::port::outb;
use core::sync::atomic::{AtomicU64, Ordering};

pub const HZ: u64 = 100;
const BASE_FREQUENCY: u64 = 1_193_182;

static TICKS: AtomicU64 = AtomicU64::new(0);

pub fn init() {
    let divisor = (BASE_FREQUENCY / HZ) as u16;
    unsafe {
        outb(0x43, 0x36); // channel 0, lo/hi byte, square wave
        outb(0x40, divisor as u8);
        outb(0x40, (divisor >> 8) as u8);
    }
}

pub fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn uptime_ms() -> u64 {
    ticks() * 1000 / HZ
}

#[allow(dead_code)]
pub fn sleep_ms(ms: u64) {
    let target = ticks() + (ms * HZ).div_ceil(1000);
    while ticks() < target {
        super::wait_for_interrupt();
    }
}
