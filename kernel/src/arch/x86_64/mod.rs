//! x86_64 architecture support.

pub mod apic;
pub mod context;
pub mod cpu;
pub mod gdt;
pub mod idt;
pub mod irq;
pub mod paging;
pub mod percpu;
pub mod pic;
pub mod pit;
pub mod port;
pub mod trap;

use core::arch::{asm, global_asm};

pub use trap::TrapFrame;

global_asm!(include_str!("boot.S"), options(raw));
global_asm!(include_str!("entry.S"), options(raw));

/// Brings up descriptor tables, interrupt controller and timer.
/// Interrupts stay disabled until [`enable_interrupts`] is called.
pub fn init() {
    gdt::init();
    percpu::init(); // after gdt::init, which reloads GS
    idt::init();
    pic::init();
    pit::init();
    irq::register(0, "timer", timer_interrupt);
}

fn timer_interrupt() {
    crate::time::tick();
    crate::task::sched::timer_tick();
}

pub fn enable_interrupts() {
    unsafe { asm!("sti", options(nomem, nostack)) }
}

pub fn disable_interrupts() {
    unsafe { asm!("cli", options(nomem, nostack)) }
}

pub fn interrupts_enabled() -> bool {
    let flags: u64;
    unsafe { asm!("pushfq", "pop {}", out(reg) flags, options(nomem, preserves_flags)) }
    flags & (1 << 9) != 0
}

/// Disables interrupts and returns whether they were enabled before.
pub fn irq_save() -> bool {
    let flags: u64;
    unsafe { asm!("pushfq", "pop {}", "cli", out(reg) flags) }
    flags & (1 << 9) != 0
}

pub fn irq_restore(enabled: bool) {
    if enabled {
        enable_interrupts();
    }
}

/// Sleeps until the next interrupt.
pub fn wait_for_interrupt() {
    unsafe { asm!("hlt", options(nomem, nostack)) }
}

pub fn halt_forever() -> ! {
    loop {
        unsafe { asm!("cli; hlt", options(nomem, nostack)) }
    }
}

/// Powers the machine off (QEMU, Bochs, VirtualBox), halts otherwise.
pub fn poweroff() -> ! {
    disable_interrupts();
    unsafe {
        port::outw(0x604, 0x2000);
        port::outw(0xB004, 0x2000);
        port::outw(0x4004, 0x3400);
    }
    halt_forever()
}

/// Resets the CPU via the keyboard controller, triple-faults as fallback.
pub fn reboot() -> ! {
    disable_interrupts();
    unsafe {
        for _ in 0..0x10000 {
            if port::inb(0x64) & 0x02 == 0 {
                break;
            }
        }
        port::outb(0x64, 0xFE);

        let null = gdt::DescriptorTablePointer { limit: 0, base: 0 };
        asm!("lidt [{}]", "int3", in(reg) &null);
    }
    halt_forever()
}
