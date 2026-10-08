//! Kernel/user ABI. Numbers and layouts follow Linux x86_64 so that the
//! same conventions (and, eventually, static musl binaries) work.

#![no_std]

pub mod errno;
pub mod syscall;
