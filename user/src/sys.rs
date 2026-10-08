//! Raw system calls and thin typed wrappers.

use crate::{Errno, Result};
use alloc::vec::Vec;
use core::arch::asm;
use huldra_abi::fs::Stat;
use huldra_abi::signal::SigAction;
use huldra_abi::syscall as nr;
use huldra_abi::{Timespec, Utsname};

/// # Safety
/// Raw system call: arguments must be valid for the call.
#[inline(always)]
pub unsafe fn syscall6(
    n: usize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
) -> isize {
    let ret: isize;
    asm!(
        "syscall",
        inlateout("rax") n as isize => ret,
        in("rdi") a0,
        in("rsi") a1,
        in("rdx") a2,
        in("r10") a3,
        in("r8") a4,
        in("r9") a5,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack),
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall3(n: usize, a0: usize, a1: usize, a2: usize) -> isize {
    syscall6(n, a0, a1, a2, 0, 0, 0)
}

fn check(ret: isize) -> Result<usize> {
    if (-4095..0).contains(&ret) {
        Err(Errno::from_code(-ret as i32).unwrap_or(Errno::EIO))
    } else {
        Ok(ret as usize)
    }
}

/// A NUL-terminated copy of `s` for passing to the kernel.
pub struct CString(Vec<u8>);

impl CString {
    pub fn new(s: &str) -> CString {
        let mut v = Vec::with_capacity(s.len() + 1);
        v.extend_from_slice(s.as_bytes());
        v.push(0);
        CString(v)
    }

    pub fn as_ptr(&self) -> usize {
        self.0.as_ptr() as usize
    }
}

pub fn read(fd: i32, buf: &mut [u8]) -> Result<usize> {
    check(unsafe { syscall3(nr::READ, fd as usize, buf.as_mut_ptr() as usize, buf.len()) })
}

pub fn write(fd: i32, buf: &[u8]) -> Result<usize> {
    check(unsafe { syscall3(nr::WRITE, fd as usize, buf.as_ptr() as usize, buf.len()) })
}

pub fn open(path: &str, flags: u32, mode: u32) -> Result<i32> {
    let p = CString::new(path);
    check(unsafe { syscall3(nr::OPEN, p.as_ptr(), flags as usize, mode as usize) })
        .map(|fd| fd as i32)
}

pub fn close(fd: i32) -> Result<()> {
    check(unsafe { syscall3(nr::CLOSE, fd as usize, 0, 0) }).map(drop)
}

pub fn stat(path: &str) -> Result<Stat> {
    let p = CString::new(path);
    let mut st = Stat::default();
    check(unsafe { syscall3(nr::STAT, p.as_ptr(), &mut st as *mut Stat as usize, 0) })?;
    Ok(st)
}

pub fn fstat(fd: i32) -> Result<Stat> {
    let mut st = Stat::default();
    check(unsafe { syscall3(nr::FSTAT, fd as usize, &mut st as *mut Stat as usize, 0) })?;
    Ok(st)
}

pub fn lseek(fd: i32, offset: i64, whence: u32) -> Result<u64> {
    check(unsafe { syscall3(nr::LSEEK, fd as usize, offset as usize, whence as usize) })
        .map(|v| v as u64)
}

pub fn mmap(
    addr: usize,
    len: usize,
    prot: u32,
    flags: u32,
    fd: i32,
    offset: usize,
) -> Result<usize> {
    check(unsafe {
        syscall6(
            nr::MMAP,
            addr,
            len,
            prot as usize,
            flags as usize,
            fd as isize as usize,
            offset,
        )
    })
}

pub fn munmap(addr: usize, len: usize) -> Result<()> {
    check(unsafe { syscall3(nr::MUNMAP, addr, len, 0) }).map(drop)
}

pub fn mprotect(addr: usize, len: usize, prot: u32) -> Result<()> {
    check(unsafe { syscall3(nr::MPROTECT, addr, len, prot as usize) }).map(drop)
}

pub fn brk(addr: usize) -> usize {
    unsafe { syscall3(nr::BRK, addr, 0, 0) as usize }
}

pub fn sigaction(sig: u32, act: Option<&SigAction>, old: Option<&mut SigAction>) -> Result<()> {
    let a = act.map_or(0, |a| a as *const SigAction as usize);
    let o = old.map_or(0, |o| o as *mut SigAction as usize);
    check(unsafe { syscall6(nr::RT_SIGACTION, sig as usize, a, o, 8, 0, 0) }).map(drop)
}

pub fn sigprocmask(how: u32, set: Option<u64>) -> Result<u64> {
    let set_storage = set.unwrap_or(0);
    let set_ptr = if set.is_some() {
        &set_storage as *const u64 as usize
    } else {
        0
    };
    let mut old = 0u64;
    check(unsafe {
        syscall6(
            nr::RT_SIGPROCMASK,
            how as usize,
            set_ptr,
            &mut old as *mut u64 as usize,
            8,
            0,
            0,
        )
    })?;
    Ok(old)
}

pub fn ioctl(fd: i32, cmd: u32, arg: usize) -> Result<usize> {
    check(unsafe { syscall3(nr::IOCTL, fd as usize, cmd as usize, arg) })
}

pub fn pipe() -> Result<(i32, i32)> {
    let mut fds = [0i32; 2];
    check(unsafe { syscall3(nr::PIPE, fds.as_mut_ptr() as usize, 0, 0) })?;
    Ok((fds[0], fds[1]))
}

pub fn dup(fd: i32) -> Result<i32> {
    check(unsafe { syscall3(nr::DUP, fd as usize, 0, 0) }).map(|v| v as i32)
}

pub fn dup2(old: i32, new: i32) -> Result<i32> {
    check(unsafe { syscall3(nr::DUP2, old as usize, new as usize, 0) }).map(|v| v as i32)
}

pub fn fcntl(fd: i32, cmd: u32, arg: usize) -> Result<usize> {
    check(unsafe { syscall3(nr::FCNTL, fd as usize, cmd as usize, arg) })
}

pub fn nanosleep(ts: &Timespec) -> Result<()> {
    let mut rem = Timespec::default();
    check(unsafe {
        syscall3(
            nr::NANOSLEEP,
            ts as *const Timespec as usize,
            &mut rem as *mut Timespec as usize,
            0,
        )
    })
    .map(drop)
}

pub fn getpid() -> u32 {
    unsafe { syscall3(nr::GETPID, 0, 0, 0) as u32 }
}

pub fn getppid() -> u32 {
    unsafe { syscall3(nr::GETPPID, 0, 0, 0) as u32 }
}

pub fn fork() -> Result<u32> {
    check(unsafe { syscall3(nr::FORK, 0, 0, 0) }).map(|v| v as u32)
}

/// `argv`/`envp` must be NULL-terminated arrays of C string pointers.
pub fn execve(path: &str, argv: &[usize], envp: &[usize]) -> Result<()> {
    let p = CString::new(path);
    check(unsafe {
        syscall3(
            nr::EXECVE,
            p.as_ptr(),
            argv.as_ptr() as usize,
            envp.as_ptr() as usize,
        )
    })
    .map(drop)
}

pub fn exit(code: i32) -> ! {
    unsafe { syscall3(nr::EXIT_GROUP, code as usize, 0, 0) };
    unreachable!()
}

pub fn wait4(pid: i32, options: u32) -> Result<(u32, i32)> {
    let mut status = 0i32;
    let r = check(unsafe {
        syscall6(
            nr::WAIT4,
            pid as isize as usize,
            &mut status as *mut i32 as usize,
            options as usize,
            0,
            0,
            0,
        )
    })?;
    Ok((r as u32, status))
}

pub fn kill(pid: i32, sig: u32) -> Result<()> {
    check(unsafe { syscall3(nr::KILL, pid as isize as usize, sig as usize, 0) }).map(drop)
}

pub fn uname() -> Result<Utsname> {
    let mut u = Utsname::default();
    check(unsafe { syscall3(nr::UNAME, &mut u as *mut Utsname as usize, 0, 0) })?;
    Ok(u)
}

pub fn getdents64(fd: i32, buf: &mut [u8]) -> Result<usize> {
    check(unsafe {
        syscall3(
            nr::GETDENTS64,
            fd as usize,
            buf.as_mut_ptr() as usize,
            buf.len(),
        )
    })
}

pub fn getcwd() -> Result<alloc::string::String> {
    let mut buf = alloc::vec![0u8; 4096];
    let n = check(unsafe { syscall3(nr::GETCWD, buf.as_mut_ptr() as usize, buf.len(), 0) })?;
    buf.truncate(n.saturating_sub(1));
    alloc::string::String::from_utf8(buf).map_err(|_| Errno::EINVAL)
}

pub fn chdir(path: &str) -> Result<()> {
    let p = CString::new(path);
    check(unsafe { syscall3(nr::CHDIR, p.as_ptr(), 0, 0) }).map(drop)
}

pub fn rename(old: &str, new: &str) -> Result<()> {
    let (o, n) = (CString::new(old), CString::new(new));
    check(unsafe { syscall3(nr::RENAME, o.as_ptr(), n.as_ptr(), 0) }).map(drop)
}

pub fn mkdir(path: &str, mode: u32) -> Result<()> {
    let p = CString::new(path);
    check(unsafe { syscall3(nr::MKDIR, p.as_ptr(), mode as usize, 0) }).map(drop)
}

pub fn rmdir(path: &str) -> Result<()> {
    let p = CString::new(path);
    check(unsafe { syscall3(nr::RMDIR, p.as_ptr(), 0, 0) }).map(drop)
}

pub fn unlink(path: &str) -> Result<()> {
    let p = CString::new(path);
    check(unsafe { syscall3(nr::UNLINK, p.as_ptr(), 0, 0) }).map(drop)
}

pub fn ftruncate(fd: i32, len: u64) -> Result<()> {
    check(unsafe { syscall3(nr::FTRUNCATE, fd as usize, len as usize, 0) }).map(drop)
}

pub fn setpgid(pid: u32, pgid: u32) -> Result<()> {
    check(unsafe { syscall3(nr::SETPGID, pid as usize, pgid as usize, 0) }).map(drop)
}

pub fn getpgrp() -> u32 {
    unsafe { syscall3(nr::GETPGRP, 0, 0, 0) as u32 }
}

pub fn setsid() -> Result<u32> {
    check(unsafe { syscall3(nr::SETSID, 0, 0, 0) }).map(|v| v as u32)
}

pub fn mount(source: &str, target: &str, fstype: &str, flags: u64) -> Result<()> {
    let (s, t, f) = (
        CString::new(source),
        CString::new(target),
        CString::new(fstype),
    );
    check(unsafe {
        syscall6(
            nr::MOUNT,
            s.as_ptr(),
            t.as_ptr(),
            f.as_ptr(),
            flags as usize,
            0,
            0,
        )
    })
    .map(drop)
}

pub fn umount(target: &str) -> Result<()> {
    let t = CString::new(target);
    check(unsafe { syscall3(nr::UMOUNT2, t.as_ptr(), 0, 0) }).map(drop)
}

pub fn reboot(cmd: u32) -> Result<()> {
    use huldra_abi::{LINUX_REBOOT_MAGIC1, LINUX_REBOOT_MAGIC2};
    check(unsafe {
        syscall3(
            nr::REBOOT,
            LINUX_REBOOT_MAGIC1 as usize,
            LINUX_REBOOT_MAGIC2 as usize,
            cmd as usize,
        )
    })
    .map(drop)
}

pub fn clock_gettime(clock: i32) -> Result<Timespec> {
    let mut ts = Timespec::default();
    check(unsafe {
        syscall3(
            nr::CLOCK_GETTIME,
            clock as usize,
            &mut ts as *mut Timespec as usize,
            0,
        )
    })?;
    Ok(ts)
}

pub fn sync() {
    unsafe { syscall3(nr::SYNC, 0, 0, 0) };
}

pub fn sched_yield() {
    unsafe { syscall3(nr::SCHED_YIELD, 0, 0, 0) };
}

pub fn poll(fds: &mut [huldra_abi::fs::PollFd], timeout_ms: i32) -> Result<usize> {
    check(unsafe { syscall3(nr::POLL, fds.as_mut_ptr() as usize, fds.len(), timeout_ms as isize as usize) })
}

pub fn statfs(path: &str) -> Result<huldra_abi::fs::Statfs> {
    let p = CString::new(path);
    let mut st = huldra_abi::fs::Statfs::default();
    check(unsafe { syscall3(nr::STATFS, p.as_ptr(), &mut st as *mut _ as usize, 0) })?;
    Ok(st)
}
