//! In-kernel self tests, run when the command line contains `ktest`.
//! The result is reported to QEMU through the isa-debug-exit device.

use crate::arch::port::outl;
use core::sync::atomic::{AtomicBool, Ordering};

static RUNNING: AtomicBool = AtomicBool::new(false);

pub fn is_running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

pub struct Test {
    pub name: &'static str,
    pub run: fn(),
}

/// Builds a test list from functions: `ktests![a, b]`.
macro_rules! ktests {
    ($($f:path),* $(,)?) => {
        &[$($crate::ktest::Test { name: stringify!($f), run: $f }),*]
    };
}

pub fn exit_qemu(success: bool) -> ! {
    unsafe { outl(0xF4, if success { 0x10 } else { 0x11 }) };
    crate::arch::halt_forever()
}

pub fn run_all() -> ! {
    RUNNING.store(true, Ordering::Relaxed);
    let suites: &[&[Test]] = &[crate::mm::TESTS, crate::fs::TESTS];
    let total: usize = suites.iter().map(|s| s.len()).sum();
    println!("running {} kernel tests", total);
    for test in suites.iter().flat_map(|s| s.iter()) {
        print!("test {} ... ", test.name);
        (test.run)();
        println!("ok");
    }
    println!("test result: ok. {} passed", total);
    exit_qemu(true)
}
