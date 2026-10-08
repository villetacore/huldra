//! System calls (Linux x86_64 numbers and conventions).
//!
//! Entered through the `syscall` instruction (see entry.S) or `int 0x80`.
//! The number is in RAX, arguments in RDI, RSI, RDX, R10, R8, R9; the result
//! or a negated errno goes back in RAX.

mod fs;
mod linux;
mod memory;
mod misc;
mod process;
mod signal;

use crate::arch::{self, TrapFrame};
use crate::fs::KResult;
use crate::task::sched;
use huldra_abi::errno::Errno;
use huldra_abi::signal::SA_RESTART;
use huldra_abi::syscall as nr;

/// Arguments of a system call.
pub struct Args<'a> {
    pub frame: &'a mut TrapFrame,
}

impl Args<'_> {
    pub fn a0(&self) -> u64 {
        self.frame.rdi
    }
    pub fn a1(&self) -> u64 {
        self.frame.rsi
    }
    pub fn a2(&self) -> u64 {
        self.frame.rdx
    }
    pub fn a3(&self) -> u64 {
        self.frame.r10
    }
    pub fn a4(&self) -> u64 {
        self.frame.r8
    }
    pub fn a5(&self) -> u64 {
        self.frame.r9
    }
}

/// What a system call wants returned in RAX.
pub enum Ret {
    Value(u64),
    /// The handler rewrote the whole register frame (execve, sigreturn).
    FrameReplaced,
}

fn value(v: u64) -> KResult<Ret> {
    Ok(Ret::Value(v))
}

fn call(n: usize, a: &mut Args) -> KResult<Ret> {
    match n {
        nr::READ => fs::read(a),
        nr::WRITE => fs::write(a),
        nr::OPEN => fs::open(a),
        nr::CLOSE => fs::close(a),
        nr::STAT | nr::LSTAT => fs::stat(a),
        nr::FSTAT => fs::fstat(a),
        nr::LSEEK => fs::lseek(a),
        nr::POLL => fs::poll(a),
        nr::PPOLL => fs::ppoll(a),
        nr::STATFS => fs::statfs(a),
        nr::FSTATFS => fs::fstatfs(a),
        nr::MMAP => memory::mmap(a),
        nr::MPROTECT => memory::mprotect(a),
        nr::MUNMAP => memory::munmap(a),
        nr::BRK => memory::brk(a),
        nr::RT_SIGACTION => signal::rt_sigaction(a),
        nr::RT_SIGPROCMASK => signal::rt_sigprocmask(a),
        nr::RT_SIGRETURN => signal::rt_sigreturn(a),
        nr::IOCTL => fs::ioctl(a),
        nr::PREAD64 => fs::pread(a),
        nr::PWRITE64 => fs::pwrite(a),
        nr::READV => fs::readv(a),
        nr::WRITEV => fs::writev(a),
        nr::ACCESS => fs::access(a),
        nr::PIPE => fs::pipe(a, 0),
        nr::PIPE2 => fs::pipe(a, a.a1() as u32),
        nr::SCHED_YIELD => {
            sched::yield_now();
            value(0)
        }
        nr::DUP => fs::dup(a),
        nr::DUP2 => fs::dup2(a),
        nr::DUP3 => fs::dup3(a),
        nr::NANOSLEEP => misc::nanosleep(a),
        nr::GETPID => value(sched::current().tgid() as u64),
        nr::FORK => process::fork(a),
        nr::VFORK => linux::vfork(a),
        nr::CLONE => linux::clone(a),
        nr::EXECVE => process::execve(a),
        nr::EXIT => linux::exit(a),
        nr::EXIT_GROUP => linux::exit_group(a),
        nr::WAIT4 => process::wait4(a),
        nr::KILL => signal::kill(a),
        nr::UNAME => misc::uname(a),
        nr::FCNTL => fs::fcntl(a),
        nr::TRUNCATE => fs::truncate(a),
        nr::FTRUNCATE => fs::ftruncate(a),
        nr::GETDENTS64 => fs::getdents64(a),
        nr::GETCWD => fs::getcwd(a),
        nr::CHDIR => fs::chdir(a),
        nr::RENAME => fs::rename(a),
        nr::MKDIR => fs::mkdir(a),
        nr::RMDIR => fs::rmdir(a),
        nr::CREAT => fs::creat(a),
        nr::UNLINK => fs::unlink(a),
        nr::UMASK => fs::umask(a),
        nr::GETUID | nr::GETGID | nr::GETEUID | nr::GETEGID => value(0),
        nr::SETUID | nr::SETGID => {
            if a.a0() == 0 {
                value(0)
            } else {
                Err(Errno::EPERM)
            }
        }
        nr::SETPGID => process::setpgid(a),
        nr::GETPPID => process::getppid(a),
        nr::GETPGRP => process::getpgid_of(0),
        nr::GETPGID => process::getpgid_of(a.a0() as u32),
        nr::SETSID => process::setsid(a),
        nr::GETSID => process::getsid(a),
        nr::ARCH_PRCTL => process::arch_prctl(a),
        nr::SYNC => fs::sync(a),
        nr::MOUNT => fs::mount(a),
        nr::UMOUNT2 => fs::umount(a),
        nr::REBOOT => misc::reboot(a),
        nr::GETTID => value(sched::current().pid as u64),
        nr::SET_TID_ADDRESS => linux::set_tid_address(a),
        nr::FUTEX => linux::futex(a),
        nr::TGKILL => linux::tgkill(a),
        nr::TKILL => linux::tkill(a),
        nr::CLOCK_NANOSLEEP => linux::clock_nanosleep(a),
        nr::CLOCK_GETRES => linux::clock_getres(a),
        nr::TIME => linux::time(a),
        nr::GETTIMEOFDAY => linux::gettimeofday(a),
        nr::GETRANDOM => linux::getrandom(a),
        nr::PRLIMIT64 => linux::prlimit64(a),
        nr::GETRLIMIT => linux::getrlimit(a),
        nr::SYSINFO => linux::sysinfo(a),
        nr::SCHED_GETAFFINITY => linux::sched_getaffinity(a),
        nr::READLINK => linux::readlink(a),
        nr::READLINKAT => linux::readlinkat(a),
        nr::FACCESSAT | nr::FACCESSAT2 => linux::faccessat(a),
        nr::FCHDIR => linux::fchdir(a),
        nr::PAUSE => linux::pause(a),
        nr::ALARM => linux::alarm(a),
        nr::FSYNC | nr::FDATASYNC => linux::fsync(a),
        nr::SET_ROBUST_LIST
        | nr::SIGALTSTACK
        | nr::MADVISE
        | nr::SCHED_SETAFFINITY
        | nr::SETRLIMIT
        | nr::PRCTL
        | nr::MEMBARRIER
        | nr::CHMOD
        | nr::FCHMOD
        | nr::FCHMODAT
        | nr::CHOWN
        | nr::FCHOWN
        | nr::LCHOWN
        | nr::UTIMENSAT => linux::ignore(a),
        nr::GETGROUPS => value(0),
        nr::GETCPU => {
            if a.a0() != 0 {
                crate::proc::uaccess::write_user(a.a0(), &0u32)?;
            }
            value(0)
        }
        // Newer interfaces whose absence callers handle (they fall back).
        nr::CLONE3 | nr::RSEQ | nr::MREMAP => Err(Errno::ENOSYS),
        nr::LINK | nr::SYMLINK => Err(Errno::EPERM),
        nr::CLOCK_GETTIME => misc::clock_gettime(a),
        nr::OPENAT => fs::openat(a),
        nr::MKDIRAT => fs::mkdirat(a),
        nr::NEWFSTATAT => fs::newfstatat(a),
        nr::UNLINKAT => fs::unlinkat(a),
        _ => {
            report_unimplemented(n);
            Err(Errno::ENOSYS)
        }
    }
}

/// Logs the first use of each unimplemented system call (helps porting).
fn report_unimplemented(n: usize) {
    use core::sync::atomic::{AtomicU64, Ordering};
    static SEEN: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
    if n < 512 && SEEN[n / 64].fetch_or(1 << (n % 64), Ordering::Relaxed) & (1 << (n % 64)) == 0 {
        let me = sched::current();
        kwarn!("{}[{}]: unimplemented system call {}", me.name(), me.pid, n);
    }
}

/// Runs the system call described by `frame` and stores the result.
pub fn handle(frame: &mut TrapFrame) {
    let n = frame.rax as usize;
    let result = {
        let mut args = Args { frame };
        call(n, &mut args)
    };
    match result {
        Ok(Ret::Value(v)) => frame.rax = v,
        Ok(Ret::FrameReplaced) => {}
        Err(Errno::EINTR) if signal_restarts() => {
            // Re-execute the `syscall`/`int 0x80` instruction after the handler.
            frame.rax = n as u64;
            frame.rip -= 2;
        }
        Err(e) => frame.rax = (-(e.code() as i64)) as u64,
    }
}

fn signal_restarts() -> bool {
    crate::proc::signal::next_action().is_some_and(|a| a.flags & SA_RESTART != 0 && a.handler > 1)
}

/// Rust side of the `syscall` instruction entry. Returns nonzero if the
/// frame can be restored with `sysretq` instead of `iretq`.
#[no_mangle]
extern "C" fn syscall_dispatch(frame: &mut TrapFrame) -> u64 {
    arch::enable_interrupts();
    handle(frame);
    sched::preempt_if_needed();
    arch::disable_interrupts();
    crate::proc::return_to_user(frame);
    let canonical = crate::mm::is_user_address(frame.rip) && frame.rip != 0;
    let user_segments =
        frame.cs == arch::gdt::USER_CODE as u64 && frame.ss == arch::gdt::USER_DATA as u64;
    (canonical && user_segments) as u64
}
