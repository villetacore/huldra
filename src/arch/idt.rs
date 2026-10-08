//! Interrupt Descriptor Table.

use super::gdt::{DescriptorTablePointer, DOUBLE_FAULT_IST, KERNEL_CODE};
use core::arch::asm;
use core::mem::size_of;
use core::ptr::{addr_of, addr_of_mut};

/// Vector used for system calls (`int 0x80`), callable from ring 3.
pub const SYSCALL_VECTOR: usize = 0x80;

const INTERRUPT_GATE: u8 = 0x8E; // present, DPL 0
const USER_INTERRUPT_GATE: u8 = 0xEE; // present, DPL 3

#[repr(C)]
#[derive(Clone, Copy)]
struct Gate {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_mid: u16,
    offset_high: u32,
    _zero: u32,
}

impl Gate {
    const EMPTY: Gate = Gate {
        offset_low: 0,
        selector: 0,
        ist: 0,
        attributes: 0,
        offset_mid: 0,
        offset_high: 0,
        _zero: 0,
    };

    fn new(handler: u64, ist: u8, attributes: u8) -> Gate {
        Gate {
            offset_low: handler as u16,
            selector: KERNEL_CODE,
            ist,
            attributes,
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            _zero: 0,
        }
    }
}

static mut IDT: [Gate; 256] = [Gate::EMPTY; 256];

extern "C" {
    /// Entry stubs for vectors 0..48 (exceptions and PIC IRQs), see trap.S.
    static isr_stub_table: [u64; 48];
    fn isr_stub_128();
}

pub fn init() {
    unsafe {
        let idt = &mut *addr_of_mut!(IDT);
        let stubs = &*addr_of!(isr_stub_table);
        for (vector, &handler) in stubs.iter().enumerate() {
            let ist = if vector == 8 { DOUBLE_FAULT_IST } else { 0 };
            idt[vector] = Gate::new(handler, ist, INTERRUPT_GATE);
        }
        idt[SYSCALL_VECTOR] = Gate::new(isr_stub_128 as *const () as u64, 0, USER_INTERRUPT_GATE);

        let ptr = DescriptorTablePointer {
            limit: (size_of::<[Gate; 256]>() - 1) as u16,
            base: idt.as_ptr() as u64,
        };
        asm!("lidt [{}]", in(reg) &ptr, options(readonly, nostack, preserves_flags));
    }
}
