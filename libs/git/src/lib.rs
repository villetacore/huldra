//! Git for Huldra, without I/O: [`object`]s (blobs, trees, commits),
//! [`pack`] files and their indexes, the [`index`] (staging area), the
//! smart HTTP [`protocol`] and line [`diff`]s. Everything is compatible
//! with real git: the tests read and write data made by `git` itself, and
//! `cargo xtask test` clones from and pushes to a real git server.
//!
//! The `git` program (`user/src/bin/git`) puts it together with the file
//! system and the network.

#![no_std]

extern crate alloc;

pub mod diff;
pub mod index;
pub mod object;
pub mod pack;
pub mod protocol;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub alloc::string::String);

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

pub use object::{Id, Kind};

#[cfg(test)]
mod tests;
