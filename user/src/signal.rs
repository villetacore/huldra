//! Signal handling.

use crate::{sys, Result};
use core::arch::global_asm;
use huldra_abi::signal::*;

pub use huldra_abi::signal::{
    SIGCHLD, SIGINT, SIGKILL, SIGPIPE, SIGQUIT, SIGSEGV, SIGTERM, SIGUSR1, SIGUSR2,
};

// Handlers return here, which asks the kernel to restore the interrupted context.
global_asm!(
    ".global __huldra_sigreturn",
    "__huldra_sigreturn:",
    "    mov eax, 15", // rt_sigreturn
    "    syscall",
    "    ud2",
);

extern "C" {
    fn __huldra_sigreturn();
}

pub type Handler = extern "C" fn(i32);

fn install(sig: u32, handler: u64, flags: u64) -> Result<()> {
    let act = SigAction {
        handler,
        flags: flags | SA_RESTORER,
        restorer: __huldra_sigreturn as *const () as u64,
        mask: 0,
    };
    sys::sigaction(sig, Some(&act), None)
}

/// Runs `handler` when `sig` arrives; interrupted system calls are restarted.
pub fn handle(sig: u32, handler: Handler) -> Result<()> {
    install(sig, handler as *const () as u64, SA_RESTART)
}

/// Like [`handle`] but interrupted system calls fail with EINTR.
pub fn handle_interrupting(sig: u32, handler: Handler) -> Result<()> {
    install(sig, handler as *const () as u64, 0)
}

pub fn ignore(sig: u32) -> Result<()> {
    install(sig, SIG_IGN, 0)
}

pub fn default(sig: u32) -> Result<()> {
    install(sig, SIG_DFL, 0)
}

pub fn kill(pid: i32, sig: u32) -> Result<()> {
    sys::kill(pid, sig)
}

pub fn block(sig: u32) -> Result<()> {
    sys::sigprocmask(SIG_BLOCK, Some(sigbit(sig))).map(drop)
}

pub fn unblock(sig: u32) -> Result<()> {
    sys::sigprocmask(SIG_UNBLOCK, Some(sigbit(sig))).map(drop)
}

/// Parses `INT`, `SIGINT` or `2`.
pub fn parse(name: &str) -> Option<u32> {
    if let Ok(n) = name.parse::<u32>() {
        return (n <= NSIG).then_some(n);
    }
    let name = name.strip_prefix("SIG").unwrap_or(name);
    Some(match name {
        "HUP" => SIGHUP,
        "INT" => SIGINT,
        "QUIT" => SIGQUIT,
        "ILL" => SIGILL,
        "ABRT" => SIGABRT,
        "FPE" => SIGFPE,
        "KILL" => SIGKILL,
        "USR1" => SIGUSR1,
        "SEGV" => SIGSEGV,
        "USR2" => SIGUSR2,
        "PIPE" => SIGPIPE,
        "ALRM" => SIGALRM,
        "TERM" => SIGTERM,
        "CHLD" => SIGCHLD,
        "CONT" => SIGCONT,
        "STOP" => SIGSTOP,
        "TSTP" => SIGTSTP,
        _ => return None,
    })
}

pub fn name(sig: u32) -> &'static str {
    huldra_abi::signal::name(sig)
}
