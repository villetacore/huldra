//! Memory mapping ABI.

pub const PROT_NONE: u32 = 0;
pub const PROT_READ: u32 = 1;
pub const PROT_WRITE: u32 = 2;
pub const PROT_EXEC: u32 = 4;

pub const MAP_SHARED: u32 = 0x01;
pub const MAP_PRIVATE: u32 = 0x02;
pub const MAP_FIXED: u32 = 0x10;
pub const MAP_ANONYMOUS: u32 = 0x20;

pub const MAP_FAILED: u64 = u64::MAX;

pub const ARCH_SET_FS: u32 = 0x1002;
pub const ARCH_GET_FS: u32 = 0x1003;
