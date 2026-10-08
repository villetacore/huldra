//! Process management ABI: wait status encoding, auxiliary vector.

pub const WNOHANG: u32 = 1;
pub const WUNTRACED: u32 = 2;

/// Status for a child that called `exit(code)`.
pub const fn exit_status(code: i32) -> i32 {
    (code & 0xFF) << 8
}

/// Status for a child killed by `signal`.
pub const fn signal_status(signal: u32) -> i32 {
    (signal & 0x7F) as i32
}

pub const fn wifexited(status: i32) -> bool {
    status & 0x7F == 0
}

pub const fn wexitstatus(status: i32) -> i32 {
    (status >> 8) & 0xFF
}

pub const fn wifsignaled(status: i32) -> bool {
    (status & 0x7F) != 0 && (status & 0x7F) != 0x7F
}

pub const fn wtermsig(status: i32) -> u32 {
    (status & 0x7F) as u32
}

pub const AT_NULL: u64 = 0;
pub const AT_PHDR: u64 = 3;
pub const AT_PHENT: u64 = 4;
pub const AT_PHNUM: u64 = 5;
pub const AT_PAGESZ: u64 = 6;
pub const AT_BASE: u64 = 7;
pub const AT_ENTRY: u64 = 9;
pub const AT_UID: u64 = 11;
pub const AT_EUID: u64 = 12;
pub const AT_GID: u64 = 13;
pub const AT_EGID: u64 = 14;
pub const AT_RANDOM: u64 = 25;
pub const AT_EXECFN: u64 = 31;
