//! Process system calls.

use super::{value, Args, Ret};
use crate::fs::KResult;
use crate::proc::{lifecycle, uaccess};
use crate::task::sched;
use core::sync::atomic::Ordering;
use huldra_abi::errno::Errno;
use huldra_abi::mm::{ARCH_GET_FS, ARCH_SET_FS};

pub fn fork(a: &mut Args) -> KResult<Ret> {
    value(lifecycle::fork(a.frame)? as u64)
}

pub fn execve(a: &mut Args) -> KResult<Ret> {
    let path = crate::proc::resolve(&uaccess::read_path(a.a0())?)?;
    let argv = uaccess::read_string_array(a.a1())?;
    let envp = uaccess::read_string_array(a.a2())?;
    *a.frame = crate::proc::exec::exec(&path, argv, envp)?;
    Ok(Ret::FrameReplaced)
}

pub fn exit(a: &mut Args) -> KResult<Ret> {
    lifecycle::exit_process(huldra_abi::process::exit_status(a.a0() as i32))
}

pub fn wait4(a: &mut Args) -> KResult<Ret> {
    let (pid, status) = lifecycle::wait(a.a0() as i32 as i64, a.a2() as u32)?;
    if a.a1() != 0 && pid != 0 {
        uaccess::write_user(a.a1(), &status)?;
    }
    value(pid as u64)
}

pub fn setpgid(a: &mut Args) -> KResult<Ret> {
    lifecycle::setpgid(a.a0() as u32, a.a1() as u32)?;
    value(0)
}

pub fn getpgid_of(pid: u32) -> KResult<Ret> {
    value(lifecycle::getpgid(pid)? as u64)
}

pub fn getppid(_a: &mut Args) -> KResult<Ret> {
    value(sched::current().proc.lock().ppid as u64)
}

pub fn setsid(_a: &mut Args) -> KResult<Ret> {
    value(lifecycle::setsid()? as u64)
}

pub fn getsid(a: &mut Args) -> KResult<Ret> {
    let t = if a.a0() == 0 {
        sched::current()
    } else {
        crate::task::lookup(a.a0() as u32).ok_or(Errno::ESRCH)?
    };
    let sid = t.proc.lock().sid;
    value(sid as u64)
}

pub fn arch_prctl(a: &mut Args) -> KResult<Ret> {
    let me = sched::current();
    match a.a0() as u32 {
        ARCH_SET_FS => {
            if !crate::mm::is_user_address(a.a1()) {
                return Err(Errno::EPERM);
            }
            me.fs_base.store(a.a1(), Ordering::Relaxed);
            unsafe { crate::arch::cpu::wrmsr(crate::arch::cpu::MSR_FS_BASE, a.a1()) };
            value(0)
        }
        ARCH_GET_FS => {
            uaccess::write_user(a.a1(), &me.fs_base.load(Ordering::Relaxed))?;
            value(0)
        }
        _ => Err(Errno::EINVAL),
    }
}
