//! Copying data between the kernel and user space.
//!
//! Every access is first checked against the current process's memory
//! areas, so a bad pointer yields EFAULT instead of a kernel fault. Pages
//! that are valid but not yet present are faulted in during the copy.

use crate::fs::KResult;
use crate::task;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::size_of;
use huldra_abi::errno::Errno;

const PAGE: u64 = crate::mm::PAGE_SIZE;

pub fn check(addr: u64, len: usize, write: bool) -> KResult<()> {
    let me = task::current();
    let mm = me.mm.lock();
    match &*mm {
        Some(mm) => mm.check_access(addr, len as u64, write),
        None => Err(Errno::EFAULT),
    }
}

pub fn copy_from_user(dst: &mut [u8], src: u64) -> KResult<()> {
    check(src, dst.len(), false)?;
    unsafe { core::ptr::copy_nonoverlapping(src as *const u8, dst.as_mut_ptr(), dst.len()) };
    Ok(())
}

pub fn copy_to_user(dst: u64, src: &[u8]) -> KResult<()> {
    check(dst, src.len(), true)?;
    unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dst as *mut u8, src.len()) };
    Ok(())
}

pub fn read_user<T: Copy>(addr: u64) -> KResult<T> {
    check(addr, size_of::<T>(), false)?;
    Ok(unsafe { (addr as *const T).read_unaligned() })
}

pub fn write_user<T: Copy>(addr: u64, value: &T) -> KResult<()> {
    check(addr, size_of::<T>(), true)?;
    unsafe { (addr as *mut T).write_unaligned(*value) };
    Ok(())
}

/// Reads a NUL-terminated string of at most `max` bytes.
pub fn read_cstr(addr: u64, max: usize) -> KResult<String> {
    let mut bytes = Vec::new();
    let mut cur = addr;
    loop {
        let chunk = (PAGE - cur % PAGE) as usize;
        check(cur, chunk, false)?;
        let slice = unsafe { core::slice::from_raw_parts(cur as *const u8, chunk) };
        match slice.iter().position(|&b| b == 0) {
            Some(n) => {
                bytes.extend_from_slice(&slice[..n]);
                break;
            }
            None => bytes.extend_from_slice(slice),
        }
        if bytes.len() > max {
            return Err(Errno::ENAMETOOLONG);
        }
        cur += chunk as u64;
    }
    if bytes.len() > max {
        return Err(Errno::ENAMETOOLONG);
    }
    String::from_utf8(bytes).map_err(|_| Errno::EINVAL)
}

/// Reads a path argument.
pub fn read_path(addr: u64) -> KResult<String> {
    read_cstr(addr, crate::fs::vfs::PATH_MAX)
}

/// Reads a NULL-terminated array of string pointers (argv, envp).
pub fn read_string_array(addr: u64) -> KResult<Vec<String>> {
    let mut out = Vec::new();
    if addr == 0 {
        return Ok(out);
    }
    let mut total = 0;
    for i in 0.. {
        let p: u64 = read_user(addr + i * 8)?;
        if p == 0 {
            break;
        }
        let s = read_cstr(p, 128 * 1024)?;
        total += s.len() + 1;
        if total > 256 * 1024 || i > 4096 {
            return Err(Errno::E2BIG);
        }
        out.push(s);
    }
    Ok(out)
}
