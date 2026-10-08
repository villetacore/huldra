//! Signal system calls.

use super::{value, Args, Ret};
use crate::fs::KResult;
use crate::proc::{signal, uaccess};
use huldra_abi::errno::Errno;
use huldra_abi::signal::SigAction;

pub fn rt_sigaction(a: &mut Args) -> KResult<Ret> {
    if a.a3() != 8 {
        return Err(Errno::EINVAL);
    }
    let new = if a.a1() != 0 { Some(uaccess::read_user::<SigAction>(a.a1())?) } else { None };
    let old = signal::sigaction(a.a0() as u32, new)?;
    if a.a2() != 0 {
        uaccess::write_user(a.a2(), &old)?;
    }
    value(0)
}

pub fn rt_sigprocmask(a: &mut Args) -> KResult<Ret> {
    if a.a3() != 8 {
        return Err(Errno::EINVAL);
    }
    let set = if a.a1() != 0 { Some(uaccess::read_user::<u64>(a.a1())?) } else { None };
    let old = signal::sigprocmask(a.a0() as u32, set)?;
    if a.a2() != 0 {
        uaccess::write_user(a.a2(), &old)?;
    }
    value(0)
}

pub fn rt_sigreturn(a: &mut Args) -> KResult<Ret> {
    if signal::sigreturn(a.frame).is_err() {
        crate::proc::lifecycle::exit_process(huldra_abi::process::signal_status(huldra_abi::signal::SIGSEGV));
    }
    Ok(Ret::FrameReplaced)
}

pub fn kill(a: &mut Args) -> KResult<Ret> {
    signal::kill(a.a0() as i32 as i64, a.a1() as u32)?;
    value(0)
}
