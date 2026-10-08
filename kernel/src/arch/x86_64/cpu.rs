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
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags))
    }
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
    __cpuid(0x8000_0000).eax >= 0x8000_0001 && __cpuid(0x8000_0001).edx & (1 << 20) != 0
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

/// Text for /proc/cpuinfo.
pub fn cpuinfo() -> alloc::string::String {
    use alloc::string::String;
    use core::fmt::Write;
    let regs = __cpuid;
    let mut vendor = [0u8; 12];
    let v = regs(0);
    for (i, r) in [v.ebx, v.edx, v.ecx].iter().enumerate() {
        vendor[i * 4..i * 4 + 4].copy_from_slice(&r.to_le_bytes());
    }
    let mut brand = [0u8; 48];
    if regs(0x8000_0000).eax >= 0x8000_0004 {
        for (i, leaf) in (0x8000_0002..=0x8000_0004).enumerate() {
            let r = regs(leaf);
            for (j, x) in [r.eax, r.ebx, r.ecx, r.edx].iter().enumerate() {
                brand[i * 16 + j * 4..i * 16 + j * 4 + 4].copy_from_slice(&x.to_le_bytes());
            }
        }
    }
    let f = regs(1);
    let mut flags = String::new();
    let features = [
        (f.edx, 0, "fpu"),
        (f.edx, 4, "tsc"),
        (f.edx, 5, "msr"),
        (f.edx, 6, "pae"),
        (f.edx, 9, "apic"),
        (f.edx, 13, "pge"),
        (f.edx, 25, "sse"),
        (f.edx, 26, "sse2"),
        (f.ecx, 0, "sse3"),
        (f.ecx, 21, "x2apic"),
        (f.ecx, 31, "hypervisor"),
    ];
    for (reg, bit, name) in features {
        if reg & (1 << bit) != 0 {
            flags.push_str(name);
            flags.push(' ');
        }
    }
    if has_nx() {
        flags.push_str("nx");
    }
    let mut s = String::new();
    let _ = writeln!(s, "processor\t: 0");
    let _ = writeln!(
        s,
        "vendor_id\t: {}",
        core::str::from_utf8(&vendor).unwrap_or("?")
    );
    let _ = writeln!(s, "cpu family\t: {}", (f.eax >> 8) & 0xF);
    let _ = writeln!(s, "model\t\t: {}", (f.eax >> 4) & 0xF);
    let _ = writeln!(
        s,
        "model name\t: {}",
        core::str::from_utf8(&brand)
            .unwrap_or("?")
            .trim_matches(|c| c == '\0' || c == ' ')
    );
    let _ = writeln!(s, "stepping\t: {}", f.eax & 0xF);
    let _ = writeln!(s, "flags\t\t: {}", flags.trim_end());
    s
}
