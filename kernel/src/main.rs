//! Huldra: a small Unix-like kernel for x86_64.

#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
mod console;
#[macro_use]
mod klog;
#[macro_use]
mod ktest;

mod arch;
mod bootinfo;
mod drivers;
mod fs;
mod mm;
mod proc;
mod shell;
mod sync;
mod syscall;
mod task;
mod time;
mod util;

use core::panic::PanicInfo;

pub const NAME: &str = "Huldra";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Called from boot.S in 64-bit mode with the loader's magic and the
/// physical address of its information structure.
#[no_mangle]
pub extern "C" fn kernel_main(magic: u32, info: u32) -> ! {
    drivers::serial::init();
    console::clear();
    println!("\x1b[96m{} {} (x86_64)\x1b[0m", NAME, VERSION);

    let boot = bootinfo::store(unsafe { bootinfo::parse(magic, info as u64) });
    kinfo!("booted via {}, cmdline: '{}'", boot.protocol, boot.cmdline.as_str());

    arch::init();
    mm::frame::init(boot);
    mm::vmm::init(boot);
    mm::frame::add_high_memory(boot);
    let (free, _) = mm::frame::stats();
    let k = mm::kernel_layout();
    kinfo!(
        "memory: {} MiB free, kernel image {} KiB at {:#x}",
        free * 4096 / (1024 * 1024),
        (k.end - k.start) / 1024,
        k.start
    );

    fs::init();
    task::sched::init();
    arch::enable_interrupts();

    let init = if boot.has_flag("ktest") {
        task::spawn_kernel("ktest", || ktest::run_all())
    } else {
        task::spawn_kernel("ksh", || shell::run())
    };
    task::detach(&init);
    task::sched::idle_loop()
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::disable_interrupts();
    unsafe { console::force_unlock() };
    println!("\x1b[97;41mKERNEL PANIC\x1b[0;91m {}\x1b[0m", info);
    if ktest::is_running() {
        ktest::exit_qemu(false);
    }
    println!("System halted.");
    arch::halt_forever()
}
