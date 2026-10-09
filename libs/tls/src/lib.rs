//! TLS 1.3 for Huldra: a [`client::Client`] state machine and
//! [`x509`] certificate verification against a [`x509::RootStore`]. No
//! I/O: the program moves the bytes (see `huldra_user::tls`).
//!
//! `examples/fetch.rs` runs the same code on the build machine against
//! real servers, which is the quickest way to debug it.

#![no_std]

extern crate alloc;

pub mod client;
pub mod x509;

pub use client::{Client, Config};
pub use x509::{Certificate, RootStore};

#[cfg(test)]
mod tests;
