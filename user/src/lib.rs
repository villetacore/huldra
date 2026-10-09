//! Huldra user-space runtime: the small "libc" every program links.
//!
//! Provides the process entry point, system call wrappers, buffered I/O,
//! files and directories, processes, signals and a heap allocator. Programs
//! live in `src/bin/` and declare their entry with [`main!`].

#![no_std]

extern crate alloc;

pub mod env;
pub mod fs;
pub mod gui;
pub mod http;
pub mod io;
pub mod net;
pub mod process;
pub mod rand;
pub mod signal;
pub mod sys;
pub mod term;
pub mod time;

mod heap;
mod rt;

pub use alloc::{format, string::String, string::ToString, vec, vec::Vec};
pub use huldra_abi as abi;
pub use huldra_abi::errno::Errno;

pub type Result<T> = core::result::Result<T, Errno>;

/// Declares the program entry point: `main!(run);` with `fn run() -> i32`.
#[macro_export]
macro_rules! main {
    ($f:path) => {
        #[no_mangle]
        extern "C" fn __huldra_main() -> i32 {
            $f()
        }
    };
}

/// Prints an error message prefixed with the program name and exits.
pub fn die(msg: core::fmt::Arguments) -> ! {
    eprintln!("{}: {}", env::program_name(), msg);
    process::exit(1)
}
