//! futex(2): sleeping on, and waking, 32-bit words in user memory.
//! Keys are (address space, virtual address), so futexes work between the
//! threads of a process (shared, cross-process futexes are not supported).

use crate::fs::KResult;
use crate::sync::SpinLock;
use crate::task::wait::WaitQueue;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use huldra_abi::errno::Errno;

type Key = (usize, u64);

static TABLE: SpinLock<BTreeMap<Key, Vec<Arc<AtomicBool>>>> = SpinLock::new(BTreeMap::new());
static WAITERS: WaitQueue = WaitQueue::new();

/// Sleeps while `*addr == expected`, until woken, the deadline (timer
/// ticks) passes (ETIMEDOUT) or a signal arrives (EINTR).
pub fn wait(mm: usize, addr: u64, expected: u32, deadline: Option<u64>) -> KResult<()> {
    if addr % 4 != 0 {
        return Err(Errno::EINVAL);
    }
    super::uaccess::check(addr, 4, false)?;
    let woken = Arc::new(AtomicBool::new(false));
    {
        let mut table = TABLE.lock();
        // Checked under the table lock: a waker cannot slip in between.
        let current = unsafe { core::ptr::read_volatile(addr as *const u32) };
        if current != expected {
            return Err(Errno::EAGAIN);
        }
        table.entry((mm, addr)).or_default().push(woken.clone());
    }
    let condition = || woken.load(Ordering::Acquire).then_some(());
    let result = match deadline {
        Some(d) => WAITERS.wait_until_deadline(condition, d).and_then(|r| r.ok_or(Errno::ETIMEDOUT)),
        None => WAITERS.wait_until(condition),
    };
    if result.is_err() {
        let mut table = TABLE.lock();
        if let Some(list) = table.get_mut(&(mm, addr)) {
            list.retain(|w| !Arc::ptr_eq(w, &woken));
            if list.is_empty() {
                table.remove(&(mm, addr));
            }
        }
        if woken.load(Ordering::Acquire) {
            return Ok(());
        }
    }
    result
}

/// Wakes up to `count` waiters on `addr`; returns how many were woken.
pub fn wake(mm: usize, addr: u64, count: usize) -> usize {
    let mut n = 0;
    {
        let mut table = TABLE.lock();
        if let Some(list) = table.get_mut(&(mm, addr)) {
            while n < count && !list.is_empty() {
                list.remove(0).store(true, Ordering::Release);
                n += 1;
            }
            if list.is_empty() {
                table.remove(&(mm, addr));
            }
        }
    }
    if n > 0 {
        WAITERS.wake_all();
    }
    n
}

/// Moves waiters from one word to another (FUTEX_REQUEUE); wakes `wake_count` first.
pub fn requeue(mm: usize, from: u64, to: u64, wake_count: usize, requeue_count: usize) -> usize {
    let woken = wake(mm, from, wake_count);
    let mut table = TABLE.lock();
    if let Some(mut list) = table.remove(&(mm, from)) {
        let moved: Vec<_> = list.drain(..requeue_count.min(list.len())).collect();
        if !list.is_empty() {
            table.insert((mm, from), list);
        }
        table.entry((mm, to)).or_default().extend(moved);
    }
    woken
}
