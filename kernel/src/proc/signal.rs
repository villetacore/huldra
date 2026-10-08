//! POSIX signals.
//!
//! Sending sets a bit in the target's pending mask and wakes it if it is
//! blocked in an interruptible wait (the wait then returns EINTR). Signals
//! are delivered on the way back to user mode: the default action runs in
//! the kernel (terminate or ignore; stopping is not supported yet), a user
//! handler gets a signal frame pushed on the user stack and returns through
//! `rt_sigreturn`.

use super::uaccess::{read_user, write_user};
use crate::arch::TrapFrame;
use crate::fs::KResult;
use crate::task::{self, sched, Pid, Task};
use alloc::sync::Arc;
use core::mem::size_of;
use huldra_abi::errno::Errno;
use huldra_abi::signal::*;

pub struct SignalState {
    pub pending: u64,
    pub blocked: u64,
    pub actions: [SigAction; NSIG as usize],
}

/// Signals that can be neither caught nor blocked.
const UNBLOCKABLE: u64 = sigbit(SIGKILL) | sigbit(SIGSTOP);

impl SignalState {
    pub fn new() -> SignalState {
        SignalState {
            pending: 0,
            blocked: 0,
            actions: [SigAction::default(); NSIG as usize],
        }
    }

    /// State inherited by a forked child (pending signals are not).
    pub fn fork(&self) -> SignalState {
        SignalState {
            pending: 0,
            blocked: self.blocked,
            actions: self.actions,
        }
    }

    /// After exec: handlers reset to default, ignored stays ignored.
    pub fn exec(&mut self) {
        for a in self.actions.iter_mut() {
            if a.handler != SIG_IGN {
                *a = SigAction::default();
            }
        }
    }

    fn ignores(&self, sig: u32) -> bool {
        let a = &self.actions[sig as usize - 1];
        a.handler == SIG_IGN || (a.handler == SIG_DFL && default_ignored(sig))
    }

    /// Pending, unblocked signals that would actually do something.
    fn deliverable(&self) -> u64 {
        let mut mask = self.pending & !self.blocked;
        let mut bits = mask;
        while bits != 0 {
            let sig = bits.trailing_zeros() + 1;
            bits &= bits - 1;
            if self.ignores(sig) {
                mask &= !sigbit(sig);
            }
        }
        mask
    }
}

fn default_ignored(sig: u32) -> bool {
    // Stop signals are ignored until job control is implemented.
    matches!(
        sig,
        SIGCHLD | SIGCONT | SIGWINCH | SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU | 23
    )
}

/// True if the current task has a signal that should interrupt a wait.
pub fn current_has_pending() -> bool {
    if !sched::is_running() {
        return false;
    }
    let me = sched::current();
    let s = me.signals.lock();
    s.deliverable() != 0
}

/// Queues `sig` for `task`.
pub fn send(task: &Arc<Task>, sig: u32) {
    if sig == 0 || sig > NSIG {
        return;
    }
    let wake = {
        let mut s = task.signals.lock();
        if s.ignores(sig) && sig != SIGKILL {
            return;
        }
        s.pending |= sigbit(sig);
        s.deliverable() != 0
    };
    if wake && task.state() == task::State::Blocked {
        sched::make_runnable(task);
    }
}

/// Delivers a synchronous fault signal even if it is blocked or ignored.
pub fn force(task: &Arc<Task>, sig: u32) {
    {
        let mut s = task.signals.lock();
        if s.actions[sig as usize - 1].handler == SIG_IGN || s.blocked & sigbit(sig) != 0 {
            s.actions[sig as usize - 1] = SigAction::default();
            s.blocked &= !sigbit(sig);
        }
        s.pending |= sigbit(sig);
    }
}

/// Sends `sig` to every process in process group `pgrp`.
pub fn send_to_group(pgrp: Pid, sig: u32) -> usize {
    let mut n = 0;
    for t in task::all_tasks() {
        if t.is_user() && t.proc.lock().pgid == pgrp {
            send(&t, sig);
            n += 1;
        }
    }
    n
}

/// Sends `sig` to the current process.
pub fn send_current(sig: u32) {
    send(&sched::current(), sig);
}

/// `kill(2)` target selection.
pub fn kill(pid: i64, sig: u32) -> KResult<()> {
    if sig > NSIG {
        return Err(Errno::EINVAL);
    }
    let me = sched::current();
    let targets: alloc::vec::Vec<Arc<Task>> = if pid > 0 {
        task::lookup(pid as Pid)
            .filter(|t| t.is_user())
            .into_iter()
            .collect()
    } else if pid == 0 {
        let pg = me.proc.lock().pgid;
        task::all_tasks()
            .into_iter()
            .filter(|t| t.is_user() && t.proc.lock().pgid == pg)
            .collect()
    } else if pid == -1 {
        task::all_tasks()
            .into_iter()
            .filter(|t| t.is_user() && t.pid != 1 && t.pid != me.pid)
            .collect()
    } else {
        let pg = (-pid) as Pid;
        task::all_tasks()
            .into_iter()
            .filter(|t| t.is_user() && t.proc.lock().pgid == pg)
            .collect()
    };
    if targets.is_empty() {
        return Err(Errno::ESRCH);
    }
    for t in targets.iter().filter(|t| t.state() != task::State::Zombie) {
        send(t, sig);
    }
    Ok(())
}

pub fn sigaction(sig: u32, new: Option<SigAction>) -> KResult<SigAction> {
    if sig == 0 || sig > NSIG || ((sig == SIGKILL || sig == SIGSTOP) && new.is_some()) {
        return Err(Errno::EINVAL);
    }
    let me = sched::current();
    let mut s = me.signals.lock();
    let old = s.actions[sig as usize - 1];
    if let Some(mut a) = new {
        a.mask &= !UNBLOCKABLE;
        s.actions[sig as usize - 1] = a;
        if a.handler == SIG_IGN {
            s.pending &= !sigbit(sig);
        }
    }
    Ok(old)
}

pub fn sigprocmask(how: u32, set: Option<u64>) -> KResult<u64> {
    let me = sched::current();
    let mut s = me.signals.lock();
    let old = s.blocked;
    if let Some(set) = set {
        s.blocked = match how {
            SIG_BLOCK => old | set,
            SIG_UNBLOCK => old & !set,
            SIG_SETMASK => set,
            _ => return Err(Errno::EINVAL),
        } & !UNBLOCKABLE;
    }
    Ok(old)
}

/// Pushed on the user stack when a handler runs; restored by sigreturn.
#[repr(C)]
#[derive(Clone, Copy)]
struct SignalFrame {
    regs: TrapFrame,
    blocked: u64,
    magic: u64,
}

const FRAME_MAGIC: u64 = 0x4855_4C44_5349_4721; // "HULDSIG!"

/// Action for the next deliverable signal of the current task, if any:
/// used by the syscall layer to decide whether to restart a syscall.
pub fn next_action() -> Option<SigAction> {
    let me = sched::current();
    let s = me.signals.lock();
    let d = s.deliverable();
    (d != 0).then(|| s.actions[d.trailing_zeros() as usize])
}

/// Delivers pending signals before returning to user mode.
pub fn deliver(frame: &mut TrapFrame) {
    let me = sched::current();
    loop {
        let (sig, action) = {
            let mut s = me.signals.lock();
            let d = s.pending & !s.blocked;
            if d == 0 {
                return;
            }
            let sig = d.trailing_zeros() + 1;
            s.pending &= !sigbit(sig);
            (sig, s.actions[sig as usize - 1])
        };
        match action.handler {
            SIG_IGN => continue,
            SIG_DFL if default_ignored(sig) => continue,
            SIG_DFL => super::lifecycle::exit_process(huldra_abi::process::signal_status(sig)),
            _ => {
                if setup_frame(&me, frame, sig, &action).is_err() {
                    super::lifecycle::exit_process(huldra_abi::process::signal_status(SIGSEGV));
                }
                return;
            }
        }
    }
}

fn setup_frame(me: &Arc<Task>, frame: &mut TrapFrame, sig: u32, action: &SigAction) -> KResult<()> {
    if action.flags & SA_RESTORER == 0 || action.restorer == 0 {
        return Err(Errno::EFAULT);
    }
    let blocked = me.signals.lock().blocked;
    // Skip the red zone, then align so the handler starts like after a call.
    let frame_addr = (frame.rsp - 128 - size_of::<SignalFrame>() as u64) & !15;
    let ret_addr = frame_addr - 8;
    write_user(
        frame_addr,
        &SignalFrame {
            regs: *frame,
            blocked,
            magic: FRAME_MAGIC,
        },
    )?;
    write_user(ret_addr, &action.restorer)?;

    {
        let mut s = me.signals.lock();
        s.blocked |= action.mask & !UNBLOCKABLE;
        if action.flags & SA_NODEFER == 0 {
            s.blocked |= sigbit(sig);
        }
        if action.flags & SA_RESETHAND != 0 {
            s.actions[sig as usize - 1] = SigAction::default();
        }
    }
    frame.rip = action.handler;
    frame.rsp = ret_addr;
    frame.rdi = sig as u64;
    frame.rsi = 0;
    frame.rdx = 0;
    frame.rax = 0;
    Ok(())
}

/// `rt_sigreturn`: restores the context saved by `setup_frame`.
pub fn sigreturn(frame: &mut TrapFrame) -> KResult<()> {
    // The handler's `ret` popped the return address: rsp points at the frame.
    let saved: SignalFrame = read_user(frame.rsp)?;
    if saved.magic != FRAME_MAGIC {
        return Err(Errno::EFAULT);
    }
    let (cs, ss) = (frame.cs, frame.ss);
    *frame = saved.regs;
    // Never let user space change privilege-related state.
    frame.cs = cs;
    frame.ss = ss;
    // CF, PF, AF, ZF, SF, DF, OF; interrupts always on.
    const USER_FLAGS: u64 = 0xCD5;
    frame.rflags = (frame.rflags & USER_FLAGS) | 0x202;
    if !crate::mm::is_user_address(frame.rip) {
        return Err(Errno::EFAULT);
    }
    sched::current().signals.lock().blocked = saved.blocked & !UNBLOCKABLE;
    Ok(())
}
