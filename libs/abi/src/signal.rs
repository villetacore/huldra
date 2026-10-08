//! Signals.

pub const SIGHUP: u32 = 1;
pub const SIGINT: u32 = 2;
pub const SIGQUIT: u32 = 3;
pub const SIGILL: u32 = 4;
pub const SIGTRAP: u32 = 5;
pub const SIGABRT: u32 = 6;
pub const SIGBUS: u32 = 7;
pub const SIGFPE: u32 = 8;
pub const SIGKILL: u32 = 9;
pub const SIGUSR1: u32 = 10;
pub const SIGSEGV: u32 = 11;
pub const SIGUSR2: u32 = 12;
pub const SIGPIPE: u32 = 13;
pub const SIGALRM: u32 = 14;
pub const SIGTERM: u32 = 15;
pub const SIGCHLD: u32 = 17;
pub const SIGCONT: u32 = 18;
pub const SIGSTOP: u32 = 19;
pub const SIGTSTP: u32 = 20;
pub const SIGTTIN: u32 = 21;
pub const SIGTTOU: u32 = 22;
pub const SIGWINCH: u32 = 28;
pub const NSIG: u32 = 64;

pub const SIG_DFL: u64 = 0;
pub const SIG_IGN: u64 = 1;

pub const SIG_BLOCK: u32 = 0;
pub const SIG_UNBLOCK: u32 = 1;
pub const SIG_SETMASK: u32 = 2;

pub const SA_SIGINFO: u64 = 0x0000_0004;
pub const SA_RESTORER: u64 = 0x0400_0000;
pub const SA_RESTART: u64 = 0x1000_0000;
pub const SA_NODEFER: u64 = 0x4000_0000;
pub const SA_RESETHAND: u64 = 0x8000_0000;

/// Kernel `struct sigaction` (x86_64).
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct SigAction {
    pub handler: u64,
    pub flags: u64,
    pub restorer: u64,
    pub mask: u64,
}

pub const fn sigbit(sig: u32) -> u64 {
    1 << (sig - 1)
}

pub fn name(sig: u32) -> &'static str {
    match sig {
        SIGHUP => "Hangup",
        SIGINT => "Interrupt",
        SIGQUIT => "Quit",
        SIGILL => "Illegal instruction",
        SIGTRAP => "Trace/breakpoint trap",
        SIGABRT => "Aborted",
        SIGBUS => "Bus error",
        SIGFPE => "Floating point exception",
        SIGKILL => "Killed",
        SIGUSR1 => "User defined signal 1",
        SIGSEGV => "Segmentation fault",
        SIGUSR2 => "User defined signal 2",
        SIGPIPE => "Broken pipe",
        SIGALRM => "Alarm clock",
        SIGTERM => "Terminated",
        SIGCHLD => "Child exited",
        SIGCONT => "Continued",
        SIGSTOP => "Stopped (signal)",
        SIGTSTP => "Stopped",
        _ => "Unknown signal",
    }
}
