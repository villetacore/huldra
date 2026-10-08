//! Fast system call entry (`syscall`/`sysret`).

use super::cpu::*;
use super::gdt::KERNEL_CODE;

extern "C" {
    fn syscall_entry();
}

pub fn init() {
    unsafe {
        wrmsr(MSR_EFER, rdmsr(MSR_EFER) | EFER_SCE);
        // sysret loads CS = STAR[63:48] + 16 and SS = STAR[63:48] + 8 (RPL 3):
        // with 0x10 that is user code 0x20 and user data 0x18.
        wrmsr(MSR_STAR, (0x10u64 << 48) | ((KERNEL_CODE as u64) << 32));
        wrmsr(MSR_LSTAR, syscall_entry as *const () as u64);
        // Clear IF, DF, TF and AC on entry.
        wrmsr(MSR_SFMASK, 0x4_0700);
    }
}
