//! Tasks: the unit of scheduling. A task is either a kernel thread or a
//! user process (one thread per process).

pub mod sched;
pub mod wait;

use crate::arch::context::Context;
use crate::fs::FdTable;
use crate::mm::kstack::KernelStack;
use crate::proc::mm::MemorySpace;
use crate::proc::signal::SignalState;
use crate::proc::ProcState;
use crate::sync::{Shared, SpinLock};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering};
use wait::WaitQueue;

pub use sched::{current, schedule, yield_now};

pub type Pid = u32;

/// `group_exit` value meaning "no exit_group pending".
pub const NO_GROUP_EXIT: i32 = i32::MIN;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum State {
    Ready = 0,
    Running = 1,
    Blocked = 2,
    Zombie = 3,
}

impl State {
    fn from_u8(v: u8) -> State {
        match v {
            0 => State::Ready,
            1 => State::Running,
            2 => State::Blocked,
            _ => State::Zombie,
        }
    }

    pub fn letter(self) -> char {
        match self {
            State::Ready | State::Running => 'R',
            State::Blocked => 'S',
            State::Zombie => 'Z',
        }
    }
}

pub struct Task {
    pub pid: Pid,
    pub name: SpinLock<String>,
    /// Changed only by the scheduler (with interrupts disabled).
    state: AtomicU8,
    context: UnsafeCell<Context>,
    /// None for the idle task, which runs on the boot stack.
    kstack: Option<KernelStack>,
    pub exit_code: AtomicI32,
    /// Woken when the task becomes a zombie.
    pub exited: WaitQueue,
    detached: AtomicBool,
    /// Ticks spent running (for /proc and `ps`).
    pub cpu_ticks: AtomicU64,
    /// FPU/SSE registers while the task is switched out.
    fpu: UnsafeCell<crate::arch::fpu::FpuState>,

    // Process state (unused by kernel threads).
    /// Page table root to load when running; 0 = kernel page table.
    pub cr3: AtomicU64,
    user: AtomicBool,
    /// User FS base (thread pointer), restored on every switch.
    pub fs_base: AtomicU64,
    /// Address space; shared by the threads of a process.
    pub mm: Shared<Option<MemorySpace>>,
    /// File descriptor table; shared with CLONE_FILES.
    pub files: Shared<FdTable>,
    /// Thread group id: the pid of the process this thread belongs to.
    pub tgid: AtomicU32,
    /// CLONE_CHILD_CLEARTID / set_tid_address: zeroed and futex-woken on exit.
    pub clear_tid: AtomicU64,
    /// Set when a vfork child execs or exits (the parent waits for it).
    pub vfork_done: AtomicBool,
    pub vfork_wait: WaitQueue,
    /// Exit status requested by exit_group from another thread.
    pub group_exit: AtomicI32,
    pub proc: SpinLock<ProcState>,
    pub signals: SpinLock<SignalState>,
    /// Woken when a child of this process exits.
    pub child_exit: WaitQueue,
}

// `context` is only touched by the scheduler with interrupts disabled.
unsafe impl Sync for Task {}
unsafe impl Send for Task {}

impl Task {
    pub fn state(&self) -> State {
        State::from_u8(self.state.load(Ordering::Acquire))
    }

    fn set_state(&self, s: State) {
        self.state.store(s as u8, Ordering::Release);
    }

    pub fn kernel_stack_top(&self) -> Option<u64> {
        self.kstack.as_ref().map(|s| s.top())
    }

    fn context_ptr(&self) -> *mut Context {
        self.context.get()
    }

    pub fn name(&self) -> String {
        self.name.lock().clone()
    }

    /// True for user processes (also after they exited).
    /// Saved FPU state (valid while the task is not running).
    ///
    /// # Safety
    /// Only the scheduler and the task itself may touch it.
    pub unsafe fn fpu(&self) -> &mut crate::arch::fpu::FpuState {
        &mut *self.fpu.get()
    }

    pub fn tgid(&self) -> Pid {
        self.tgid.load(Ordering::Acquire)
    }

    /// True for a non-leader thread of a process.
    pub fn is_thread(&self) -> bool {
        self.tgid() != self.pid
    }

    pub fn is_user(&self) -> bool {
        self.user.load(Ordering::Acquire)
    }

    pub fn set_user(&self) {
        self.user.store(true, Ordering::Release);
    }
}

static NEXT_PID: AtomicU32 = AtomicU32::new(1);
static TASKS: SpinLock<BTreeMap<Pid, Arc<Task>>> = SpinLock::new(BTreeMap::new());

pub fn alloc_pid() -> Pid {
    NEXT_PID.fetch_add(1, Ordering::Relaxed)
}

pub fn lookup(pid: Pid) -> Option<Arc<Task>> {
    TASKS.lock().get(&pid).cloned()
}

pub fn all_tasks() -> Vec<Arc<Task>> {
    TASKS.lock().values().cloned().collect()
}

fn new_task(pid: Pid, name: &str, kstack: Option<KernelStack>, context: Context) -> Arc<Task> {
    Arc::new(Task {
        pid,
        name: SpinLock::new(String::from(name)),
        state: AtomicU8::new(State::Ready as u8),
        context: UnsafeCell::new(context),
        kstack,
        exit_code: AtomicI32::new(0),
        exited: WaitQueue::new(),
        detached: AtomicBool::new(false),
        cpu_ticks: AtomicU64::new(0),
        fpu: UnsafeCell::new(crate::arch::fpu::FpuState::initial()),
        cr3: AtomicU64::new(0),
        user: AtomicBool::new(false),
        fs_base: AtomicU64::new(0),
        mm: Shared::new(None),
        files: Shared::new(FdTable::new()),
        tgid: AtomicU32::new(pid),
        clear_tid: AtomicU64::new(0),
        vfork_done: AtomicBool::new(false),
        vfork_wait: WaitQueue::new(),
        group_exit: AtomicI32::new(NO_GROUP_EXIT),
        proc: SpinLock::new(ProcState::kernel()),
        signals: SpinLock::new(SignalState::new()),
        child_exit: WaitQueue::new(),
    })
}

/// Creates a task that will enter user mode with `context`; the caller
/// fills in its process state before calling [`start`].
pub fn new_user_task(name: &str, kstack: KernelStack, context: Context) -> Arc<Task> {
    new_task(alloc_pid(), name, Some(kstack), context)
}

/// Makes a fully initialized task visible and runnable.
pub fn start(task: &Arc<Task>) {
    TASKS.lock().insert(task.pid, task.clone());
    sched::make_runnable(task);
}

/// Removes a reaped task from the task table.
pub fn remove(pid: Pid) -> Option<Arc<Task>> {
    TASKS.lock().remove(&pid)
}

extern "C" fn closure_entry(arg: u64) -> i32 {
    let f = unsafe { Box::from_raw(arg as *mut Box<dyn FnOnce() -> i32 + Send>) };
    f()
}

/// Starts a kernel thread running `f`. Join it with [`join`] or call
/// [`detach`] so it is cleaned up automatically when it exits.
pub fn spawn_kernel<F: FnOnce() -> i32 + Send + 'static>(name: &str, f: F) -> Arc<Task> {
    let boxed: Box<Box<dyn FnOnce() -> i32 + Send>> = Box::new(Box::new(f));
    let kstack = KernelStack::new().expect("out of memory for kernel stack");
    let context = Context::new_kernel(kstack.top(), closure_entry, Box::into_raw(boxed) as u64);
    let task = new_task(alloc_pid(), name, Some(kstack), context);
    TASKS.lock().insert(task.pid, task.clone());
    sched::make_runnable(&task);
    task
}

/// Lets an exited kernel thread be reaped without a `join`.
pub fn detach(task: &Arc<Task>) {
    task.detached.store(true, Ordering::Release);
    if task.state() == State::Zombie {
        TASKS.lock().remove(&task.pid);
    }
}

/// Waits for a kernel thread to exit and returns its exit code.
pub fn join(task: Arc<Task>) -> i32 {
    task.exited
        .wait_uninterruptible(|| (task.state() == State::Zombie).then_some(()));
    TASKS.lock().remove(&task.pid);
    task.exit_code.load(Ordering::Acquire)
}

/// Terminates the current task.
pub fn exit_current(code: i32) -> ! {
    let me = current();
    me.exit_code.store(code, Ordering::Release);
    crate::arch::disable_interrupts();
    sched::set_zombie(&me);
    me.exited.wake_all();
    if me.detached.load(Ordering::Acquire) {
        // The task table must not free our stack while we still run on it:
        // hand the last reference to the idle task.
        TASKS.lock().remove(&me.pid);
        sched::defer_drop(me);
    } else {
        drop(me);
    }
    schedule();
    unreachable!("zombie task was scheduled");
}

#[no_mangle]
extern "C" fn kthread_exit(code: i32) -> ! {
    exit_current(code)
}

/// Puts the current task to sleep for at least `ms` milliseconds.
pub fn sleep_ms(ms: u64) {
    let target = crate::time::ticks() + crate::time::ms_to_ticks(ms);
    sched::SLEEPERS.wait_uninterruptible(|| (crate::time::ticks() >= target).then_some(()));
}

pub const TESTS: &[crate::ktest::Test] = ktests![
    tests::threads_run_and_join,
    tests::mutex_contention,
    tests::sleep,
    tests::preemption,
    tests::wait_queue_handoff,
];

mod tests {
    use super::*;
    use crate::sync::Mutex;

    pub fn threads_run_and_join() {
        let handles: Vec<_> = (0..8).map(|i| spawn_kernel("t", move || i * 2)).collect();
        let sum: i32 = handles.into_iter().map(join).sum();
        assert_eq!(sum, (0..8).map(|i| i * 2).sum());
    }

    pub fn mutex_contention() {
        let counter = Arc::new(Mutex::new(0u64));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let c = counter.clone();
                spawn_kernel("m", move || {
                    for _ in 0..500 {
                        let mut g = c.lock();
                        let v = *g;
                        yield_now(); // force interleaving while holding the mutex
                        *g = v + 1;
                    }
                    0
                })
            })
            .collect();
        handles.into_iter().for_each(|h| {
            join(h);
        });
        assert_eq!(*counter.lock(), 2000);
    }

    pub fn sleep() {
        let start = crate::time::ticks();
        sleep_ms(50);
        assert!(crate::time::ticks() - start >= 5);
    }

    pub fn preemption() {
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        // Busy loop that never yields: only the timer can take the CPU away.
        let spinner = spawn_kernel("spin", move || {
            while !s.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
            0
        });
        sleep_ms(30);
        stop.store(true, Ordering::Relaxed);
        join(spinner);
    }

    pub fn wait_queue_handoff() {
        let wq = Arc::new(WaitQueue::new());
        let value = Arc::new(AtomicU32::new(0));
        let (w, v) = (wq.clone(), value.clone());
        let consumer = spawn_kernel("consumer", move || {
            let got = w.wait_uninterruptible(|| match v.load(Ordering::Acquire) {
                0 => None,
                x => Some(x),
            });
            got as i32
        });
        sleep_ms(20);
        value.store(42, Ordering::Release);
        wq.wake_all();
        assert_eq!(join(consumer), 42);
    }
}
