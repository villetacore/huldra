//! System calls (Linux x86_64 numbers and conventions).
//!
//! Entered through the `syscall` instruction (see entry.S) or `int 0x80`.
//! The number is in RAX, arguments in RDI, RSI, RDX, R10, R8, R9; the result
//! or a negated errno goes back in RAX.

mod fs;
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
        nr::GETPID => value(sched::current().pid as u64),
        nr::FORK | nr::VFORK => process::fork(a),
        nr::EXECVE => process::execve(a),
        nr::EXIT | nr::EXIT_GROUP => process::exit(a),
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
        nr::SET_TID_ADDRESS => value(sched::current().pid as u64),
        nr::CLOCK_GETTIME => misc::clock_gettime(a),
        nr::OPENAT => fs::openat(a),
        nr::MKDIRAT => fs::mkdirat(a),
        nr::NEWFSTATAT => fs::newfstatat(a),
        nr::UNLINKAT => fs::unlinkat(a),
        _ => Err(Errno::ENOSYS),
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
