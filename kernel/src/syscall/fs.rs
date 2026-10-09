//! File system calls.

use super::{value, Args, Ret};
use crate::fs::{self, vfs, FileType, KResult, OpenFile};
use crate::proc::{self, uaccess};
use crate::task::sched;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use huldra_abi::errno::Errno;
use huldra_abi::fs::*;
use huldra_abi::termios::FIONREAD;

/// Largest single read/write transferred through a kernel buffer.
const CHUNK: usize = 64 * 1024;

fn file(fd: u64) -> KResult<Arc<OpenFile>> {
    sched::current().files.lock().get(fd as i32)
}

fn install(file: Arc<OpenFile>, cloexec: bool) -> KResult<Ret> {
    let fd = sched::current().files.lock().alloc(file, cloexec)?;
    value(fd as u64)
}

/// Resolves a path argument relative to `dirfd` (or the cwd).
fn path_at(dirfd: i32, addr: u64) -> KResult<String> {
    let path = uaccess::read_path(addr)?;
    if path.starts_with('/') || dirfd == AT_FDCWD {
        return proc::resolve(&path);
    }
    let dir = sched::current().files.lock().get(dirfd)?;
    if !dir.inode.metadata().is_dir() {
        return Err(Errno::ENOTDIR);
    }
    vfs::normalize(&dir.path, &path)
}

fn current_umask() -> u32 {
    sched::current().proc.lock().umask
}

fn do_read(f: &OpenFile, buf: u64, len: usize, offset: Option<u64>) -> KResult<usize> {
    let len = len.min(CHUNK);
    uaccess::check(buf, len, true)?;
    let mut tmp = alloc::vec![0u8; len];
    let n = match offset {
        Some(off) => f.pread(&mut tmp, off)?,
        None => f.read(&mut tmp)?,
    };
    uaccess::copy_to_user(buf, &tmp[..n])?;
    Ok(n)
}

fn do_write(f: &OpenFile, buf: u64, len: usize, offset: Option<u64>) -> KResult<usize> {
    let mut written = 0;
    while written < len {
        let n = (len - written).min(CHUNK);
        let mut tmp = alloc::vec![0u8; n];
        uaccess::copy_from_user(&mut tmp, buf + written as u64)?;
        let w = match offset {
            Some(off) => f.pwrite(&tmp, off + written as u64)?,
            None => f.write(&tmp)?,
        };
        written += w;
        if w < n {
            break;
        }
    }
    Ok(written)
}

pub fn read(a: &mut Args) -> KResult<Ret> {
    let f = file(a.a0())?;
    value(do_read(&f, a.a1(), a.a2() as usize, None)? as u64)
}

pub fn write(a: &mut Args) -> KResult<Ret> {
    let f = file(a.a0())?;
    value(do_write(&f, a.a1(), a.a2() as usize, None)? as u64)
}

pub fn pread(a: &mut Args) -> KResult<Ret> {
    let f = file(a.a0())?;
    value(do_read(&f, a.a1(), a.a2() as usize, Some(a.a3()))? as u64)
}

pub fn pwrite(a: &mut Args) -> KResult<Ret> {
    let f = file(a.a0())?;
    value(do_write(&f, a.a1(), a.a2() as usize, Some(a.a3()))? as u64)
}

#[derive(Clone, Copy)]
#[repr(C)]
struct IoVec {
    base: u64,
    len: u64,
}

fn iovecs(addr: u64, count: u64) -> KResult<Vec<IoVec>> {
    if count > 1024 {
        return Err(Errno::EINVAL);
    }
    (0..count)
        .map(|i| uaccess::read_user::<IoVec>(addr + i * 16))
        .collect()
}

pub fn readv(a: &mut Args) -> KResult<Ret> {
    let f = file(a.a0())?;
    let mut total = 0;
    for v in iovecs(a.a1(), a.a2())? {
        let n = do_read(&f, v.base, v.len as usize, None)?;
        total += n;
        if n < v.len as usize {
            break;
        }
    }
    value(total as u64)
}

pub fn writev(a: &mut Args) -> KResult<Ret> {
    let f = file(a.a0())?;
    let mut total = 0;
    for v in iovecs(a.a1(), a.a2())? {
        let n = do_write(&f, v.base, v.len as usize, None)?;
        total += n;
        if n < v.len as usize {
            break;
        }
    }
    value(total as u64)
}

fn open_path(path: &str, flags: u32, mode: u32) -> KResult<Ret> {
    let f = fs::open(path, flags, mode & !current_umask())?;
    install(f, flags & O_CLOEXEC != 0)
}

pub fn open(a: &mut Args) -> KResult<Ret> {
    let path = path_at(AT_FDCWD, a.a0())?;
    open_path(&path, a.a1() as u32, a.a2() as u32)
}

pub fn openat(a: &mut Args) -> KResult<Ret> {
    let path = path_at(a.a0() as i32, a.a1())?;
    open_path(&path, a.a2() as u32, a.a3() as u32)
}

pub fn creat(a: &mut Args) -> KResult<Ret> {
    let path = path_at(AT_FDCWD, a.a0())?;
    open_path(&path, O_WRONLY | O_CREAT | O_TRUNC, a.a1() as u32)
}

pub fn close(a: &mut Args) -> KResult<Ret> {
    let removed = sched::current().files.lock().close(a.a0() as i32);
    removed?;
    value(0)
}

pub fn stat(a: &mut Args) -> KResult<Ret> {
    let path = path_at(AT_FDCWD, a.a0())?;
    let st = fs::stat(&path)?.to_stat();
    uaccess::write_user(a.a1(), &st)?;
    value(0)
}

pub fn lstat(a: &mut Args) -> KResult<Ret> {
    let path = path_at(AT_FDCWD, a.a0())?;
    let st = fs::lstat(&path)?.to_stat();
    uaccess::write_user(a.a1(), &st)?;
    value(0)
}

pub fn symlink(a: &mut Args) -> KResult<Ret> {
    let target = uaccess::read_path(a.a0())?;
    fs::symlink(&target, &path_at(AT_FDCWD, a.a1())?)?;
    value(0)
}

pub fn symlinkat(a: &mut Args) -> KResult<Ret> {
    let target = uaccess::read_path(a.a0())?;
    fs::symlink(&target, &path_at(a.a1() as i32, a.a2())?)?;
    value(0)
}

pub fn fstat(a: &mut Args) -> KResult<Ret> {
    let st = file(a.a0())?.inode.metadata().to_stat();
    uaccess::write_user(a.a1(), &st)?;
    value(0)
}

const AT_EMPTY_PATH: u64 = 0x1000;

pub fn newfstatat(a: &mut Args) -> KResult<Ret> {
    let st = if a.a3() & AT_EMPTY_PATH != 0 && uaccess::read_user::<u8>(a.a1())? == 0 {
        file(a.a0())?.inode.metadata().to_stat()
    } else {
        let path = path_at(a.a0() as i32, a.a1())?;
        if a.a3() as u32 & AT_SYMLINK_NOFOLLOW != 0 {
            fs::lstat(&path)?.to_stat()
        } else {
            fs::stat(&path)?.to_stat()
        }
    };
    uaccess::write_user(a.a2(), &st)?;
    value(0)
}

pub fn lseek(a: &mut Args) -> KResult<Ret> {
    value(file(a.a0())?.seek(a.a1() as i64, a.a2() as u32)?)
}

pub fn access(a: &mut Args) -> KResult<Ret> {
    fs::stat(&path_at(AT_FDCWD, a.a0())?)?;
    value(0)
}

pub fn ioctl(a: &mut Args) -> KResult<Ret> {
    let f = file(a.a0())?;
    let cmd = a.a1() as u32;
    match f.inode.ioctl(cmd, a.a2()) {
        Err(Errno::ENOTTY) if cmd == FIONREAD => {
            let n = f.inode.bytes_available().ok_or(Errno::ENOTTY)? as i32;
            uaccess::write_user(a.a2(), &n)?;
            value(0)
        }
        r => value(r?),
    }
}

pub fn pipe(a: &mut Args, flags: u32) -> KResult<Ret> {
    let (r, w) = fs::pipe::create()?;
    let cloexec = flags & O_CLOEXEC != 0;
    uaccess::check(a.a0(), 8, true)?;
    let me = sched::current();
    let mut files = me.files.lock();
    let rfd = files.alloc(r, cloexec)?;
    let wfd = match files.alloc(w, cloexec) {
        Ok(fd) => fd,
        Err(e) => {
            let _ = files.close(rfd);
            return Err(e);
        }
    };
    drop(files);
    uaccess::write_user(a.a0(), &[rfd, wfd])?;
    value(0)
}

pub fn dup(a: &mut Args) -> KResult<Ret> {
    let f = file(a.a0())?;
    install(f, false)
}

pub fn dup2(a: &mut Args) -> KResult<Ret> {
    let (old, new) = (a.a0() as i32, a.a1() as i32);
    let me = sched::current();
    let mut files = me.files.lock();
    if old == new {
        files.get(old)?;
        return value(new as u64);
    }
    value(files.dup2(old, new, false)? as u64)
}

pub fn dup3(a: &mut Args) -> KResult<Ret> {
    let (old, new) = (a.a0() as i32, a.a1() as i32);
    if old == new {
        return Err(Errno::EINVAL);
    }
    let cloexec = a.a2() as u32 & O_CLOEXEC != 0;
    value(sched::current().files.lock().dup2(old, new, cloexec)? as u64)
}

pub fn fcntl(a: &mut Args) -> KResult<Ret> {
    let fd = a.a0() as i32;
    let me = sched::current();
    let mut files = me.files.lock();
    match a.a1() as u32 {
        F_DUPFD | F_DUPFD_CLOEXEC => {
            let f = files.get(fd)?;
            let cloexec = a.a1() as u32 == F_DUPFD_CLOEXEC;
            value(files.alloc_from(a.a2() as usize, f, cloexec)? as u64)
        }
        F_GETFD => value(if files.cloexec(fd)? {
            FD_CLOEXEC as u64
        } else {
            0
        }),
        F_SETFD => {
            files.set_cloexec(fd, a.a2() as u32 & FD_CLOEXEC != 0)?;
            value(0)
        }
        F_GETFL => value(files.get(fd)?.flags() as u64),
        F_SETFL => {
            files.get(fd)?.set_status_flags(a.a2() as u32);
            value(0)
        }
        _ => Err(Errno::EINVAL),
    }
}

pub fn truncate(a: &mut Args) -> KResult<Ret> {
    let path = path_at(AT_FDCWD, a.a0())?;
    vfs::lookup(&path)?.truncate(a.a1())?;
    value(0)
}

pub fn ftruncate(a: &mut Args) -> KResult<Ret> {
    let f = file(a.a0())?;
    if !f.writable() {
        return Err(Errno::EINVAL);
    }
    f.inode.truncate(a.a1())?;
    value(0)
}

pub fn getdents64(a: &mut Args) -> KResult<Ret> {
    let f = file(a.a0())?;
    let len = (a.a2() as usize).min(CHUNK);
    uaccess::check(a.a1(), len, true)?;
    let mut tmp = alloc::vec![0u8; len];
    let n = f.getdents(&mut tmp)?;
    uaccess::copy_to_user(a.a1(), &tmp[..n])?;
    value(n as u64)
}

pub fn getcwd(a: &mut Args) -> KResult<Ret> {
    let mut cwd = proc::cwd().into_bytes();
    cwd.push(0);
    if cwd.len() > a.a1() as usize {
        return Err(Errno::ERANGE);
    }
    uaccess::copy_to_user(a.a0(), &cwd)?;
    value(cwd.len() as u64)
}

pub fn chdir(a: &mut Args) -> KResult<Ret> {
    let path = path_at(AT_FDCWD, a.a0())?;
    if !fs::stat(&path)?.is_dir() {
        return Err(Errno::ENOTDIR);
    }
    sched::current().proc.lock().cwd = path;
    value(0)
}

pub fn rename(a: &mut Args) -> KResult<Ret> {
    let old = path_at(AT_FDCWD, a.a0())?;
    let new = path_at(AT_FDCWD, a.a1())?;
    fs::rename(&old, &new)?;
    value(0)
}

pub fn mkdir(a: &mut Args) -> KResult<Ret> {
    let path = path_at(AT_FDCWD, a.a0())?;
    fs::mkdir(&path, a.a1() as u32 & !current_umask())?;
    value(0)
}

pub fn mkdirat(a: &mut Args) -> KResult<Ret> {
    let path = path_at(a.a0() as i32, a.a1())?;
    fs::mkdir(&path, a.a2() as u32 & !current_umask())?;
    value(0)
}

pub fn rmdir(a: &mut Args) -> KResult<Ret> {
    fs::rmdir(&path_at(AT_FDCWD, a.a0())?)?;
    value(0)
}

pub fn unlink(a: &mut Args) -> KResult<Ret> {
    fs::unlink(&path_at(AT_FDCWD, a.a0())?)?;
    value(0)
}

pub fn unlinkat(a: &mut Args) -> KResult<Ret> {
    let path = path_at(a.a0() as i32, a.a1())?;
    if a.a2() as u32 & AT_REMOVEDIR != 0 {
        fs::rmdir(&path)?;
    } else {
        fs::unlink(&path)?;
    }
    value(0)
}

pub fn umask(a: &mut Args) -> KResult<Ret> {
    let me = sched::current();
    let mut p = me.proc.lock();
    let old = p.umask;
    p.umask = a.a0() as u32 & 0o777;
    value(old as u64)
}

pub fn sync(_a: &mut Args) -> KResult<Ret> {
    fs::sync_all();
    value(0)
}

pub fn mount(a: &mut Args) -> KResult<Ret> {
    let source = if a.a0() != 0 {
        uaccess::read_path(a.a0())?
    } else {
        String::new()
    };
    let target = path_at(AT_FDCWD, a.a1())?;
    let fstype = uaccess::read_cstr(a.a2(), 32)?;
    fs::mount_by_type(&fstype, &source, &target, a.a3())?;
    value(0)
}

pub fn umount(a: &mut Args) -> KResult<Ret> {
    vfs::umount(&path_at(AT_FDCWD, a.a0())?)?;
    value(0)
}

#[allow(dead_code)]
fn is_dir(kind: FileType) -> bool {
    kind == FileType::Directory
}

fn poll_fds(fds_addr: u64, nfds: u64, timeout_ms: Option<u64>) -> KResult<Ret> {
    if nfds > 1024 {
        return Err(Errno::EINVAL);
    }
    let mut fds: Vec<PollFd> = (0..nfds).map(|i| uaccess::read_user::<PollFd>(fds_addr + i * 8)).collect::<KResult<_>>()?;
    let deadline = timeout_ms.map(|ms| crate::time::ticks() + crate::time::ms_to_ticks(ms));
    loop {
        let mut ready = 0;
        {
            let me = sched::current();
            let files = me.files.lock();
            for p in fds.iter_mut() {
                p.revents = 0;
                if p.fd < 0 {
                    continue;
                }
                match files.get(p.fd) {
                    Err(_) => p.revents = POLLNVAL,
                    Ok(f) => {
                        let st = f.inode.poll();
                        if st.readable && p.events & (POLLIN | POLLPRI) != 0 {
                            p.revents |= POLLIN;
                        }
                        if st.writable && p.events & POLLOUT != 0 {
                            p.revents |= POLLOUT;
                        }
                        if st.hangup {
                            p.revents |= POLLHUP;
                        }
                    }
                }
                if p.revents != 0 {
                    ready += 1;
                }
            }
        }
        let expired = deadline.is_some_and(|d| crate::time::ticks() >= d);
        if ready > 0 || expired {
            for (i, p) in fds.iter().enumerate() {
                uaccess::write_user(fds_addr + i as u64 * 8, p)?;
            }
            return value(ready);
        }
        // Re-check on the next tick (or earlier wakeups of this task).
        let next = crate::time::ticks() + 1;
        sched::SLEEPERS.wait_until(|| (crate::time::ticks() >= next).then_some(()))?;
    }
}

pub fn poll(a: &mut Args) -> KResult<Ret> {
    let timeout = a.a2() as i32;
    poll_fds(a.a0(), a.a1(), (timeout >= 0).then_some(timeout as u64))
}

pub fn ppoll(a: &mut Args) -> KResult<Ret> {
    let timeout = if a.a2() == 0 {
        None
    } else {
        let ts: huldra_abi::Timespec = uaccess::read_user(a.a2())?;
        Some(ts.tv_sec as u64 * 1000 + ts.tv_nsec as u64 / 1_000_000)
    };
    poll_fds(a.a0(), a.a1(), timeout)
}

fn statfs_of(path: &str) -> KResult<Statfs> {
    let (_, fs) = vfs::mount_of(path).ok_or(Errno::ENOENT)?;
    let s = fs.statfs();
    Ok(Statfs {
        f_type: s.magic as i64,
        f_bsize: s.block_size as i64,
        f_blocks: s.blocks,
        f_bfree: s.free_blocks,
        f_bavail: s.free_blocks,
        f_files: s.files,
        f_ffree: s.free_files,
        f_namelen: 255,
        f_frsize: s.block_size as i64,
        ..Statfs::default()
    })
}

pub fn statfs(a: &mut Args) -> KResult<Ret> {
    let path = path_at(AT_FDCWD, a.a0())?;
    fs::stat(&path)?;
    uaccess::write_user(a.a1(), &statfs_of(&path)?)?;
    value(0)
}

pub fn fstatfs(a: &mut Args) -> KResult<Ret> {
    let path = file(a.a0())?.path.clone();
    uaccess::write_user(a.a1(), &statfs_of(&path)?)?;
    value(0)
}
