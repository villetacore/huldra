//! Time.

use crate::sys;
use huldra_abi::{Timespec, CLOCK_MONOTONIC, CLOCK_REALTIME};

pub fn sleep_ms(ms: u64) {
    let ts = Timespec { tv_sec: (ms / 1000) as i64, tv_nsec: ((ms % 1000) * 1_000_000) as i64 };
    let _ = sys::nanosleep(&ts);
}

/// Milliseconds since boot.
pub fn uptime_ms() -> u64 {
    sys::clock_gettime(CLOCK_MONOTONIC).map_or(0, |t| t.tv_sec as u64 * 1000 + t.tv_nsec as u64 / 1_000_000)
}

/// Seconds since the Unix epoch.
pub fn now() -> i64 {
    sys::clock_gettime(CLOCK_REALTIME).map_or(0, |t| t.tv_sec)
}

/// Broken-down UTC time.
#[derive(Clone, Copy, Debug)]
pub struct DateTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub weekday: u32,
}

impl DateTime {
    pub fn from_unix(t: i64) -> DateTime {
        let days = t.div_euclid(86_400);
        let secs = t.rem_euclid(86_400);
        // Civil-from-days (Howard Hinnant).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
        DateTime {
            year,
            month,
            day,
            hour: (secs / 3600) as u32,
            minute: (secs / 60 % 60) as u32,
            second: (secs % 60) as u32,
            weekday: (days + 4).rem_euclid(7) as u32,
        }
    }
}

pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
pub const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
