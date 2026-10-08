//! Process lifecycle: init, fork, exit, wait and process groups.

use super::mm::MemorySpace;
use super::{exec, signal, ProcState};
use crate::arch::{self, context::Context, TrapFrame};
use crate::fs::{self, KResult};
use crate::mm::kstack::KernelStack;
use crate::task::{self, sched, Pid, State};
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
        String::from("PATH=/bin:/sbin"),
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

pub fn fork(frame: &TrapFrame) -> KResult<Pid> {
    let parent = sched::current();
    let mm: MemorySpace = parent
        .mm
        .lock()
        .as_ref()
        .ok_or(Errno::EINVAL)?
        .duplicate()?;

    let mut child_frame = *frame;
    child_frame.rax = 0;
    let kstack = KernelStack::new().ok_or(Errno::ENOMEM)?;
    let context = Context::new_user(kstack.top(), &child_frame);
    let child = task::new_user_task(&parent.name(), kstack, context);

    child.cr3.store(mm.root(), Ordering::Release);
    child.set_user();
    child
        .fs_base
        .store(parent.fs_base.load(Ordering::Relaxed), Ordering::Relaxed);
    *child.mm.lock() = Some(mm);
    *child.files.lock() = parent.files.lock().clone();
    {
        let p = parent.proc.lock();
        *child.proc.lock() = ProcState {
            ppid: parent.pid,
            pgid: p.pgid,
            sid: p.sid,
            cwd: p.cwd.clone(),
            umask: p.umask,
            cmdline: p.cmdline.clone(),
            children: Vec::new(),
        };
    }
    *child.signals.lock() = parent.signals.lock().fork();
    parent.proc.lock().children.push(child.pid);
    task::start(&child);
    Ok(child.pid)
}

/// Terminates the current process with a wait status (see `huldra_abi::process`).
pub fn exit_process(status: i32) -> ! {
    let me = sched::current();
    if me.pid == 1 {
        panic!("attempted to kill init (status {:#x})", status);
    }

    // Close files (may wake pipe readers) and free the address space.
    let files = core::mem::take(&mut *me.files.lock());
    drop(files);
    unsafe { crate::mm::vmm::kernel_page_table().activate() };
    me.cr3.store(0, Ordering::Release);
    let mm = me.mm.lock().take();
    drop(mm);

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
