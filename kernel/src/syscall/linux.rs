//! System calls needed by Linux programs (glibc and musl): threads, futexes,
//! time, limits, randomness and a few that are accepted but do nothing.

use super::{value, Args, Ret};
use crate::fs::{self, KResult};
use crate::proc::{futex, lifecycle, signal, uaccess};
use crate::task::{self, sched};
use alloc::string::String;
use huldra_abi::errno::Errno;
use huldra_abi::Timespec;

pub fn clone(a: &mut Args) -> KResult<Ret> {
    // x86_64 order: flags, stack, parent_tid, child_tid, tls.
    let (flags, stack, ptid, ctid, tls) = (a.a0(), a.a1(), a.a2(), a.a3(), a.a4());
    value(lifecycle::clone(a.frame, flags, stack, ptid, ctid, tls)? as u64)
}

pub fn vfork(a: &mut Args) -> KResult<Ret> {
    let flags = lifecycle::CLONE_VM | lifecycle::CLONE_VFORK | huldra_abi::signal::SIGCHLD as u64;
    value(lifecycle::clone(a.frame, flags, 0, 0, 0, 0)? as u64)
}

pub fn exit(a: &mut Args) -> KResult<Ret> {
    let me = sched::current();
    if me.is_thread() {
        drop(me);
        lifecycle::exit_thread()
    }
    drop(me);
    lifecycle::exit_process(huldra_abi::process::exit_status(a.a0() as i32))
}

pub fn exit_group(a: &mut Args) -> KResult<Ret> {
    lifecycle::exit_group(huldra_abi::process::exit_status(a.a0() as i32))
}

pub fn set_tid_address(a: &mut Args) -> KResult<Ret> {
    let me = sched::current();
    me.clear_tid.store(a.a0(), core::sync::atomic::Ordering::Release);
    value(me.pid as u64)
}

const FUTEX_WAIT: u32 = 0;
const FUTEX_WAKE: u32 = 1;
const FUTEX_REQUEUE: u32 = 3;
const FUTEX_CMP_REQUEUE: u32 = 4;
const FUTEX_WAKE_OP: u32 = 5;
const FUTEX_WAIT_BITSET: u32 = 9;
const FUTEX_WAKE_BITSET: u32 = 10;
const FUTEX_CLOCK_REALTIME: u32 = 256;

fn timespec_to_ms(ts: &Timespec) -> u64 {
    ts.tv_sec.max(0) as u64 * 1000 + (ts.tv_nsec.max(0) as u64).div_ceil(1_000_000)
}

pub fn futex(a: &mut Args) -> KResult<Ret> {
    let (addr, op, val, timeout, addr2, val3) = (a.a0(), a.a1() as u32, a.a2() as u32, a.a3(), a.a4(), a.a5() as u32);
    let mm = sched::current().mm.id();
    let cmd = op & 0x7F;
    match cmd {
        FUTEX_WAIT | FUTEX_WAIT_BITSET => {
            let deadline = if timeout == 0 {
                None
            } else {
                let ts: Timespec = uaccess::read_user(timeout)?;
                let ms = if cmd == FUTEX_WAIT {
                    timespec_to_ms(&ts)
                } else {
                    // Absolute time on CLOCK_MONOTONIC (or REALTIME).
                    let now = if op & FUTEX_CLOCK_REALTIME != 0 {
                        let (s, ns) = crate::time::now_precise();
                        Timespec { tv_sec: s, tv_nsec: ns }
                    } else {
                        let ms = crate::time::uptime_ms();
                        Timespec { tv_sec: (ms / 1000) as i64, tv_nsec: ((ms % 1000) * 1_000_000) as i64 }
                    };
                    timespec_to_ms(&ts).saturating_sub(timespec_to_ms(&now))
                };
                Some(crate::time::ticks() + crate::time::ms_to_ticks(ms))
            };
            futex::wait(mm, addr, val, deadline)?;
            value(0)
        }
        FUTEX_WAKE | FUTEX_WAKE_BITSET => value(futex::wake(mm, addr, val as usize) as u64),
        FUTEX_REQUEUE => value(futex::requeue(mm, addr, addr2, val as usize, timeout as usize) as u64),
        FUTEX_CMP_REQUEUE => {
            let current: u32 = uaccess::read_user(addr)?;
            if current != val3 {
                return Err(Errno::EAGAIN);
            }
            value(futex::requeue(mm, addr, addr2, val as usize, timeout as usize) as u64)
        }
        FUTEX_WAKE_OP => {
            // Rarely used by glibc for condition variables: wake both words.
            let n = futex::wake(mm, addr, val as usize) + futex::wake(mm, addr2, timeout as usize);
            value(n as u64)
        }
        _ => Err(Errno::ENOSYS),
    }
}

pub fn tgkill(a: &mut Args) -> KResult<Ret> {
    let (tgid, tid, sig) = (a.a0() as u32, a.a1() as u32, a.a2() as u32);
    let t = task::lookup(tid).filter(|t| t.tgid() == tgid).ok_or(Errno::ESRCH)?;
    if sig != 0 {
        signal::send(&t, sig);
    }
    value(0)
}

pub fn tkill(a: &mut Args) -> KResult<Ret> {
    let t = task::lookup(a.a0() as u32).ok_or(Errno::ESRCH)?;
    if a.a1() != 0 {
        signal::send(&t, a.a1() as u32);
    }
    value(0)
}

pub fn clock_nanosleep(a: &mut Args) -> KResult<Ret> {
    const TIMER_ABSTIME: u64 = 1;
    let ts: Timespec = uaccess::read_user(a.a2())?;
    let ms = if a.a1() & TIMER_ABSTIME != 0 {
        let now = if a.a0() == huldra_abi::CLOCK_REALTIME as u64 {
            let (s, ns) = crate::time::now_precise();
            Timespec { tv_sec: s, tv_nsec: ns }
        } else {
            let ms = crate::time::uptime_ms();
            Timespec { tv_sec: (ms / 1000) as i64, tv_nsec: ((ms % 1000) * 1_000_000) as i64 }
        };
        timespec_to_ms(&ts).saturating_sub(timespec_to_ms(&now))
    } else {
        timespec_to_ms(&ts)
    };
    let target = crate::time::ticks() + crate::time::ms_to_ticks(ms);
    sched::SLEEPERS.wait_until(|| (crate::time::ticks() >= target).then_some(()))?;
    value(0)
}

pub fn time(a: &mut Args) -> KResult<Ret> {
    let now = crate::time::now();
    if a.a0() != 0 {
        uaccess::write_user(a.a0(), &now)?;
    }
    value(now as u64)
}

pub fn gettimeofday(a: &mut Args) -> KResult<Ret> {
    if a.a0() != 0 {
        let (s, ns) = crate::time::now_precise();
        uaccess::write_user(a.a0(), &[s, ns / 1000])?;
    }
    value(0)
}

pub fn clock_getres(a: &mut Args) -> KResult<Ret> {
    if a.a1() != 0 {
        uaccess::write_user(a.a1(), &Timespec { tv_sec: 0, tv_nsec: 1_000_000_000 / crate::time::HZ as i64 })?;
    }
    value(0)
}

pub fn getrandom(a: &mut Args) -> KResult<Ret> {
    let len = (a.a1() as usize).min(1 << 20);
    let mut buf = alloc::vec![0u8; len];
    crate::random::fill(&mut buf);
    uaccess::copy_to_user(a.a0(), &buf)?;
    value(len as u64)
}

/// `struct rlimit` values: (current, maximum).
fn rlimit(resource: u64) -> (u64, u64) {
    const INFINITY: u64 = u64::MAX;
    match resource {
        3 => (8 << 20, INFINITY),     // RLIMIT_STACK
        7 => (256, 256),              // RLIMIT_NOFILE
        _ => (INFINITY, INFINITY),
    }
}

pub fn prlimit64(a: &mut Args) -> KResult<Ret> {
    // pid (a0) is ignored: limits are global and fixed.
    if a.a3() != 0 {
        let (cur, max) = rlimit(a.a1());
        uaccess::write_user(a.a3(), &[cur, max])?;
    }
    value(0)
}

pub fn getrlimit(a: &mut Args) -> KResult<Ret> {
    let (cur, max) = rlimit(a.a0());
    uaccess::write_user(a.a1(), &[cur, max])?;
    value(0)
}

/// `struct sysinfo` (x86_64).
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct SysInfo {
    uptime: i64,
    loads: [u64; 3],
    totalram: u64,
    freeram: u64,
    sharedram: u64,
    bufferram: u64,
    totalswap: u64,
    freeswap: u64,
    procs: u16,
    _pad: u16,
    _pad2: u32,
    totalhigh: u64,
    freehigh: u64,
    mem_unit: u32,
    _f: [u8; 4],
}

pub fn sysinfo(a: &mut Args) -> KResult<Ret> {
    let (free, total) = crate::mm::frame::stats();
    let info = SysInfo {
        uptime: (crate::time::uptime_ms() / 1000) as i64,
        totalram: total as u64 * 4096,
        freeram: free as u64 * 4096,
        procs: task::all_tasks().len() as u16,
        mem_unit: 1,
        ..SysInfo::default()
    };
    uaccess::write_user(a.a0(), &info)?;
    value(0)
}

pub fn sched_getaffinity(a: &mut Args) -> KResult<Ret> {
    let len = (a.a1() as usize).min(128);
    if len < 8 {
        return Err(Errno::EINVAL);
    }
    let mut mask = alloc::vec![0u8; len];
    mask[0] = 1; // one CPU
    uaccess::copy_to_user(a.a2(), &mask)?;
    value(8)
}

fn readlink_path(path: &str, buf: u64, size: u64) -> KResult<Ret> {
    let target: String = match path {
        "/proc/self/exe" => sched::current().proc.lock().exe.clone(),
        p => fs::readlink(p)?,
    };
    let n = target.len().min(size as usize);
    uaccess::copy_to_user(buf, &target.as_bytes()[..n])?;
    value(n as u64)
}

pub fn readlink(a: &mut Args) -> KResult<Ret> {
    let path = crate::proc::resolve(&uaccess::read_path(a.a0())?)?;
    readlink_path(&path, a.a1(), a.a2())
}

pub fn readlinkat(a: &mut Args) -> KResult<Ret> {
    let path = crate::proc::resolve(&uaccess::read_path(a.a1())?)?;
    readlink_path(&path, a.a2(), a.a3())
}

pub fn faccessat(a: &mut Args) -> KResult<Ret> {
    let path = crate::proc::resolve(&uaccess::read_path(a.a1())?)?;
    fs::stat(&path)?;
    value(0)
}

pub fn fchdir(a: &mut Args) -> KResult<Ret> {
    let me = sched::current();
    let f = me.files.lock().get(a.a0() as i32)?;
    if !f.inode.metadata().is_dir() {
        return Err(Errno::ENOTDIR);
    }
    me.proc.lock().cwd = f.path.clone();
    value(0)
}

pub fn pause(_a: &mut Args) -> KResult<Ret> {
    sched::SLEEPERS.wait_until(|| None::<()>)?;
    value(0)
}

pub fn alarm(a: &mut Args) -> KResult<Ret> {
    let me = sched::current();
    let left = crate::proc::alarm::set(&me, a.a0() * 1000);
    value(left.div_ceil(1000))
}

/// Calls that succeed without doing anything (no permissions, no robust
/// futex lists, no CPU affinity, ...).
pub fn ignore(_a: &mut Args) -> KResult<Ret> {
    value(0)
}

pub fn fsync(a: &mut Args) -> KResult<Ret> {
    sched::current().files.lock().get(a.a0() as i32)?;
    fs::sync_all();
    value(0)
}
