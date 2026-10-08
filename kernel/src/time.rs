//! Kernel time keeping, driven by the periodic timer interrupt.

use core::sync::atomic::{AtomicU64, Ordering};

pub const HZ: u64 = 100;

static TICKS: AtomicU64 = AtomicU64::new(0);

/// Called from the timer interrupt.
pub fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn uptime_ms() -> u64 {
    ticks() * 1000 / HZ
}

pub fn ms_to_ticks(ms: u64) -> u64 {
    (ms * HZ).div_ceil(1000)
}
