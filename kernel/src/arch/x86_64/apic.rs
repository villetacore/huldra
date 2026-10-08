//! Local APIC (per-CPU timer and interrupt acknowledgement) and I/O APIC
//! (routing of device interrupts). Found through the ACPI MADT; the kernel
//! keeps using the legacy PIC and PIT if they are not available.

use super::acpi::{self, Madt, Override};
use super::cpu::{rdmsr, wrmsr};
use super::irq::IRQ_BASE;
use super::port::{inb, outb};
use crate::mm::phys_to_virt;
use crate::sync::{Once, SpinLock};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

pub const TIMER_VECTOR: u8 = 0xEF;
pub const SPURIOUS_VECTOR: u8 = 0xFF;

const MSR_APIC_BASE: u32 = 0x1B;
const LAPIC_ID: u64 = 0x20;
const LAPIC_EOI: u64 = 0xB0;
const LAPIC_SVR: u64 = 0xF0;
const LAPIC_LVT_TIMER: u64 = 0x320;
const LAPIC_TIMER_INIT: u64 = 0x380;
const LAPIC_TIMER_CURRENT: u64 = 0x390;
const LAPIC_TIMER_DIVIDE: u64 = 0x3E0;
const TIMER_PERIODIC: u32 = 1 << 17;
const MASKED: u32 = 1 << 16;

struct Apic {
    lapic: u64,
    io_apics: Vec<acpi::IoApic>,
    overrides: Vec<Override>,
}

static APIC: Once<Apic> = Once::new();
static IOAPIC_LOCK: SpinLock<()> = SpinLock::new(());
static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);

fn lapic_write(reg: u64, value: u32) {
    let base = APIC.expect("APIC").lapic;
    unsafe { ((phys_to_virt(base) + reg) as *mut u32).write_volatile(value) }
}

fn lapic_read(reg: u64) -> u32 {
    let base = APIC.expect("APIC").lapic;
    unsafe { ((phys_to_virt(base) + reg) as *const u32).read_volatile() }
}

fn ioapic_write(base: u64, reg: u32, value: u32) {
    let _g = IOAPIC_LOCK.lock();
    unsafe {
        (phys_to_virt(base) as *mut u32).write_volatile(reg);
        ((phys_to_virt(base) + 0x10) as *mut u32).write_volatile(value);
    }
}

fn ioapic_read(base: u64, reg: u32) -> u32 {
    let _g = IOAPIC_LOCK.lock();
    unsafe {
        (phys_to_virt(base) as *mut u32).write_volatile(reg);
        ((phys_to_virt(base) + 0x10) as *const u32).read_volatile()
    }
}

/// Measures LAPIC timer ticks per 10 ms using PIT channel 2.
fn calibrate_timer() -> u32 {
    const PIT_10MS: u16 = (1_193_182 / 100) as u16;
    unsafe {
        let gate = inb(0x61);
        outb(0x61, (gate & !0x02) | 0x01); // gate on, speaker off
        outb(0x43, 0xB0); // channel 2, lobyte/hibyte, mode 0
        outb(0x42, PIT_10MS as u8);
        outb(0x42, (PIT_10MS >> 8) as u8);
        let g = inb(0x61) & !0x01;
        outb(0x61, g); // restart the count
        outb(0x61, g | 0x01);
        lapic_write(LAPIC_TIMER_DIVIDE, 0x3); // divide by 16
        lapic_write(LAPIC_TIMER_INIT, u32::MAX);
        while inb(0x61) & 0x20 == 0 {}
        let elapsed = u32::MAX - lapic_read(LAPIC_TIMER_CURRENT);
        lapic_write(LAPIC_TIMER_INIT, 0);
        outb(0x61, gate);
        elapsed
    }
}

/// Brings up the APICs if the firmware describes them. Must run with
/// interrupts disabled; returns false if the legacy PIC stays in charge.
pub fn init() -> bool {
    let Some(Madt {
        local_apic,
        io_apics,
        overrides,
        cpus,
    }) = acpi::parse_madt()
    else {
        kwarn!("ACPI MADT not found, staying on the 8259 PIC");
        return false;
    };
    if io_apics.is_empty() {
        return false;
    }
    unsafe { wrmsr(MSR_APIC_BASE, rdmsr(MSR_APIC_BASE) | (1 << 11)) };
    APIC.call_once(|| Apic {
        lapic: local_apic,
        io_apics,
        overrides,
    });
    lapic_write(LAPIC_SVR, 0x100 | SPURIOUS_VECTOR as u32);

    // Mask every I/O APIC input until a driver asks for it.
    for io in &APIC.expect("APIC").io_apics {
        let max = (ioapic_read(io.address, 1) >> 16) & 0xFF;
        for i in 0..=max {
            ioapic_write(io.address, 0x10 + 2 * i, MASKED);
        }
    }

    let per_tick = calibrate_timer();
    TIMER_COUNT.call_once(|| per_tick);
    kinfo!(
        "APIC: local APIC at {:#x} (id {}), I/O APIC id {}, {} CPU(s), timer {} ticks/10ms",
        local_apic,
        lapic_read(LAPIC_ID) >> 24,
        APIC.expect("APIC").io_apics[0].id,
        cpus,
        per_tick
    );
    true
}

static TIMER_COUNT: Once<u32> = Once::new();

/// Starts the periodic local APIC timer at `crate::time::HZ`.
pub fn start_timer() {
    let count = *TIMER_COUNT.expect("APIC timer") as u64 * 100 / crate::time::HZ;
    lapic_write(LAPIC_TIMER_DIVIDE, 0x3);
    lapic_write(LAPIC_LVT_TIMER, TIMER_VECTOR as u32 | TIMER_PERIODIC);
    lapic_write(LAPIC_TIMER_INIT, count as u32);
}

/// Routes ISA `irq` (via any MADT override) to vector `IRQ_BASE + irq`.
pub fn ioapic_unmask(irq: u8) {
    let Some(apic) = APIC.get() else { return };
    let ovr = apic.overrides.iter().find(|o| o.irq == irq);
    let gsi = ovr.map_or(irq as u32, |o| o.gsi);
    let Some(io) = apic
        .io_apics
        .iter()
        .find(|io| gsi >= io.gsi_base && gsi < io.gsi_base + 24)
    else {
        return;
    };
    let mut low = (IRQ_BASE + irq) as u32;
    if ovr.is_some_and(|o| o.active_low) {
        low |= 1 << 13;
    }
    if ovr.is_some_and(|o| o.level_triggered) {
        low |= 1 << 15;
    }
    let dest = lapic_read(LAPIC_ID) >> 24;
    let entry = 0x10 + 2 * (gsi - io.gsi_base);
    ioapic_write(io.address, entry + 1, dest << 24);
    ioapic_write(io.address, entry, low);
}

pub fn eoi() {
    lapic_write(LAPIC_EOI, 0);
}

pub fn timer_ticks() -> u64 {
    TIMER_TICKS.load(Ordering::Relaxed)
}

pub fn timer_interrupt() {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
    crate::time::tick();
    crate::task::sched::timer_tick();
    eoi();
}
