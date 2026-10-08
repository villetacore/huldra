//! Per-CPU data, reached through the GS segment base in kernel mode.
//!
//! Only one CPU is brought up today, but everything that is per-CPU by
//! nature (the kernel stack for syscall entry, the scratch slot for the user
//! stack pointer) lives here so SMP can be added without reshuffling.

use super::cpu::{wrmsr, MSR_GS_BASE, MSR_KERNEL_GS_BASE};
use core::ptr::addr_of_mut;

/// Layout is shared with entry.S / syscall.S: keep offsets in sync.
#[repr(C)]
pub struct PerCpu {
    /// gs:0 — pointer to this structure.
    pub self_ptr: u64,
    /// gs:8 — top of the current task's kernel stack.
    pub kernel_rsp: u64,
    /// gs:16 — user RSP saved by the syscall entry.
    pub user_rsp: u64,
    pub cpu_id: u32,
}

static mut BSP: PerCpu = PerCpu {
    self_ptr: 0,
    kernel_rsp: 0,
    user_rsp: 0,
    cpu_id: 0,
};

pub fn init() {
    unsafe {
        let p = addr_of_mut!(BSP);
        (*p).self_ptr = p as u64;
        wrmsr(MSR_GS_BASE, p as u64);
        wrmsr(MSR_KERNEL_GS_BASE, 0);
    }
}

fn this_cpu() -> *mut PerCpu {
    let p: u64;
    unsafe {
        core::arch::asm!("mov {}, gs:[0]", out(reg) p, options(nostack, preserves_flags, readonly))
    };
    p as *mut PerCpu
}

/// Sets the stack used when entering the kernel from user mode
/// (both the TSS RSP0 for interrupts and the syscall entry stack).
pub fn set_kernel_stack(top: u64) {
    unsafe { (*this_cpu()).kernel_rsp = top };
    super::gdt::set_kernel_stack(top);
}
