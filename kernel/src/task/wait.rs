//! Wait queues: block a task until a condition holds.

use super::sched;
use super::Task;
use crate::arch;
use crate::sync::SpinLock;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use huldra_abi::errno::Errno;

pub struct WaitQueue {
    waiters: SpinLock<VecDeque<Arc<Task>>>,
}

impl WaitQueue {
    pub const fn new() -> Self {
        WaitQueue {
            waiters: SpinLock::new(VecDeque::new()),
        }
    }

    fn wait_inner<T>(
        &self,
        mut condition: impl FnMut() -> Option<T>,
        interruptible: bool,
    ) -> Result<T, Errno> {
        loop {
            let irq = arch::irq_save();
            if let Some(v) = condition() {
                arch::irq_restore(irq);
                return Ok(v);
            }
            if interruptible && crate::proc::signal::current_has_pending() {
                arch::irq_restore(irq);
                return Err(Errno::EINTR);
            }
            if !sched::is_running() {
                // Early boot: nothing to switch to, just wait for an interrupt.
                unsafe { core::arch::asm!("sti; hlt", options(nomem, nostack)) };
                arch::irq_restore(irq);
                continue;
            }
            let me = sched::current();
            self.waiters.lock().push_back(me.clone());
            sched::block_current();
            drop(me);
            sched::schedule();
            arch::irq_restore(irq);
        }
    }

    /// Blocks until `condition` returns `Some`, or a signal arrives (EINTR).
    /// The condition is evaluated with interrupts disabled, so a wakeup
    /// between the check and going to sleep cannot be lost.
    pub fn wait_until<T>(&self, condition: impl FnMut() -> Option<T>) -> Result<T, Errno> {
        self.wait_inner(condition, true)
    }

    /// Like [`wait_until`](Self::wait_until) but ignores signals.
    pub fn wait_uninterruptible<T>(&self, condition: impl FnMut() -> Option<T>) -> T {
        match self.wait_inner(condition, false) {
            Ok(v) => v,
            Err(_) => unreachable!(),
        }
    }

    pub fn wake_all(&self) {
        let waiters = core::mem::take(&mut *self.waiters.lock());
        for t in waiters {
            sched::make_runnable(&t);
        }
    }

    pub fn wake_one(&self) {
        let t = self.waiters.lock().pop_front();
        if let Some(t) = t {
            sched::make_runnable(&t);
        }
    }
}
