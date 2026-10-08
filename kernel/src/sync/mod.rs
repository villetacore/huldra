//! Synchronization primitives.

mod mutex;

pub use mutex::Mutex;

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
        SpinLock {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(value),
        }
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

/// Guard for a lock owned through an `Arc` (keeps the lock alive).
pub struct ArcSpinGuard<T> {
    lock: alloc::sync::Arc<SpinLock<T>>,
    irq: bool,
}

impl<T> SpinLock<T> {
    pub fn lock_arc(this: &alloc::sync::Arc<SpinLock<T>>) -> ArcSpinGuard<T> {
        let irq = arch::irq_save();
        while this.locked.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        ArcSpinGuard { lock: this.clone(), irq }
    }
}

impl<T> Deref for ArcSpinGuard<T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for ArcSpinGuard<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for ArcSpinGuard<T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
        arch::irq_restore(self.irq);
    }
}

/// State that several tasks may share (address space and file table of a
/// thread group). `lock` locks the current shared object; `share`/`set`
/// change which object this task refers to.
pub struct Shared<T> {
    slot: SpinLock<alloc::sync::Arc<SpinLock<T>>>,
}

impl<T> Shared<T> {
    pub fn new(value: T) -> Self {
        Shared { slot: SpinLock::new(alloc::sync::Arc::new(SpinLock::new(value))) }
    }

    /// The shared object (for handing it to another task).
    pub fn share(&self) -> alloc::sync::Arc<SpinLock<T>> {
        self.slot.lock().clone()
    }

    /// Points this task at `object`; returns the previous one.
    pub fn set(&self, object: alloc::sync::Arc<SpinLock<T>>) -> alloc::sync::Arc<SpinLock<T>> {
        core::mem::replace(&mut *self.slot.lock(), object)
    }

    /// Replaces the object with a new, unshared one holding `value`.
    pub fn reset(&self, value: T) -> alloc::sync::Arc<SpinLock<T>> {
        self.set(alloc::sync::Arc::new(SpinLock::new(value)))
    }

    pub fn lock(&self) -> ArcSpinGuard<T> {
        let arc = self.share();
        SpinLock::lock_arc(&arc)
    }

    /// Identity of the shared object (e.g. for futex keys).
    pub fn id(&self) -> usize {
        alloc::sync::Arc::as_ptr(&*self.slot.lock()) as usize
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
        Once {
            state: AtomicU8::new(INCOMPLETE),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    pub fn call_once(&self, f: impl FnOnce() -> T) -> &T {
        if self
            .state
            .compare_exchange(INCOMPLETE, RUNNING, Ordering::Acquire, Ordering::Acquire)
            .is_ok()
        {
            unsafe { (*self.value.get()).write(f()) };
            self.state.store(COMPLETE, Ordering::Release);
        }
        while self.state.load(Ordering::Acquire) != COMPLETE {
            core::hint::spin_loop();
        }
        unsafe { (*self.value.get()).assume_init_ref() }
    }

    pub fn get(&self) -> Option<&T> {
        (self.state.load(Ordering::Acquire) == COMPLETE)
            .then(|| unsafe { (*self.value.get()).assume_init_ref() })
    }

    /// Returns the value; panics if it was not initialized yet.
    pub fn expect(&self, what: &str) -> &T {
        match self.get() {
            Some(v) => v,
            None => panic!("{} not initialized", what),
        }
    }
}
