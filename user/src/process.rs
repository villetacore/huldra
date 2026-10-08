//! Processes: fork, exec, wait, exit, process groups.

use crate::sys::{self, CString};
use crate::{Errno, Result};
use alloc::string::String;
use alloc::vec::Vec;
use huldra_abi::process::*;
use huldra_abi::termios::{TIOCGPGRP, TIOCSPGRP};

pub type Pid = u32;

pub fn exit(code: i32) -> ! {
    crate::io::flush_stdout();
    sys::exit(code)
}

/// Returns `Some(child_pid)` in the parent and `None` in the child.
pub fn fork() -> Result<Option<Pid>> {
    crate::io::flush_stdout();
    let pid = sys::fork()?;
    Ok(if pid == 0 { None } else { Some(pid) })
}

/// Replaces the current program; returns only on error.
pub fn exec(path: &str, args: &[&str], env: &[String]) -> Errno {
    let args_c: Vec<CString> = args.iter().map(|a| CString::new(a)).collect();
    let env_c: Vec<CString> = env.iter().map(|e| CString::new(e)).collect();
    let mut argv: Vec<usize> = args_c.iter().map(|c| c.as_ptr()).collect();
    argv.push(0);
    let mut envp: Vec<usize> = env_c.iter().map(|c| c.as_ptr()).collect();
    envp.push(0);
    crate::io::flush_stdout();
    match sys::execve(path, &argv, &envp) {
        Ok(()) => Errno::EIO,
        Err(e) => e,
    }
}

/// Finds `name` in `$PATH` (names containing `/` are used as given).
pub fn find_in_path(name: &str) -> Option<String> {
    if name.contains('/') {
        return crate::fs::exists(name).then(|| String::from(name));
    }
    let path = crate::env::var("PATH").unwrap_or("/bin:/sbin:/usr/bin:/usr/local/bin");
    path.split(':')
        .map(|dir| crate::fs::join(dir, name))
        .find(|p| crate::fs::exists(p))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    Exited(i32),
    Signaled(u32),
}

impl ExitStatus {
    pub fn from_raw(status: i32) -> ExitStatus {
        if wifsignaled(status) {
            ExitStatus::Signaled(wtermsig(status))
        } else {
            ExitStatus::Exited(wexitstatus(status))
        }
    }

    /// Shell-style code: exit code, or 128 + signal number.
    pub fn code(self) -> i32 {
        match self {
            ExitStatus::Exited(c) => c,
            ExitStatus::Signaled(s) => 128 + s as i32,
        }
    }

    pub fn success(self) -> bool {
        self == ExitStatus::Exited(0)
    }
}

/// Waits for a child (`pid = -1` for any); retries on EINTR.
pub fn wait(pid: i32) -> Result<(Pid, ExitStatus)> {
    loop {
        match sys::wait4(pid, 0) {
            Ok((p, status)) => return Ok((p, ExitStatus::from_raw(status))),
            Err(Errno::EINTR) => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Non-blocking wait; `Ok(None)` if no child has exited yet.
pub fn try_wait(pid: i32) -> Result<Option<(Pid, ExitStatus)>> {
    let (p, status) = sys::wait4(pid, WNOHANG)?;
    Ok((p != 0).then(|| (p, ExitStatus::from_raw(status))))
}

/// Runs a program and waits for it.
pub fn run(path: &str, args: &[&str]) -> Result<ExitStatus> {
    match fork()? {
        None => {
            let e = exec(path, args, &crate::env::envp());
            crate::eprintln!("{}: {}", path, e);
            exit(127)
        }
        Some(pid) => Ok(wait(pid as i32)?.1),
    }
}

pub fn getpid() -> Pid {
    sys::getpid()
}

pub fn set_foreground(fd: i32, pgrp: Pid) -> Result<()> {
    let p = pgrp as i32;
    sys::ioctl(fd, TIOCSPGRP, &p as *const i32 as usize).map(drop)
}

pub fn foreground(fd: i32) -> Result<Pid> {
    let mut p = 0i32;
    sys::ioctl(fd, TIOCGPGRP, &mut p as *mut i32 as usize)?;
    Ok(p as Pid)
}
