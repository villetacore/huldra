//! Common trap entry: CPU exceptions, hardware IRQs and system calls.

use super::{halt_forever, pic, pit, read_cr2};
use crate::console;
use crate::drivers::{keyboard, vga::Color};

/// Register state saved by `isr_common` (see trap.S), lowest address first.
#[repr(C)]
#[derive(Debug)]
pub struct TrapFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error: u64,
    // Pushed by the CPU.
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

const EXCEPTIONS: [&str; 32] = [
    "Divide Error",
    "Debug",
    "Non-Maskable Interrupt",
    "Breakpoint",
    "Overflow",
    "Bound Range Exceeded",
    "Invalid Opcode",
    "Device Not Available",
    "Double Fault",
    "Coprocessor Segment Overrun",
    "Invalid TSS",
    "Segment Not Present",
    "Stack-Segment Fault",
    "General Protection Fault",
    "Page Fault",
    "Reserved",
    "x87 Floating-Point Exception",
    "Alignment Check",
    "Machine Check",
    "SIMD Floating-Point Exception",
    "Virtualization Exception",
    "Control Protection Exception",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Hypervisor Injection Exception",
    "VMM Communication Exception",
    "Security Exception",
    "Reserved",
];

#[no_mangle]
extern "C" fn trap_dispatch(frame: &mut TrapFrame) {
    match frame.vector {
        3 => println!("[trap] breakpoint at {:#x}", frame.rip),
        0..=31 => fatal_exception(frame),
        32..=47 => {
            let irq = (frame.vector - pic::IRQ_BASE as u64) as u8;
            match irq {
                0 => pit::tick(),
                1 => keyboard::handle_irq(),
                _ => {}
            }
            pic::eoi(irq);
        }
        0x80 => crate::syscall::dispatch(frame),
        v => println!("[trap] unexpected vector {}", v),
    }
}

fn fatal_exception(f: &TrapFrame) -> ! {
    // The faulting code may have held the console lock.
    unsafe { console::force_unlock() };
    console::set_color(Color::White, Color::Red);
    println!(
        "\n*** CPU EXCEPTION {}: {} ***",
        f.vector, EXCEPTIONS[f.vector as usize]
    );
    console::set_color(Color::LightRed, Color::Black);
    println!("error code: {:#x}", f.error);
    if f.vector == 14 {
        let e = f.error;
        println!(
            "fault address: {:#018x} ({} {} in {} mode)",
            read_cr2(),
            if e & 1 != 0 { "protection violation" } else { "page not present" },
            if e & 2 != 0 { "on write" } else { "on read" },
            if e & 4 != 0 { "user" } else { "kernel" },
        );
    }
    println!("rip={:#018x} cs={:#x} rflags={:#x}", f.rip, f.cs, f.rflags);
    println!("rsp={:#018x} ss={:#x}", f.rsp, f.ss);
    println!("rax={:#018x} rbx={:#018x} rcx={:#018x}", f.rax, f.rbx, f.rcx);
    println!("rdx={:#018x} rsi={:#018x} rdi={:#018x}", f.rdx, f.rsi, f.rdi);
    println!("rbp={:#018x} r8 ={:#018x} r9 ={:#018x}", f.rbp, f.r8, f.r9);
    println!("r10={:#018x} r11={:#018x} r12={:#018x}", f.r10, f.r11, f.r12);
    println!("r13={:#018x} r14={:#018x} r15={:#018x}", f.r13, f.r14, f.r15);
    println!("System halted.");
    halt_forever()
}
