//! Processes: user address spaces, executable loading, process lifecycle
//! (fork/exec/exit/wait), signals and safe access to user memory.

pub mod alarm;
pub mod exec;
pub mod futex;
pub mod lifecycle;
pub mod mm;
pub mod signal;
pub mod uaccess;

use crate::arch::cpu::{self, wrmsr, MSR_FS_BASE};
use crate::arch::TrapFrame;
use crate::task::{self, Pid, Task};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use huldra_abi::signal::*;

/// Per-process bookkeeping that is not needed by the scheduler.
pub struct ProcState {
    pub ppid: Pid,
    pub pgid: Pid,
    pub sid: Pid,
    /// Normalized absolute path of the working directory.
    pub cwd: String,
    pub umask: u32,
    pub cmdline: Vec<String>,
    pub children: Vec<Pid>,
    /// Path of the running executable (/proc/self/exe).
    pub exe: String,
}

impl ProcState {
    pub fn kernel() -> ProcState {
        ProcState {
            ppid: 0,
            pgid: 0,
            sid: 0,
            cwd: String::from("/"),
            umask: 0o022,
            cmdline: Vec::new(),
            children: Vec::new(),
            exe: String::new(),
        }
    }
}

/// Loads `next`'s page table and thread pointer (called by the scheduler).
pub fn switch_address_space(_prev: &Arc<Task>, next: &Arc<Task>) {
    let cr3 = match next.cr3.load(Ordering::Acquire) {
        0 => crate::mm::vmm::kernel_page_table().root(),
        root => root,
    };
    if cpu::read_cr3() & !0xFFF != cr3 {
        unsafe { cpu::write_cr3(cr3) };
    }
    if next.is_user() {
        unsafe { wrmsr(MSR_FS_BASE, next.fs_base.load(Ordering::Relaxed)) };
    }
}

/// Current working directory of the running task.
pub fn cwd() -> String {
    task::current().proc.lock().cwd.clone()
}

/// Resolves a user-supplied path against the working directory.
pub fn resolve(path: &str) -> crate::fs::KResult<String> {
    crate::fs::vfs::normalize(&cwd(), path)
}

/// Process details shown by /proc.
#[derive(Default)]
pub struct PsInfo {
    pub ppid: Pid,
    pub pgid: Pid,
    pub sid: Pid,
    pub cmdline: Vec<String>,
    pub vm_bytes: u64,
    pub open_files: usize,
}

pub fn ps_info(t: &Task) -> PsInfo {
    let p = t.proc.lock();
    let mut info = PsInfo {
        ppid: p.ppid,
        pgid: p.pgid,
        sid: p.sid,
        cmdline: if p.cmdline.is_empty() {
            alloc::vec![t.name()]
        } else {
            p.cmdline.clone()
        },
        ..PsInfo::default()
    };
    drop(p);
    info.vm_bytes = t.mm.lock().as_ref().map_or(0, |m| m.vm_bytes());
    info.open_files = t.files.lock().iter().count();
    info
}

/// Page fault handler. Returns false if the fault is a kernel bug.
pub fn handle_page_fault(frame: &mut TrapFrame) -> bool {
    let addr = cpu::read_cr2();
    let write = frame.error & 2 != 0;
    let exec = frame.error & 16 != 0;
    let me = task::current();
    if crate::mm::is_user_address(addr)
        && me
            .mm
            .lock()
            .as_mut()
            .is_some_and(|mm| mm.handle_fault(addr, write, exec))
    {
        return true;
    }
    if frame.from_user() {
        let area = me.mm.lock().as_ref().and_then(|mm| mm.find(addr).copied());
        if let Some(v) = area {
            kdebug!("fault in area {:#x}..{:#x} prot {} {:?}", v.start, v.end, v.prot, v.kind);
        }
        kinfo!(
            "{}[{}]: segfault at {:#x} ip {:#x} ({})",
            me.name(),
            me.pid,
            addr,
            frame.rip,
            if write {
                "write"
            } else if exec {
                "exec"
            } else {
                "read"
            }
        );
        signal::force(&me, SIGSEGV);
        return true;
    }
    false
}

/// CPU exceptions raised by user code become signals.
pub fn handle_user_exception(frame: &mut TrapFrame) -> bool {
    if !frame.from_user() {
        return false;
    }
    let sig = match frame.vector {
        0 | 16 | 19 => SIGFPE,
        6 => SIGILL,
        3 | 1 => SIGTRAP,
        13 | 12 | 11 | 10 | 17 => SIGSEGV,
        _ => return false,
    };
    let me = task::current();
    kinfo!(
        "{}[{}]: {} at ip {:#x}",
        me.name(),
        me.pid,
        huldra_abi::signal::name(sig),
        frame.rip
    );
    signal::force(&me, sig);
    true
}

/// Work done right before returning to user mode: deliver signals.
pub fn return_to_user(frame: &mut TrapFrame) {
    if frame.from_user() {
        signal::deliver(frame);
    }
}
