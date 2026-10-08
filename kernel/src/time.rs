//! Kernel time keeping, driven by the periodic timer interrupt.

use core::sync::atomic::{AtomicI64, AtomicU64, Ordering};

pub const HZ: u64 = 100;

static TICKS: AtomicU64 = AtomicU64::new(0);
static BOOT_EPOCH: AtomicI64 = AtomicI64::new(0);

pub fn init() {
    BOOT_EPOCH.store(crate::drivers::rtc::read_unix_time(), Ordering::Relaxed);
}

/// Wall-clock time in seconds since the Unix epoch.
pub fn now() -> i64 {
    BOOT_EPOCH.load(Ordering::Relaxed) + (uptime_ms() / 1000) as i64
}

/// Wall-clock time as (seconds, nanoseconds).
pub fn now_precise() -> (i64, i64) {
    let ms = uptime_ms();
    (BOOT_EPOCH.load(Ordering::Relaxed) + (ms / 1000) as i64, ((ms % 1000) * 1_000_000) as i64)
}

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
