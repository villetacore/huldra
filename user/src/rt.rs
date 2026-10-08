//! Process startup and the panic handler.

use core::arch::global_asm;

// The kernel enters at `_start` with RSP pointing at argc, followed by argv,
// NULL, envp, NULL and the auxiliary vector (System V ABI).
global_asm!(
    ".global _start",
    "_start:",
    "    xor rbp, rbp",
    "    mov rdi, rsp",
    "    and rsp, -16",
    "    call __huldra_start",
    "    ud2",
);

extern "C" {
    fn __huldra_main() -> i32;
}

#[no_mangle]
unsafe extern "C" fn __huldra_start(sp: *const u64) -> ! {
    let argc = *sp as usize;
    let argv = sp.add(1) as *const *const u8;
    let envp = argv.add(argc + 1);
    crate::env::init(argc, argv, envp);
    let code = __huldra_main();
    crate::io::flush_stdout();
    crate::process::exit(code)
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    crate::io::flush_stdout();
    crate::eprintln!("{}: {}", crate::env::program_name(), info);
    crate::process::exit(101)
}
