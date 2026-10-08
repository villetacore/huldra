//! Kernel/user ABI. Numbers and layouts follow Linux x86_64 so that the
//! same conventions (and, eventually, static musl binaries) work.

#![no_std]

pub mod errno;
pub mod fs;
pub mod mm;
pub mod net;
pub mod process;
pub mod signal;
pub mod syscall;
pub mod termios;

/// `struct timespec`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

/// `struct utsname`
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Utsname {
    pub sysname: [u8; 65],
    pub nodename: [u8; 65],
    pub release: [u8; 65],
    pub version: [u8; 65],
    pub machine: [u8; 65],
    pub domainname: [u8; 65],
}

impl Default for Utsname {
    fn default() -> Self {
        Utsname {
            sysname: [0; 65],
            nodename: [0; 65],
            release: [0; 65],
            version: [0; 65],
            machine: [0; 65],
            domainname: [0; 65],
        }
    }
}

pub const CLOCK_REALTIME: i32 = 0;
pub const CLOCK_MONOTONIC: i32 = 1;

pub const LINUX_REBOOT_MAGIC1: u32 = 0xFEE1_DEAD;
pub const LINUX_REBOOT_MAGIC2: u32 = 672_274_793;
pub const LINUX_REBOOT_CMD_RESTART: u32 = 0x0123_4567;
pub const LINUX_REBOOT_CMD_HALT: u32 = 0xCDEF_0123;
pub const LINUX_REBOOT_CMD_POWER_OFF: u32 = 0x4321_FEDC;

/// Writes `s` into a NUL-padded fixed buffer (for `Utsname` fields).
pub fn copy_cstr(dst: &mut [u8], s: &str) {
    let n = s.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&s.as_bytes()[..n]);
    dst[n..].fill(0);
}
