//! CMOS real-time clock: read once at boot to get the wall-clock time.

use crate::arch::port::{inb, outb};

fn read_register(reg: u8) -> u8 {
    unsafe {
        outb(0x70, reg);
        inb(0x71)
    }
}

fn update_in_progress() -> bool {
    read_register(0x0A) & 0x80 != 0
}

fn bcd(v: u8) -> u8 {
    (v & 0x0F) + (v >> 4) * 10
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Seconds since the Unix epoch, read from the RTC.
pub fn read_unix_time() -> i64 {
    // Read twice until stable to avoid catching an update halfway.
    let read = || {
        while update_in_progress() {}
        [0x00, 0x02, 0x04, 0x07, 0x08, 0x09].map(read_register)
    };
    let mut r = read();
    loop {
        let again = read();
        if again == r {
            break;
        }
        r = again;
    }
    let status_b = read_register(0x0B);
    let [mut sec, mut min, mut hour, mut day, mut month, mut year] = r;
    if status_b & 0x04 == 0 {
        sec = bcd(sec);
        min = bcd(min);
        hour = bcd(hour & 0x7F) | (hour & 0x80);
        day = bcd(day);
        month = bcd(month);
        year = bcd(year);
    }
    if status_b & 0x02 == 0 && hour & 0x80 != 0 {
        hour = ((hour & 0x7F) + 12) % 24;
    }
    let days = days_from_civil(2000 + year as i64, month as i64, day as i64);
    days * 86_400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64
}
