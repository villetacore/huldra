//! TCP/IP for Huldra, written without I/O so it runs (and is tested) on
//! the build machine as well as in the kernel: [`wire`] formats,
//! [`tcp`] connections, the [`stack`] (ARP, routing, sockets, DHCP
//! client) and [`dns`] messages for the resolver.

#![no_std]

extern crate alloc;

pub mod dhcp;
pub mod dns;
pub mod stack;
pub mod tcp;
pub mod wire;

pub use stack::{IfConfig, NetError, Proto, Readiness, SocketId, Stack};
pub use wire::{Ip, Mac};

#[cfg(test)]
mod tests;
