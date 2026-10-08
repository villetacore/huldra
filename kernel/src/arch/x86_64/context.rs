//! Task contexts: initial kernel stacks and context switching.

use super::gdt::{USER_CODE, USER_DATA};
use super::trap::TrapFrame;
use core::mem::size_of;

extern "C" {
    fn switch_context(save_rsp: *mut u64, new_rsp: u64);
    fn kthread_trampoline();
    fn user_trampoline();
}

/// Saved stack pointer of a task that is not running.
#[derive(Default)]
#[repr(transparent)]
pub struct Context {
    rsp: u64,
}

/// Callee-saved registers pushed by `switch_context`, lowest address first.
#[repr(C)]
struct SwitchFrame {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    rbx: u64,
    rbp: u64,
    rflags: u64,
    ret: u64,
}

const RFLAGS_RESERVED: u64 = 1 << 1;
const RFLAGS_IF: u64 = 1 << 9;

unsafe fn push_switch_frame(rsp: u64, frame: SwitchFrame) -> u64 {
    let rsp = rsp - size_of::<SwitchFrame>() as u64;
    (rsp as *mut SwitchFrame).write(frame);
    rsp
}

impl Context {
    /// Context that starts `entry(arg)` on the stack ending at `stack_top`.
    pub fn new_kernel(stack_top: u64, entry: extern "C" fn(u64) -> i32, arg: u64) -> Context {
        // Leave 16 bytes so that `call entry` happens with a 16-byte aligned stack.
        let frame = SwitchFrame {
            r15: 0,
            r14: 0,
            r13: entry as *const () as u64,
            r12: arg,
            rbx: 0,
            rbp: 0,
            rflags: RFLAGS_RESERVED,
            ret: kthread_trampoline as *const () as u64,
        };
        Context { rsp: unsafe { push_switch_frame(stack_top - 16, frame) } }
    }

    /// Context that enters user mode with the register state `frame`.
    pub fn new_user(stack_top: u64, frame: &TrapFrame) -> Context {
        unsafe {
            let tf = stack_top - size_of::<TrapFrame>() as u64;
            core::ptr::copy_nonoverlapping(frame, tf as *mut TrapFrame, 1);
            let sf = SwitchFrame {
                r15: 0,
                r14: 0,
                r13: 0,
                r12: 0,
                rbx: 0,
                rbp: 0,
                rflags: RFLAGS_RESERVED,
                ret: user_trampoline as *const () as u64,
            };
            Context { rsp: push_switch_frame(tf, sf) }
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut u64 {
        &mut self.rsp
    }

    pub fn rsp(&self) -> u64 {
        self.rsp
    }
}

/// Saves the running context into `*save` and resumes `next`.
///
/// # Safety
/// Interrupts must be disabled; `next` must hold a valid saved context whose
/// stack stays alive until it is switched away from again.
pub unsafe fn switch(save: *mut u64, next: u64) {
    switch_context(save, next);
}

/// Initial register state for a new user program.
pub fn user_entry_frame(entry: u64, stack: u64) -> TrapFrame {
    TrapFrame {
        rip: entry,
        cs: USER_CODE as u64,
        rflags: RFLAGS_RESERVED | RFLAGS_IF,
        rsp: stack,
        ss: USER_DATA as u64,
        ..TrapFrame::default()
    }
}
