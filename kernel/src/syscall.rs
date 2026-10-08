//! System call interface (`int 0x80`, Linux x86_64 numbering).
//!
//! Number in RAX, arguments in RDI, RSI, RDX; result (or -errno) in RAX.

use crate::arch::trap::TrapFrame;

pub const SYS_WRITE: u64 = 1;
pub const SYS_GETPID: u64 = 39;

const EBADF: i64 = 9;
const ENOSYS: i64 = 38;

pub fn dispatch(frame: &mut TrapFrame) {
    let ret = match frame.rax {
        SYS_WRITE => sys_write(frame.rdi, frame.rsi, frame.rdx),
        SYS_GETPID => 1,
        _ => -ENOSYS,
    };
    frame.rax = ret as u64;
}

fn sys_write(fd: u64, buf: u64, len: u64) -> i64 {
    if fd != 1 && fd != 2 {
        return -EBADF;
    }
    // TODO: validate the user pointer once user mode exists.
    let bytes = unsafe { core::slice::from_raw_parts(buf as *const u8, len as usize) };
    match core::str::from_utf8(bytes) {
        Ok(s) => print!("{}", s),
        Err(_) => bytes.iter().for_each(|&b| print!("{}", b as char)),
    }
    len as i64
}
