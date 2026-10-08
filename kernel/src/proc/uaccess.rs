//! Copying data between the kernel and user space.

use crate::fs::KResult;
use huldra_abi::errno::Errno;

/// Reads a `T` from user address `addr`.
pub fn read_user<T: Copy>(addr: u64) -> KResult<T> {
    if addr == 0 {
        return Err(Errno::EFAULT);
    }
    Ok(unsafe { (addr as *const T).read_unaligned() })
}

/// Writes `value` to user address `addr`.
pub fn write_user<T: Copy>(addr: u64, value: &T) -> KResult<()> {
    if addr == 0 {
        return Err(Errno::EFAULT);
    }
    unsafe { (addr as *mut T).write_unaligned(*value) };
    Ok(())
}
