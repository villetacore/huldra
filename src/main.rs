//! Huldra: a small Unix-like kernel for x86_64.

#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
mod console;

mod arch;
mod bootinfo;
mod drivers;
mod fs;
mod mm;
mod shell;
mod sync;
mod syscall;

use core::panic::PanicInfo;
use drivers::vga::Color;

pub const NAME: &str = "Huldra";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Called from boot.S in 64-bit mode with the loader's magic and info pointer.
#[no_mangle]
pub extern "C" fn kernel_main(magic: u32, info: u32) -> ! {
    drivers::serial::init();
    console::clear();
    console::set_color(Color::LightCyan, Color::Black);
    println!("{} {} (x86_64)", NAME, VERSION);
    console::reset_color();

    mm::heap::init();
    let boot = unsafe { bootinfo::parse(magic, info as usize) };
    log_ok(format_args!("booted via {}", boot.protocol));

    arch::init();
    log_ok(format_args!("GDT/TSS, IDT, PIC, PIT at {} Hz", arch::pit::HZ));

    mm::frame::init(&boot.memory);
    let (_, frames) = mm::frame::stats();
    log_ok(format_args!(
        "physical memory: {} MiB usable, heap {} MiB",
        frames * mm::PAGE_SIZE / (1024 * 1024),
        mm::heap::HEAP_SIZE / (1024 * 1024)
    ));
    bootinfo::store(boot);

    fs::init();
    log_ok(format_args!("tmpfs mounted on /"));

    arch::enable_interrupts();
    log_ok(format_args!("interrupts enabled, starting shell"));

    shell::run()
}

fn log_ok(args: core::fmt::Arguments) {
    print!("[");
    console::set_color(Color::LightGreen, Color::Black);
    print!(" ok ");
    console::reset_color();
    println!("] {}", args);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::disable_interrupts();
    unsafe { console::force_unlock() };
    console::set_color(Color::White, Color::Red);
    print!("KERNEL PANIC");
    console::set_color(Color::LightRed, Color::Black);
    println!(" {}", info);
    println!("System halted.");
    arch::halt_forever()
}
