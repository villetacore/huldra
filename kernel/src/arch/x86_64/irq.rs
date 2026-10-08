//! Hardware interrupt lines: handler registration, dispatch and statistics.
//!
//! Drivers call [`register`] with an ISA IRQ number; the active interrupt
//! controller (8259 PIC, or the I/O APIC when available) routes it to vector
//! `IRQ_BASE + irq` and is told about end-of-interrupt here.

use crate::sync::SpinLock;
use alloc::format;
use alloc::string::String;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const IRQ_BASE: u8 = 32;
pub const IRQ_LINES: usize = 24;

#[derive(Clone, Copy)]
struct Line {
    handler: Option<fn()>,
    name: &'static str,
}

static LINES: SpinLock<[Line; IRQ_LINES]> = SpinLock::new(
    [Line {
        handler: None,
        name: "",
    }; IRQ_LINES],
);
static COUNTS: [AtomicU64; IRQ_LINES] = [const { AtomicU64::new(0) }; IRQ_LINES];
static USE_APIC: AtomicBool = AtomicBool::new(false);

/// Installs `handler` for `irq` and unmasks the line.
pub fn register(irq: u8, name: &'static str, handler: fn()) {
    LINES.lock()[irq as usize] = Line {
        handler: Some(handler),
        name,
    };
    if USE_APIC.load(Ordering::Acquire) {
        super::apic::ioapic_unmask(irq);
    } else {
        super::pic::unmask(irq);
    }
}

/// Switches interrupt routing from the PIC to the I/O APIC.
pub fn switch_to_apic() {
    super::pic::disable();
    USE_APIC.store(true, Ordering::Release);
    // The local APIC timer replaces the PIT on line 0.
    LINES.lock()[0] = Line {
        handler: None,
        name: "",
    };
    let lines = *LINES.lock();
    for (irq, line) in lines.iter().enumerate() {
        if line.handler.is_some() {
            super::apic::ioapic_unmask(irq as u8);
        }
    }
}

pub fn using_apic() -> bool {
    USE_APIC.load(Ordering::Acquire)
}

/// Runs the handler for `irq` and acknowledges the interrupt.
pub fn dispatch(irq: u8) {
    COUNTS[irq as usize].fetch_add(1, Ordering::Relaxed);
    let handler = LINES.lock()[irq as usize].handler;
    if let Some(h) = handler {
        h();
    }
    end_of_interrupt(irq);
}

pub fn end_of_interrupt(irq: u8) {
    if USE_APIC.load(Ordering::Acquire) {
        super::apic::eoi();
    } else {
        super::pic::eoi(irq);
    }
}

/// Text for /proc/interrupts.
pub fn interrupts_text() -> String {
    let lines = *LINES.lock();
    let controller = if using_apic() { "IO-APIC" } else { "XT-PIC" };
    let mut s = String::from("           CPU0\n");
    for (i, line) in lines.iter().enumerate() {
        if line.handler.is_some() {
            s.push_str(&format!(
                "{:>3}: {:>10}   {:<8} {}\n",
                i,
                COUNTS[i].load(Ordering::Relaxed),
                controller,
                line.name
            ));
        }
    }
    s.push_str(&format!(
        "LOC: {:>10}   local timer\n",
        super::apic::timer_ticks()
    ));
    s
}
