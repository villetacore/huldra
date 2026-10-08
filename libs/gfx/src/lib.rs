//! Graphics for Huldra, shared by the display server, window managers
//! and applications, and tested on the build machine: pixel buffers and
//! drawing ([`canvas`]), bitmap fonts ([`font`]), the display
//! [`proto`]col, [`keymap`]s, a terminal emulator core ([`term`]) and
//! i3-style tiling ([`tile`]).

#![no_std]

extern crate alloc;

pub mod canvas;
pub mod font;
pub mod keymap;
pub mod proto;
pub mod term;
pub mod tile;

pub use canvas::{rgb, Canvas, Rect};
pub use font::Font;
