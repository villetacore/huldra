//! Archive formats and hashing shared by user space and the build tool:
//! POSIX ustar archives (`tar`, packages) and SHA-256.

#![no_std]

extern crate alloc;

pub mod sha256;
pub mod tar;
