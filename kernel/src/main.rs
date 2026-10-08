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
mod net;
mod proc;
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
    if boot.has_flag("debug") {
        klog::set_console_level(klog::Level::Debug);
    } else if boot.has_flag("quiet") {
        klog::set_console_level(klog::Level::Warn);
    }
    kinfo!(
        "booted via {}, cmdline: '{}'",
        boot.protocol,
        boot.cmdline.as_str()
    );

    for r in boot.memory.iter() {
        kdebug!(
            "memory: {:#012x}..{:#012x} {}",
            r.base,
            r.base + r.len,
            r.kind_name()
        );
    }
    arch::init();
    time::init();
    mm::frame::init(boot);
    mm::vmm::init(boot);
    mm::frame::add_high_memory(boot);
    arch::init_apic();
    let (free, _) = mm::frame::stats();
    let k = mm::kernel_layout();
    kinfo!(
        "memory: {} MiB free, kernel image {} KiB at {:#x}",
        free * 4096 / (1024 * 1024),
        (k.end - k.start) / 1024,
        k.start
    );

    task::sched::init();
    drivers::init();
    fs::init(boot.option("root"));
    net::init(boot.option("ip"));
    arch::enable_interrupts();

    if boot.has_flag("ktest") {
        let t = task::spawn_kernel("ktest", || ktest::run_all());
        task::detach(&t);
    } else {
        let path = alloc::string::String::from(boot.option("init").unwrap_or("/sbin/init"));
        let init = task::spawn_kernel("init", move || proc::lifecycle::run_init(&path));
        assert_eq!(init.pid, 1);
    }
    net::start();
    // Write dirty disk blocks back every few seconds.
    task::detach(&task::spawn_kernel("flushd", || loop {
        task::sleep_ms(5000);
        fs::sync_all();
    }));
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
