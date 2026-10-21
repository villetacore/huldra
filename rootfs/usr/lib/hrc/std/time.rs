//! Time: `Duration`, `Instant` and `SystemTime`.

use crate::fmt;
use crate::option::Option::{self, None, Some};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Duration {
    secs: u64,
    nanos: u32,
}

impl Duration {
    pub const ZERO: Duration = Duration { secs: 0, nanos: 0 };
    pub fn new(secs: u64, nanos: u32) -> Duration {
        Duration {
            secs: secs + (nanos / 1_000_000_000) as u64,
            nanos: nanos % 1_000_000_000,
        }
    }
    pub fn from_secs(secs: u64) -> Duration {
        Duration { secs, nanos: 0 }
    }
    pub fn from_millis(ms: u64) -> Duration {
        Duration::new(ms / 1000, ((ms % 1000) * 1_000_000) as u32)
    }
    pub fn from_micros(us: u64) -> Duration {
        Duration::new(us / 1_000_000, ((us % 1_000_000) * 1000) as u32)
    }
    pub fn from_nanos(ns: u64) -> Duration {
        Duration::new(ns / 1_000_000_000, (ns % 1_000_000_000) as u32)
    }
    pub fn from_secs_f64(s: f64) -> Duration {
        let secs = s as u64;
        Duration::new(secs, ((s - secs as f64) * 1e9) as u32)
    }
    pub fn as_secs(&self) -> u64 {
        self.secs
    }
    pub fn subsec_millis(&self) -> u32 {
        self.nanos / 1_000_000
    }
    pub fn subsec_micros(&self) -> u32 {
        self.nanos / 1000
    }
    pub fn subsec_nanos(&self) -> u32 {
        self.nanos
    }
    pub fn as_millis(&self) -> u64 {
        self.secs * 1000 + (self.nanos / 1_000_000) as u64
    }
    pub fn as_micros(&self) -> u64 {
        self.secs * 1_000_000 + (self.nanos / 1000) as u64
    }
    pub fn as_nanos(&self) -> u64 {
        self.secs * 1_000_000_000 + self.nanos as u64
    }
    pub fn as_secs_f64(&self) -> f64 {
        self.secs as f64 + self.nanos as f64 / 1e9
    }
    pub fn as_secs_f32(&self) -> f32 {
        self.as_secs_f64() as f32
    }
    pub fn is_zero(&self) -> bool {
        self.secs == 0 && self.nanos == 0
    }
    pub fn saturating_sub(self, o: Duration) -> Duration {
        if self < o {
            Duration::ZERO
        } else {
            self - o
        }
    }
    pub fn checked_sub(self, o: Duration) -> Option<Duration> {
        if self < o {
            None
        } else {
            Some(self - o)
        }
    }
}

impl crate::ops::Add for Duration {
    type Output = Duration;
    fn add(self, o: Duration) -> Duration {
        Duration::new(self.secs + o.secs, self.nanos + o.nanos)
    }
}

impl crate::ops::Sub for Duration {
    type Output = Duration;
    fn sub(self, o: Duration) -> Duration {
        if self.nanos >= o.nanos {
            Duration::new(self.secs - o.secs, self.nanos - o.nanos)
        } else {
            Duration::new(self.secs - o.secs - 1, self.nanos + 1_000_000_000 - o.nanos)
        }
    }
}

impl crate::ops::AddAssign for Duration {
    fn add_assign(&mut self, o: Duration) {
        *self = *self + o;
    }
}

impl crate::ops::Mul<u32> for Duration {
    type Output = Duration;
    fn mul(self, n: u32) -> Duration {
        Duration::from_nanos(self.as_nanos() * n as u64)
    }
}

impl crate::ops::Div<u32> for Duration {
    type Output = Duration;
    fn div(self, n: u32) -> Duration {
        Duration::from_nanos(self.as_nanos() / n as u64)
    }
}

impl fmt::Debug for Duration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.secs > 0 {
            let s = self.secs as f64 + self.nanos as f64 / 1e9;
            write!(f, "{}s", s)
        } else if self.nanos >= 1_000_000 {
            write!(f, "{}ms", self.nanos as f64 / 1e6)
        } else if self.nanos >= 1000 {
            write!(f, "{}µs", self.nanos as f64 / 1e3)
        } else {
            write!(f, "{}ns", self.nanos)
        }
    }
}

#[derive(Clone, Copy)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

extern "C" {
    fn clock_gettime(id: i32, ts: *mut Timespec) -> i32;
    fn nanosleep(req: *const Timespec, rem: *mut Timespec) -> i32;
}

fn now(clock: i32) -> Duration {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    unsafe {
        clock_gettime(clock, &mut ts as *mut Timespec);
    }
    Duration::new(ts.sec as u64, ts.nsec as u32)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Instant(Duration);

impl Instant {
    pub fn now() -> Instant {
        Instant(now(1))
    }
    pub fn elapsed(&self) -> Duration {
        Instant::now().0.saturating_sub(self.0)
    }
    pub fn duration_since(&self, earlier: Instant) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

impl crate::ops::Sub for Instant {
    type Output = Duration;
    fn sub(self, o: Instant) -> Duration {
        self.0.saturating_sub(o.0)
    }
}

impl crate::ops::Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, d: Duration) -> Instant {
        Instant(self.0 + d)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SystemTime(Duration);

pub const UNIX_EPOCH: SystemTime = SystemTime(Duration { secs: 0, nanos: 0 });

#[derive(Clone, Debug)]
pub struct SystemTimeError;

impl SystemTime {
    pub const UNIX_EPOCH: SystemTime = SystemTime(Duration { secs: 0, nanos: 0 });
    pub fn now() -> SystemTime {
        SystemTime(now(0))
    }
    pub fn duration_since(&self, earlier: SystemTime) -> Result<Duration, SystemTimeError> {
        if self.0 < earlier.0 {
            Err(SystemTimeError)
        } else {
            Ok(self.0 - earlier.0)
        }
    }
    pub fn elapsed(&self) -> Result<Duration, SystemTimeError> {
        SystemTime::now().duration_since(*self)
    }
}

pub fn sleep(d: Duration) {
    let ts = Timespec {
        sec: d.secs as i64,
        nsec: d.nanos as i64,
    };
    let mut rem = Timespec { sec: 0, nsec: 0 };
    unsafe {
        nanosleep(&ts as *const Timespec, &mut rem as *mut Timespec);
    }
}
