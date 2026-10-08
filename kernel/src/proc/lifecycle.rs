//! Process lifecycle: init, fork, exit, wait and process groups.

use super::mm::MemorySpace;
use super::{exec, signal, uaccess, ProcState};
use crate::arch::{self, context::Context, TrapFrame};
use crate::fs::{self, FdTable, KResult};
use crate::mm::kstack::KernelStack;
use crate::task::{self, sched, Pid, State, Task};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use huldra_abi::errno::Errno;
use huldra_abi::fs::O_RDWR;
use huldra_abi::process::WNOHANG;
use huldra_abi::signal::SIGCHLD;

/// Turns the current kernel thread into pid 1 running `path`.
pub fn run_init(path: &str) -> ! {
    let me = sched::current();
    assert_eq!(me.pid, 1, "init must be pid 1");
    {
        let mut p = me.proc.lock();
        p.pgid = 1;
        p.sid = 1;
    }
    let console = fs::open("/dev/console", O_RDWR, 0).expect("cannot open /dev/console");
    {
        let mut files = me.files.lock();
        for _ in 0..3 {
            files.alloc(console.clone(), false).unwrap();
        }
    }
    let tty = crate::drivers::tty::console();
    tty.set_session(1);
    tty.set_foreground(1);

    let env = alloc::vec![
        String::from("PATH=/bin:/sbin:/usr/bin:/usr/local/bin"),
        String::from("HOME=/root"),
        String::from("TERM=linux")
    ];
    let frame = match exec::exec(path, alloc::vec![String::from(path)], env) {
        Ok(f) => f,
        Err(e) => panic!("cannot execute init program {}: {}", path, e),
    };
    kinfo!("starting {} as pid 1", path);
    let top = me.kernel_stack_top().unwrap();
    drop(me);
    unsafe { arch::context::enter_user(&frame, top) }
}

pub const CLONE_VM: u64 = 0x100;
pub const CLONE_FILES: u64 = 0x400;
pub const CLONE_SIGHAND: u64 = 0x800;
pub const CLONE_VFORK: u64 = 0x4000;
pub const CLONE_PARENT: u64 = 0x8000;
pub const CLONE_THREAD: u64 = 0x10000;
pub const CLONE_SETTLS: u64 = 0x80000;
pub const CLONE_PARENT_SETTID: u64 = 0x100000;
pub const CLONE_CHILD_CLEARTID: u64 = 0x200000;
pub const CLONE_CHILD_SETTID: u64 = 0x1000000;

pub fn fork(frame: &TrapFrame) -> KResult<Pid> {
    clone(frame, SIGCHLD as u64, 0, 0, 0, 0)
}

/// `clone(2)`: creates a process (fork, vfork) or a thread.
pub fn clone(frame: &TrapFrame, flags: u64, stack: u64, parent_tid: u64, child_tid: u64, tls: u64) -> KResult<Pid> {
    let parent = sched::current();
    let thread = flags & CLONE_THREAD != 0;
    if thread && flags & (CLONE_VM | CLONE_SIGHAND) != (CLONE_VM | CLONE_SIGHAND) {
        return Err(Errno::EINVAL);
    }

    let mut child_frame = *frame;
    child_frame.rax = 0;
    if stack != 0 {
        child_frame.rsp = stack;
    }
    let kstack = KernelStack::new().ok_or(Errno::ENOMEM)?;
    let context = Context::new_user(kstack.top(), &child_frame);
    let child = task::new_user_task(&parent.name(), kstack, context);

    // Address space: shared, or a copy.
    if flags & CLONE_VM != 0 {
        child.mm.set(parent.mm.share());
    } else {
        let mm: MemorySpace = parent.mm.lock().as_ref().ok_or(Errno::EINVAL)?.duplicate()?;
        *child.mm.lock() = Some(mm);
    }
    let root = child.mm.lock().as_ref().map(|m| m.root()).ok_or(Errno::EINVAL)?;
    child.cr3.store(root, Ordering::Release);
    child.set_user();
    let fs_base = if flags & CLONE_SETTLS != 0 { tls } else { parent.fs_base.load(Ordering::Relaxed) };
    child.fs_base.store(fs_base, Ordering::Relaxed);

    if flags & CLONE_FILES != 0 {
        child.files.set(parent.files.share());
    } else {
        *child.files.lock() = parent.files.lock().clone();
    }
    if thread {
        child.tgid.store(parent.tgid(), Ordering::Release);
    }
    {
        let p = parent.proc.lock();
        let ppid = if thread || flags & CLONE_PARENT != 0 { p.ppid } else { parent.pid };
        *child.proc.lock() = ProcState {
            ppid,
            pgid: p.pgid,
            sid: p.sid,
            cwd: p.cwd.clone(),
            umask: p.umask,
            cmdline: p.cmdline.clone(),
            children: Vec::new(),
            exe: p.exe.clone(),
        };
    }
    *child.signals.lock() = parent.signals.lock().fork();
    unsafe {
        parent.fpu().save();
        *child.fpu() = *parent.fpu();
    }

    if flags & CLONE_PARENT_SETTID != 0 {
        uaccess::write_user(parent_tid, &(child.pid as i32))?;
    }
    if flags & CLONE_CHILD_SETTID != 0 {
        let tid = (child.pid as i32).to_le_bytes();
        if let Some(mm) = child.mm.lock().as_mut() {
            mm.write_bytes(child_tid, &tid)?;
        }
    }
    if flags & CLONE_CHILD_CLEARTID != 0 {
        child.clear_tid.store(child_tid, Ordering::Release);
    }

    if !thread {
        let owner = if flags & CLONE_PARENT != 0 { task::lookup(parent.proc.lock().ppid) } else { Some(parent.clone()) };
        if let Some(o) = owner {
            o.proc.lock().children.push(child.pid);
        }
    }
    let pid = child.pid;
    task::start(&child);

    if flags & CLONE_VFORK != 0 {
        // The parent sleeps until the child execs or exits.
        child.vfork_wait.wait_uninterruptible(|| child.vfork_done.load(Ordering::Acquire).then_some(()));
    }
    Ok(pid)
}

/// Wakes a vfork parent (the child has its own image now, or is gone).
pub fn release_vfork(me: &Task) {
    if !me.vfork_done.swap(true, Ordering::AcqRel) {
        me.vfork_wait.wake_all();
    }
}

/// Clears the thread id word and wakes a futex waiter (pthread_join).
fn clear_child_tid(me: &Task) {
    let addr = me.clear_tid.swap(0, Ordering::AcqRel);
    if addr != 0 && uaccess::write_user(addr, &0i32).is_ok() {
        super::futex::wake(me.mm.id(), addr, 1);
    }
}

/// Sends SIGKILL to every other thread of the current thread group.
pub fn kill_other_threads(me: &Task) {
    let tgid = me.tgid();
    for t in task::all_tasks() {
        if t.tgid() == tgid && t.pid != me.pid && t.state() != State::Zombie {
            signal::send(&t, huldra_abi::signal::SIGKILL);
        }
    }
}

/// `exit_group`: terminates every thread of the process.
pub fn exit_group(status: i32) -> ! {
    let me = sched::current();
    kill_other_threads(&me);
    if me.is_thread() {
        if let Some(leader) = task::lookup(me.tgid()) {
            let _ = leader.group_exit.compare_exchange(task::NO_GROUP_EXIT, status, Ordering::AcqRel, Ordering::Acquire);
            signal::send(&leader, huldra_abi::signal::SIGKILL);
        }
        drop(me);
        exit_thread()
    }
    drop(me);
    exit_process(status)
}

/// Ends the current thread (the whole process if it is the leader).
pub fn exit_thread() -> ! {
    let me = sched::current();
    if !me.is_thread() {
        drop(me);
        exit_process(huldra_abi::process::exit_status(0));
    }
    clear_child_tid(&me);
    release_vfork(&me);
    let irq = arch::irq_save();
    me.cr3.store(0, Ordering::Release);
    unsafe { crate::mm::vmm::kernel_page_table().activate() };
    arch::irq_restore(irq);
    drop(me.files.reset(FdTable::new()));
    drop(me.mm.reset(None));
    arch::disable_interrupts();
    sched::set_zombie(&me);
    me.exited.wake_all();
    // Nobody waits for threads: the idle task frees them.
    task::remove(me.pid);
    sched::defer_drop(me);
    sched::schedule();
    unreachable!("exited thread was scheduled");
}

/// Terminates the current process with a wait status (see `huldra_abi::process`).
pub fn exit_process(status: i32) -> ! {
    let me = sched::current();
    if me.is_thread() {
        drop(me);
        exit_thread();
    }
    let status = match me.group_exit.load(Ordering::Acquire) {
        task::NO_GROUP_EXIT => status,
        s => s,
    };
    if me.pid == 1 {
        panic!("attempted to kill init (status {:#x})", status);
    }
    kill_other_threads(&me);
    clear_child_tid(&me);
    release_vfork(&me);

    // Close files (may wake pipe readers) and free the address space.
    drop(me.files.reset(FdTable::new()));
    let irq = arch::irq_save();
    me.cr3.store(0, Ordering::Release);
    unsafe { crate::mm::vmm::kernel_page_table().activate() };
    arch::irq_restore(irq);
    drop(me.mm.reset(None));

    // Orphans are adopted by init.
    let (children, ppid) = {
        let mut p = me.proc.lock();
        (core::mem::take(&mut p.children), p.ppid)
    };
    if !children.is_empty() {
        let init = task::lookup(1).expect("init is gone");
        for c in &children {
            if let Some(t) = task::lookup(*c) {
                t.proc.lock().ppid = 1;
            }
        }
        init.proc.lock().children.extend(children);
        init.child_exit.wake_all();
    }

    me.exit_code.store(status, Ordering::Release);
    arch::disable_interrupts();
    sched::set_zombie(&me);
    if let Some(parent) = task::lookup(ppid) {
        signal::send(&parent, SIGCHLD);
        parent.child_exit.wake_all();
    }
    me.exited.wake_all();
    drop(me);
    sched::schedule();
    unreachable!("zombie process was scheduled");
}

/// `wait4`: waits for a child to exit; returns (pid, status).
pub fn wait(pid: i64, options: u32) -> KResult<(Pid, i32)> {
    let me = sched::current();
    let my_pgid = me.proc.lock().pgid;
    me.child_exit
        .wait_until(|| {
            let children = me.proc.lock().children.clone();
            let mut any = false;
            for c in children {
                let Some(t) = task::lookup(c) else { continue };
                let pgid = t.proc.lock().pgid;
                let matches = match pid {
                    p if p > 0 => c == p as Pid,
                    -1 => true,
                    0 => pgid == my_pgid,
                    p => pgid == (-p) as Pid,
                };
                if !matches {
                    continue;
                }
                any = true;
                if t.state() == State::Zombie {
                    me.proc.lock().children.retain(|&x| x != c);
                    task::remove(c);
                    return Some(Ok((c, t.exit_code.load(Ordering::Acquire))));
                }
            }
            if !any {
                Some(Err(Errno::ECHILD))
            } else if options & WNOHANG != 0 {
                Some(Ok((0, 0)))
            } else {
                None
            }
        })
        .and_then(|r| r)
}

pub fn setpgid(pid: Pid, pgid: Pid) -> KResult<()> {
    let me = sched::current();
    let pid = if pid == 0 { me.pid } else { pid };
    let pgid = if pgid == 0 { pid } else { pgid };
    let target = task::lookup(pid).ok_or(Errno::ESRCH)?;
    if pid != me.pid && target.proc.lock().ppid != me.pid {
        return Err(Errno::ESRCH);
    }
    let mut p = target.proc.lock();
    if p.sid == pid {
        return Err(Errno::EPERM); // session leaders cannot move
    }
    p.pgid = pgid;
    Ok(())
}

pub fn getpgid(pid: Pid) -> KResult<Pid> {
    let t = if pid == 0 {
        sched::current()
    } else {
        task::lookup(pid).ok_or(Errno::ESRCH)?
    };
    let pgid = t.proc.lock().pgid;
    Ok(pgid)
}

pub fn setsid() -> KResult<Pid> {
    let me = sched::current();
    let mut p = me.proc.lock();
    if p.pgid == me.pid {
        return Err(Errno::EPERM);
    }
    p.sid = me.pid;
    p.pgid = me.pid;
    Ok(me.pid)
}
