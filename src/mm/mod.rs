//! Memory management.

pub mod frame;
pub mod heap;

pub const PAGE_SIZE: u64 = 4096;

pub const fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}

pub const fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}
