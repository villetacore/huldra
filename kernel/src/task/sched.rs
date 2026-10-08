//! Round-robin preemptive scheduler.
//!
//! The timer interrupt shortens the running task's time slice; when it runs
//! out, the task is preempted on the way out of the interrupt. Kernel code
//! is preemptible whenever interrupts are enabled: spinlocks disable
//! interrupts, so a lock holder is never switched away from.

use super::wait::WaitQueue;
use super::{new_task, State, Task};
use crate::arch::{self, context::Context, percpu};
use crate::sync::SpinLock;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Ticks a task may run before it is preempted.
const TIME_SLICE: u32 = 5;

struct Scheduler {
    run_queue: VecDeque<Arc<Task>>,
    current: Option<Arc<Task>>,
    idle: Option<Arc<Task>>,
    /// Exited detached tasks whose memory the idle task frees.
    graveyard: Vec<Arc<Task>>,
}

static SCHED: SpinLock<Scheduler> = SpinLock::new(Scheduler {
    run_queue: VecDeque::new(),
    current: None,
    idle: None,
    graveyard: Vec::new(),
});
static NEED_RESCHED: AtomicBool = AtomicBool::new(false);
static SLICE_LEFT: AtomicU32 = AtomicU32::new(TIME_SLICE);
static STARTED: AtomicBool = AtomicBool::new(false);

/// Tasks sleeping on a timeout; woken on every tick.
pub static SLEEPERS: WaitQueue = WaitQueue::new();

/// Turns the boot context into the idle task (pid 0).
pub fn init() {
    let idle = new_task(0, "idle", None, Context::default());
    idle.set_state(State::Running);
    let mut s = SCHED.lock();
    s.current = Some(idle.clone());
    s.idle = Some(idle);
    STARTED.store(true, Ordering::Release);
}

pub fn is_running() -> bool {
    STARTED.load(Ordering::Acquire)
}

pub fn current() -> Arc<Task> {
    SCHED
        .lock()
        .current
        .clone()
        .expect("scheduler not initialized")
}

pub fn current_pid() -> super::Pid {
    SCHED.lock().current.as_ref().map_or(0, |t| t.pid)
}

/// Makes a new or blocked task runnable.
pub fn make_runnable(task: &Arc<Task>) {
    let mut s = SCHED.lock();
    match task.state() {
        State::Blocked | State::Ready if !s.run_queue.iter().any(|t| Arc::ptr_eq(t, task)) => {
            let is_current = s.current.as_ref().is_some_and(|c| Arc::ptr_eq(c, task));
            if !is_current {
                task.set_state(State::Ready);
                s.run_queue.push_back(task.clone());
            } else {
                // Woken before it managed to switch away: just keep running.
                task.set_state(State::Running);
            }
        }
        _ => {}
    }
}

/// Marks the current task blocked; the caller must then call [`schedule`].
pub fn block_current() {
    let s = SCHED.lock();
    if let Some(c) = &s.current {
        c.set_state(State::Blocked);
    }
}

pub(crate) fn set_zombie(task: &Arc<Task>) {
    let _s = SCHED.lock();
    task.set_state(State::Zombie);
}

pub(crate) fn defer_drop(task: Arc<Task>) {
    SCHED.lock().graveyard.push(task);
}

/// Picks the next task and switches to it. Returns when the calling task
/// is scheduled again.
pub fn schedule() {
    let irq = arch::irq_save();
    let switch = {
        let mut s = SCHED.lock();
        NEED_RESCHED.store(false, Ordering::Relaxed);
        SLICE_LEFT.store(TIME_SLICE, Ordering::Relaxed);

        let prev = s.current.clone().expect("scheduler not initialized");
        let idle = s.idle.clone().unwrap();
        if prev.state() == State::Running && !Arc::ptr_eq(&prev, &idle) {
            prev.set_state(State::Ready);
            s.run_queue.push_back(prev.clone());
        }

        let next = loop {
            match s.run_queue.pop_front() {
                Some(t) if t.state() == State::Ready => break t,
                Some(_) => continue,
                None => break idle.clone(),
            }
        };

        if Arc::ptr_eq(&next, &prev) {
            next.set_state(State::Running);
            None
        } else {
            next.set_state(State::Running);
            if let Some(top) = next.kernel_stack_top() {
                percpu::set_kernel_stack(top);
            }
            crate::proc::switch_address_space(&prev, &next);
            unsafe {
                prev.fpu().save();
                next.fpu().restore();
            }
            let save = unsafe { (*prev.context_ptr()).as_mut_ptr() };
            let to = unsafe { (*next.context_ptr()).rsp() };
            s.current = Some(next);
            // `prev` stays alive: it is referenced by the task table, a wait
            // queue, the run queue or the graveyard.
            Some((save, to))
        }
    };
    if let Some((save, to)) = switch {
        unsafe { arch::context::switch(save, to) };
    }
    arch::irq_restore(irq);
}

pub fn yield_now() {
    schedule();
}

/// Called from the timer interrupt.
pub fn timer_tick() {
    if let Some(c) = SCHED.lock().current.as_ref() {
        c.cpu_ticks.fetch_add(1, Ordering::Relaxed);
    }
    SLEEPERS.wake_all();
    crate::proc::alarm::tick();
    if SLICE_LEFT.load(Ordering::Relaxed) <= 1 {
        NEED_RESCHED.store(true, Ordering::Relaxed);
    } else {
        SLICE_LEFT.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Called on the way out of every interrupt.
pub fn preempt_if_needed() {
    if is_running() && NEED_RESCHED.load(Ordering::Relaxed) {
        schedule();
    }
}

/// The idle task's loop: run whatever is runnable, otherwise halt.
pub fn idle_loop() -> ! {
    loop {
        arch::disable_interrupts();
        let (runnable, dead) = {
            let mut s = SCHED.lock();
            (!s.run_queue.is_empty(), core::mem::take(&mut s.graveyard))
        };
        drop(dead);
        if runnable {
            arch::enable_interrupts();
            schedule();
        } else {
            // `sti; hlt` is atomic with respect to interrupts.
            unsafe { core::arch::asm!("sti; hlt", options(nomem, nostack)) };
        }
    }
}
