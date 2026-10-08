//! Global Descriptor Table and Task State Segment.
//!
//! Selector layout is compatible with `syscall`/`sysret`:
//! kernel code, kernel data, user data, user code, TSS.

use core::arch::asm;
use core::mem::size_of;
use core::ptr::{addr_of, addr_of_mut};

pub const KERNEL_CODE: u16 = 0x08;
pub const KERNEL_DATA: u16 = 0x10;
#[allow(dead_code)]
pub const USER_DATA: u16 = 0x18 | 3;
#[allow(dead_code)]
pub const USER_CODE: u16 = 0x20 | 3;
pub const TSS_SELECTOR: u16 = 0x28;

/// IST slot used by the double-fault handler (1-based, as in the IDT).
pub const DOUBLE_FAULT_IST: u8 = 1;

const STACK_SIZE: usize = 16 * 1024;

#[repr(C, packed)]
pub struct DescriptorTablePointer {
    pub limit: u16,
    pub base: u64,
}

#[repr(C, packed)]
struct Tss {
    _reserved0: u32,
    rsp: [u64; 3],
    _reserved1: u64,
    ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    iomap_base: u16,
}

#[repr(C, align(16))]
struct Stack([u8; STACK_SIZE]);

static mut DOUBLE_FAULT_STACK: Stack = Stack([0; STACK_SIZE]);

static mut TSS: Tss = Tss {
    _reserved0: 0,
    rsp: [0; 3],
    _reserved1: 0,
    ist: [0; 7],
    _reserved2: 0,
    _reserved3: 0,
    iomap_base: size_of::<Tss>() as u16,
};

static mut GDT: [u64; 7] = [
    0,
    0x00AF_9A00_0000_FFFF, // kernel code (64-bit)
    0x00CF_9200_0000_FFFF, // kernel data
    0x00CF_F200_0000_FFFF, // user data (DPL 3)
    0x00AF_FA00_0000_FFFF, // user code (DPL 3, 64-bit)
    0,                     // TSS (low)
    0,                     // TSS (high)
];

fn tss_descriptor(base: u64, limit: u64) -> (u64, u64) {
    let low = (limit & 0xFFFF)
        | ((base & 0xFF_FFFF) << 16)
        | (0x89 << 40) // present, 64-bit available TSS
        | (((limit >> 16) & 0xF) << 48)
        | (((base >> 24) & 0xFF) << 56);
    (low, base >> 32)
}

pub fn init() {
    unsafe {
        let df_top = addr_of!(DOUBLE_FAULT_STACK) as u64 + STACK_SIZE as u64;
        let tss = addr_of_mut!(TSS);
        (*tss).ist = [df_top, 0, 0, 0, 0, 0, 0];

        let gdt = &mut *addr_of_mut!(GDT);
        let (low, high) = tss_descriptor(tss as u64, size_of::<Tss>() as u64 - 1);
        gdt[5] = low;
        gdt[6] = high;

        let ptr = DescriptorTablePointer {
            limit: (size_of::<[u64; 7]>() - 1) as u16,
            base: gdt.as_ptr() as u64,
        };
        asm!("lgdt [{}]", in(reg) &ptr, options(readonly, nostack, preserves_flags));

        // Reload CS with a far return, then the data segments.
        asm!(
            "push {cs}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            "mov ds, {ds:x}",
            "mov es, {ds:x}",
            "mov ss, {ds:x}",
            "mov fs, {zero:x}",
            "mov gs, {zero:x}",
            cs = in(reg) KERNEL_CODE as u64,
            ds = in(reg) KERNEL_DATA as u64,
            zero = in(reg) 0u64,
            tmp = out(reg) _,
        );

        asm!("ltr {:x}", in(reg) TSS_SELECTOR, options(nomem, nostack, preserves_flags));
    }
}

/// Stack loaded on a ring 3 -> ring 0 transition (for future user mode).
#[allow(dead_code)]
pub fn set_kernel_stack(top: u64) {
    unsafe { (*addr_of_mut!(TSS)).rsp = [top, 0, 0] }
}
