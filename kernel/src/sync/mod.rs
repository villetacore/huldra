//! Synchronization primitives.

use crate::arch;
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Spinlock that also disables interrupts while held, so it is safe to
/// share data between normal kernel code and interrupt handlers. Because
/// interrupts are off while it is held, the holder is never preempted.
pub struct SpinLock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinLock<T> {}
unsafe impl<T: Send> Send for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        SpinLock { locked: AtomicBool::new(false), data: UnsafeCell::new(value) }
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        let irq = arch::irq_save();
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        SpinLockGuard { lock: self, irq }
    }

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }

    /// Forcibly releases the lock. Only for panic/fatal paths.
    pub unsafe fn force_unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
    irq: bool,
}

impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
        arch::irq_restore(self.irq);
    }
}

/// A value initialized exactly once, then shared read-only.
pub struct Once<T> {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<T>>,
}

const INCOMPLETE: u8 = 0;
const RUNNING: u8 = 1;
const COMPLETE: u8 = 2;

unsafe impl<T: Send + Sync> Sync for Once<T> {}
unsafe impl<T: Send> Send for Once<T> {}

impl<T> Once<T> {
    pub const fn new() -> Self {
        Once { state: AtomicU8::new(INCOMPLETE), value: UnsafeCell::new(MaybeUninit::uninit()) }
    }

    pub fn call_once(&self, f: impl FnOnce() -> T) -> &T {
        if self.state.compare_exchange(INCOMPLETE, RUNNING, Ordering::Acquire, Ordering::Acquire).is_ok() {
            unsafe { (*self.value.get()).write(f()) };
            self.state.store(COMPLETE, Ordering::Release);
        }
        while self.state.load(Ordering::Acquire) != COMPLETE {
            core::hint::spin_loop();
        }
        unsafe { (*self.value.get()).assume_init_ref() }
    }

    pub fn get(&self) -> Option<&T> {
        (self.state.load(Ordering::Acquire) == COMPLETE).then(|| unsafe { (*self.value.get()).assume_init_ref() })
    }

    /// Returns the value; panics if it was not initialized yet.
    pub fn expect(&self, what: &str) -> &T {
        match self.get() {
            Some(v) => v,
            None => panic!("{} not initialized", what),
        }
    }
}
