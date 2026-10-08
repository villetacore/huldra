//! Sleeping mutex: waiters block instead of spinning, so it may be held
//! across operations that sleep (disk I/O, waiting for input).
//! Must not be used from interrupt handlers.

use crate::task::wait::WaitQueue;
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

pub struct Mutex<T> {
    locked: AtomicBool,
    waiters: WaitQueue,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Mutex<T> {}
unsafe impl<T: Send> Send for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Mutex { locked: AtomicBool::new(false), waiters: WaitQueue::new(), data: UnsafeCell::new(value) }
    }

    fn try_acquire(&self) -> bool {
        self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok()
    }

    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.waiters.wait_uninterruptible(|| self.try_acquire().then_some(()));
        MutexGuard { mutex: self }
    }

    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        self.try_acquire().then_some(MutexGuard { mutex: self })
    }
}

pub struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.locked.store(false, Ordering::Release);
        self.mutex.waiters.wake_one();
    }
}
