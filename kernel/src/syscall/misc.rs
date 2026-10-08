//! Miscellaneous system calls: time, system information, reboot.

use super::{value, Args, Ret};
use crate::fs::KResult;
use crate::proc::uaccess;
use crate::task::sched;
use huldra_abi::errno::Errno;
use huldra_abi::*;

pub fn uname(a: &mut Args) -> KResult<Ret> {
    let mut u = Utsname::default();
    let host = crate::fs::read_file("/etc/hostname").unwrap_or_default();
    let host = core::str::from_utf8(&host).unwrap_or("").trim();
    copy_cstr(&mut u.sysname, crate::NAME);
    copy_cstr(&mut u.nodename, if host.is_empty() { "localhost" } else { host });
    copy_cstr(&mut u.release, crate::VERSION);
    copy_cstr(&mut u.version, "#1 SMP");
    copy_cstr(&mut u.machine, "x86_64");
    uaccess::write_user(a.a0(), &u)?;
    value(0)
}

pub fn clock_gettime(a: &mut Args) -> KResult<Ret> {
    let ts = match a.a0() as i32 {
        CLOCK_REALTIME => {
            let (s, ns) = crate::time::now_precise();
            Timespec { tv_sec: s, tv_nsec: ns }
        }
        CLOCK_MONOTONIC | 4 /* MONOTONIC_RAW */ | 7 /* BOOTTIME */ => {
            let ms = crate::time::uptime_ms();
            Timespec { tv_sec: (ms / 1000) as i64, tv_nsec: ((ms % 1000) * 1_000_000) as i64 }
        }
        _ => return Err(Errno::EINVAL),
    };
    uaccess::write_user(a.a1(), &ts)?;
    value(0)
}

pub fn nanosleep(a: &mut Args) -> KResult<Ret> {
    let req: Timespec = uaccess::read_user(a.a0())?;
    if req.tv_sec < 0 || !(0..1_000_000_000).contains(&req.tv_nsec) {
        return Err(Errno::EINVAL);
    }
    let ms = req.tv_sec as u64 * 1000 + (req.tv_nsec as u64).div_ceil(1_000_000);
    let target = crate::time::ticks() + crate::time::ms_to_ticks(ms);
    let r = sched::SLEEPERS.wait_until(|| (crate::time::ticks() >= target).then_some(()));
    if r.is_err() && a.a1() != 0 {
        let left_ms = target.saturating_sub(crate::time::ticks()) * 1000 / crate::time::HZ;
        let rem = Timespec { tv_sec: (left_ms / 1000) as i64, tv_nsec: ((left_ms % 1000) * 1_000_000) as i64 };
        uaccess::write_user(a.a1(), &rem)?;
    }
    r?;
    value(0)
}

pub fn reboot(a: &mut Args) -> KResult<Ret> {
    if a.a0() as u32 != LINUX_REBOOT_MAGIC1 || a.a1() as u32 != LINUX_REBOOT_MAGIC2 {
        return Err(Errno::EINVAL);
    }
    let cmd = a.a2() as u32;
    if !matches!(cmd, LINUX_REBOOT_CMD_RESTART | LINUX_REBOOT_CMD_HALT | LINUX_REBOOT_CMD_POWER_OFF) {
        return Err(Errno::EINVAL);
    }
    crate::fs::sync_all();
    kinfo!("system is going down");
    match cmd {
        LINUX_REBOOT_CMD_RESTART => crate::arch::reboot(),
        LINUX_REBOOT_CMD_POWER_OFF => crate::arch::poweroff(),
        _ => crate::arch::halt_forever(),
    }
}
