//! Local APIC and I/O APIC (placeholder until ACPI support lands).

pub const TIMER_VECTOR: u8 = 0xEF;
pub const SPURIOUS_VECTOR: u8 = 0xFF;

pub fn ioapic_unmask(_irq: u8) {}

pub fn eoi() {}

pub fn timer_ticks() -> u64 {
    0
}

pub fn timer_interrupt() {}
