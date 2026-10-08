//! Control registers, MSRs and CPU feature detection.

use core::arch::asm;
use core::arch::x86_64::__cpuid;

pub const MSR_EFER: u32 = 0xC000_0080;
pub const MSR_STAR: u32 = 0xC000_0081;
pub const MSR_LSTAR: u32 = 0xC000_0082;
pub const MSR_SFMASK: u32 = 0xC000_0084;
pub const MSR_FS_BASE: u32 = 0xC000_0100;
pub const MSR_GS_BASE: u32 = 0xC000_0101;
pub const MSR_KERNEL_GS_BASE: u32 = 0xC000_0102;

pub const EFER_SCE: u64 = 1 << 0;

pub fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    unsafe { asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags)) }
    (hi as u64) << 32 | lo as u64
}

pub unsafe fn wrmsr(msr: u32, value: u64) {
    asm!("wrmsr", in("ecx") msr, in("eax") value as u32, in("edx") (value >> 32) as u32, options(nostack, preserves_flags));
}

pub fn read_cr2() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, cr2", out(reg) v, options(nomem, nostack, preserves_flags)) }
    v
}

pub fn read_cr3() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, cr3", out(reg) v, options(nomem, nostack, preserves_flags)) }
    v
}

pub unsafe fn write_cr3(v: u64) {
    asm!("mov cr3, {}", in(reg) v, options(nostack, preserves_flags));
}

pub fn read_cr4() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, cr4", out(reg) v, options(nomem, nostack, preserves_flags)) }
    v
}

pub unsafe fn write_cr4(v: u64) {
    asm!("mov cr4, {}", in(reg) v, options(nostack, preserves_flags));
}

pub fn invlpg(addr: u64) {
    unsafe { asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags)) }
}

pub fn has_nx() -> bool {
    unsafe { __cpuid(0x8000_0000).eax >= 0x8000_0001 && __cpuid(0x8000_0001).edx & (1 << 20) != 0 }
}

/// Enables global pages (kernel mappings survive CR3 switches in the TLB).
pub fn enable_global_pages() {
    unsafe { write_cr4(read_cr4() | 1 << 7) }
}

/// Reads the time-stamp counter.
#[allow(dead_code)]
pub fn rdtsc() -> u64 {
    let (lo, hi): (u32, u32);
    unsafe { asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags)) }
    (hi as u64) << 32 | lo as u64
}
