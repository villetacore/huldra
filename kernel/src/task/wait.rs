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

    /// Removes the current task from the queue (after a timeout or signal,
    /// so a later `wake_one` is not spent on a task that left).
    fn forget_current(&self) {
        if sched::is_running() {
            let pid = sched::current_pid();
            self.waiters.lock().retain(|t| t.pid != pid);
        }
    }

    fn wait_inner<T>(
        &self,
        mut condition: impl FnMut() -> Option<T>,
        interruptible: bool,
        deadline: Option<u64>,
    ) -> Result<Option<T>, Errno> {
        loop {
            let irq = arch::irq_save();
            if let Some(v) = condition() {
                self.forget_current();
                arch::irq_restore(irq);
                return Ok(Some(v));
            }
            if deadline.is_some_and(|d| crate::time::ticks() >= d) {
                self.forget_current();
                arch::irq_restore(irq);
                return Ok(None);
            }
            if interruptible && crate::proc::signal::current_has_pending() {
                self.forget_current();
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
            {
                let mut q = self.waiters.lock();
                if !q.iter().any(|t| Arc::ptr_eq(t, &me)) {
                    q.push_back(me.clone());
                }
            }
            if deadline.is_some() {
                // The timer tick wakes sleepers, which re-checks the deadline.
                sched::SLEEPERS.enqueue(me.clone());
            }
            sched::block_current();
            drop(me);
            sched::schedule();
            arch::irq_restore(irq);
        }
    }

    pub(crate) fn enqueue(&self, task: Arc<Task>) {
        let mut q = self.waiters.lock();
        if !q.iter().any(|t| Arc::ptr_eq(t, &task)) {
            q.push_back(task);
        }
    }

    /// Like [`wait_until`](Self::wait_until) but gives up at timer tick
    /// `deadline`, returning `Ok(None)`.
    pub fn wait_until_deadline<T>(&self, condition: impl FnMut() -> Option<T>, deadline: u64) -> Result<Option<T>, Errno> {
        self.wait_inner(condition, true, Some(deadline))
    }

    /// Blocks until `condition` returns `Some`, or a signal arrives (EINTR).
    /// The condition is evaluated with interrupts disabled, so a wakeup
    /// between the check and going to sleep cannot be lost.
    pub fn wait_until<T>(&self, condition: impl FnMut() -> Option<T>) -> Result<T, Errno> {
        self.wait_inner(condition, true, None).map(|v| v.expect("no deadline"))
    }

    /// Like [`wait_until`](Self::wait_until) but ignores signals.
    pub fn wait_uninterruptible<T>(&self, condition: impl FnMut() -> Option<T>) -> T {
        match self.wait_inner(condition, false, None) {
            Ok(Some(v)) => v,
            _ => unreachable!(),
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
