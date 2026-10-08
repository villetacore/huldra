//! utest: system call tests run from user space (part of `cargo xtask test`).

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use huldra_user::abi::fs::*;
use huldra_user::abi::mm::*;
use huldra_user::process::{self, ExitStatus};
use huldra_user::{env, fs, println, signal, sys, time, Errno};

huldra_user::main!(main);

type TestResult = Result<(), String>;

macro_rules! check {
    ($cond:expr) => {
        if !$cond {
            return Err(huldra_user::format!("line {}: {}", line!(), stringify!($cond)));
        }
    };
    ($cond:expr, $($arg:tt)*) => {
        if !$cond {
            return Err(huldra_user::format!("line {}: {}", line!(), huldra_user::format!($($arg)*)));
        }
    };
}

/// Runs `f` in a child process and returns how it ended.
fn in_child(f: impl FnOnce() -> i32) -> ExitStatus {
    match process::fork().expect("fork") {
        None => process::exit(f()),
        Some(pid) => process::wait(pid as i32).expect("wait").1,
    }
}

/// Runs `f` in a child with stdout redirected into a pipe; returns the output.
fn capture(f: impl FnOnce() -> i32) -> (ExitStatus, String) {
    let (r, w) = sys::pipe().expect("pipe");
    match process::fork().expect("fork") {
        None => {
            let _ = sys::close(r);
            let _ = sys::dup2(w, 1);
            let _ = sys::close(w);
            process::exit(f())
        }
        Some(pid) => {
            let _ = sys::close(w);
            let mut out = Vec::new();
            let mut buf = [0u8; 512];
            loop {
                match sys::read(r, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => out.extend_from_slice(&buf[..n]),
                    Err(Errno::EINTR) => continue,
                    Err(_) => break,
                }
            }
            let _ = sys::close(r);
            let status = process::wait(pid as i32).expect("wait").1;
            (status, String::from_utf8_lossy(&out).into_owned())
        }
    }
}

fn fork_and_exit_codes() -> TestResult {
    for code in [0, 1, 42, 255] {
        check!(in_child(|| code) == ExitStatus::Exited(code));
    }
    Ok(())
}

fn many_children() -> TestResult {
    let mut pids = Vec::new();
    for i in 0..20 {
        match process::fork().map_err(|e| huldra_user::format!("{}", e))? {
            None => {
                time::sleep_ms(10);
                process::exit(i)
            }
            Some(pid) => pids.push((pid, i)),
        }
    }
    let mut seen = 0;
    while let Ok((pid, status)) = process::wait(-1) {
        let expected = pids.iter().find(|p| p.0 == pid).map(|p| p.1);
        check!(
            Some(status) == expected.map(ExitStatus::Exited),
            "pid {} status {:?}",
            pid,
            status
        );
        seen += 1;
    }
    check!(seen == 20);
    check!(process::wait(-1) == Err(Errno::ECHILD));
    Ok(())
}

fn fork_copies_memory() -> TestResult {
    let mut v = alloc::vec![1u64; 10_000];
    let status = in_child(|| {
        v.iter_mut().for_each(|x| *x = 7);
        (v.iter().sum::<u64>() == 70_000) as i32
    });
    check!(status == ExitStatus::Exited(1));
    check!(
        v.iter().sum::<u64>() == 10_000,
        "parent memory changed by child"
    );
    Ok(())
}

fn pipes_between_processes() -> TestResult {
    let (status, out) = capture(|| {
        for i in 0..1000 {
            huldra_user::print!("{} ", i);
        }
        0
    });
    check!(status.success());
    let numbers: Vec<u32> = out
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    check!(numbers.len() == 1000 && numbers[999] == 999);
    Ok(())
}

fn exec_with_args_and_env() -> TestResult {
    let (status, out) = capture(|| {
        let e = process::exec(
            "/bin/echo",
            &["echo", "one", "two  three"],
            &[String::from("X=1")],
        );
        huldra_user::eprintln!("exec: {}", e);
        1
    });
    check!(status.success());
    check!(out == "one two  three\n", "got {:?}", out);
    let (_, out) = capture(|| {
        process::exec(
            "/bin/env",
            &["env"],
            &[String::from("GREETING=hello"), String::from("A=b")],
        );
        1
    });
    check!(out.contains("GREETING=hello"), "env got {:?}", out);
    Ok(())
}

fn exec_errors() -> TestResult {
    check!(process::exec("/no/such/file", &["x"], &[]) == Errno::ENOENT);
    fs::write("/tmp/utest-text", b"just text, no #!").map_err(|e| huldra_user::format!("{}", e))?;
    check!(process::exec("/tmp/utest-text", &["x"], &[]) == Errno::EACCES);
    let fd = sys::open("/tmp/utest-noexec", O_WRONLY | O_CREAT | O_TRUNC, 0o755).unwrap();
    let _ = sys::write(fd, b"garbage that is not an ELF file");
    let _ = sys::close(fd);
    check!(process::exec("/tmp/utest-noexec", &["x"], &[]) == Errno::ENOEXEC);
    let _ = fs::remove_file("/tmp/utest-text");
    let _ = fs::remove_file("/tmp/utest-noexec");
    Ok(())
}

fn shebang_scripts() -> TestResult {
    let fd = sys::open("/tmp/utest.sh", O_WRONLY | O_CREAT | O_TRUNC, 0o755).unwrap();
    let _ = sys::write(fd, b"#!/bin/sh\necho script says $1\n");
    let _ = sys::close(fd);
    let (status, out) = capture(|| {
        process::exec("/tmp/utest.sh", &["utest.sh", "hi"], &env::envp());
        1
    });
    let _ = fs::remove_file("/tmp/utest.sh");
    check!(
        status.success() && out == "script says hi\n",
        "got {:?} ({:?})",
        out,
        status
    );
    Ok(())
}

static SIGNALS: AtomicU32 = AtomicU32::new(0);

extern "C" fn count_signal(sig: i32) {
    SIGNALS.fetch_add(sig as u32, Ordering::SeqCst);
}

fn signal_handlers() -> TestResult {
    SIGNALS.store(0, Ordering::SeqCst);
    signal::handle(signal::SIGUSR1, count_signal).unwrap();
    signal::kill(process::getpid() as i32, signal::SIGUSR1).unwrap();
    check!(
        SIGNALS.load(Ordering::SeqCst) == signal::SIGUSR1,
        "handler did not run"
    );
    // Blocked signals wait until unblocked.
    signal::block(signal::SIGUSR1).unwrap();
    signal::kill(process::getpid() as i32, signal::SIGUSR1).unwrap();
    check!(SIGNALS.load(Ordering::SeqCst) == signal::SIGUSR1);
    signal::unblock(signal::SIGUSR1).unwrap();
    check!(SIGNALS.load(Ordering::SeqCst) == 2 * signal::SIGUSR1);
    signal::default(signal::SIGUSR1).unwrap();
    Ok(())
}

fn default_actions() -> TestResult {
    let status = in_child(|| {
        let _ = signal::kill(process::getpid() as i32, signal::SIGTERM);
        0
    });
    check!(
        status == ExitStatus::Signaled(signal::SIGTERM),
        "{:?}",
        status
    );
    let status = in_child(|| {
        let _ = signal::ignore(signal::SIGTERM);
        let _ = signal::kill(process::getpid() as i32, signal::SIGTERM);
        5
    });
    check!(status == ExitStatus::Exited(5), "{:?}", status);
    Ok(())
}

fn kill_sleeping_child() -> TestResult {
    let pid = match process::fork().unwrap() {
        None => {
            time::sleep_ms(60_000);
            process::exit(0)
        }
        Some(pid) => pid,
    };
    time::sleep_ms(50);
    signal::kill(pid as i32, signal::SIGKILL).unwrap();
    let (_, status) = process::wait(pid as i32).unwrap();
    check!(
        status == ExitStatus::Signaled(signal::SIGKILL),
        "{:?}",
        status
    );
    Ok(())
}

extern "C" fn ignore_signal(_: i32) {}

fn interrupted_read() -> TestResult {
    // A read blocked on an empty pipe fails with EINTR when a handler runs.
    let (r, w) = sys::pipe().unwrap();
    let pid = match process::fork().unwrap() {
        None => {
            let _ = sys::close(w);
            signal::handle_interrupting(signal::SIGUSR2, ignore_signal).unwrap();
            let mut buf = [0u8; 8];
            let code = match sys::read(r, &mut buf) {
                Err(Errno::EINTR) => 42,
                _ => 1,
            };
            process::exit(code)
        }
        Some(pid) => pid,
    };
    time::sleep_ms(50);
    signal::kill(pid as i32, signal::SIGUSR2).unwrap();
    let (_, status) = process::wait(pid as i32).unwrap();
    let _ = sys::close(r);
    let _ = sys::close(w);
    check!(status == ExitStatus::Exited(42), "{:?}", status);
    Ok(())
}

fn segfaults_kill_only_the_child() -> TestResult {
    let status = in_child(|| unsafe { core::ptr::read_volatile(0x10 as *const u64) as i32 });
    check!(
        status == ExitStatus::Signaled(signal::SIGSEGV),
        "null read: {:?}",
        status
    );
    let status = in_child(|| unsafe {
        // Kernel memory is not accessible from user mode.
        core::ptr::write_volatile(0xFFFF_8000_0000_0000 as *mut u64, 1);
        0
    });
    check!(
        status == ExitStatus::Signaled(signal::SIGSEGV),
        "kernel write: {:?}",
        status
    );
    let status = in_child(|| unsafe {
        // Code is not writable.
        core::ptr::write_volatile(main as *const () as *mut u8, 0x90);
        0
    });
    check!(
        status == ExitStatus::Signaled(signal::SIGSEGV),
        "text write: {:?}",
        status
    );
    Ok(())
}

fn bad_pointers_give_efault() -> TestResult {
    check!(
        sys::write(1, unsafe {
            core::slice::from_raw_parts(0x1000 as *const u8, 16)
        }) == Err(Errno::EFAULT)
    );
    check!(
        sys::read(0, unsafe {
            core::slice::from_raw_parts_mut(0xFFFF_8000_0000_0000u64 as *mut u8, 16)
        }) == Err(Errno::EFAULT)
    );
    check!(sys::stat("").err() == Some(Errno::ENOENT));
    Ok(())
}

fn memory_mappings() -> TestResult {
    // Large heap allocation (demand paged).
    let v: Vec<u8> = alloc::vec![3u8; 16 << 20];
    check!(v.iter().step_by(4096).all(|&b| b == 3));
    drop(v);

    let len = 1 << 20;
    let addr = sys::mmap(
        0,
        len,
        PROT_READ | PROT_WRITE,
        MAP_PRIVATE | MAP_ANONYMOUS,
        -1,
        0,
    )
    .unwrap();
    let mem = unsafe { core::slice::from_raw_parts_mut(addr as *mut u8, len) };
    check!(mem.iter().all(|&b| b == 0), "anonymous memory not zeroed");
    mem.fill(0xAB);
    sys::munmap(addr, len).unwrap();
    let status = in_child(|| unsafe { core::ptr::read_volatile(addr as *const u8) as i32 });
    check!(
        status == ExitStatus::Signaled(signal::SIGSEGV),
        "access after munmap: {:?}",
        status
    );

    let ro = sys::mmap(0, 4096, PROT_READ, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0).unwrap();
    let status = in_child(|| unsafe {
        core::ptr::write_volatile(ro as *mut u8, 1);
        0
    });
    check!(status == ExitStatus::Signaled(signal::SIGSEGV));
    sys::mprotect(ro, 4096, PROT_READ | PROT_WRITE).unwrap();
    unsafe { core::ptr::write_volatile(ro as *mut u8, 1) };

    let brk = sys::brk(0);
    check!(sys::brk(brk + 8192) == brk + 8192);
    unsafe { core::ptr::write_volatile((brk + 100) as *mut u8, 9) };
    check!(sys::brk(brk) == brk);
    Ok(())
}

fn files_and_directories() -> TestResult {
    let e = |e: Errno| huldra_user::format!("{}", e);
    fs::create_dir_all("/tmp/utest/a/b").map_err(e)?;
    fs::write("/tmp/utest/a/b/file", b"0123456789").map_err(e)?;
    check!(fs::read("/tmp/utest/a/b/file").map_err(e)? == b"0123456789");

    let fd = sys::open("/tmp/utest/a/b/file", O_RDWR, 0).unwrap();
    check!(sys::lseek(fd, 4, SEEK_SET) == Ok(4));
    let mut buf = [0u8; 3];
    check!(sys::read(fd, &mut buf) == Ok(3) && &buf == b"456");
    check!(sys::lseek(fd, 0, SEEK_END) == Ok(10));
    sys::ftruncate(fd, 4).unwrap();
    check!(sys::fstat(fd).unwrap().st_size == 4);
    let _ = sys::close(fd);
    check!(sys::read(fd, &mut buf) == Err(Errno::EBADF));

    let fd = sys::open("/tmp/utest/a/b/file", O_WRONLY | O_APPEND, 0).unwrap();
    let _ = sys::write(fd, b"XY");
    let _ = sys::close(fd);
    check!(fs::read("/tmp/utest/a/b/file").unwrap() == b"0123XY");

    fs::rename("/tmp/utest/a/b/file", "/tmp/utest/moved").map_err(e)?;
    let names: Vec<String> = fs::read_dir("/tmp/utest")
        .map_err(e)?
        .into_iter()
        .map(|d| d.name)
        .collect();
    check!(names == ["a", "moved"], "{:?}", names);
    check!(fs::remove_dir("/tmp/utest/a") == Err(Errno::ENOTEMPTY));
    check!(sys::open("/tmp/utest/moved", O_CREAT | O_EXCL | O_WRONLY, 0o644) == Err(Errno::EEXIST));
    check!(sys::open("/tmp/utest/a", O_WRONLY, 0) == Err(Errno::EISDIR));
    fs::remove_all("/tmp/utest").map_err(e)?;
    check!(!fs::exists("/tmp/utest"));
    Ok(())
}

fn dup2_redirects_output() -> TestResult {
    let status = in_child(|| {
        let fd = sys::open("/tmp/utest-out", O_WRONLY | O_CREAT | O_TRUNC, 0o644).unwrap();
        sys::dup2(fd, 1).unwrap();
        huldra_user::println!("redirected");
        0
    });
    check!(status.success());
    check!(fs::read_to_string("/tmp/utest-out").unwrap() == "redirected\n");
    let _ = fs::remove_file("/tmp/utest-out");
    Ok(())
}

fn cwd_and_relative_paths() -> TestResult {
    let old = fs::current_dir().unwrap();
    fs::create_dir_all("/tmp/cwdtest/sub").unwrap();
    fs::set_current_dir("/tmp/cwdtest").unwrap();
    check!(fs::current_dir().unwrap() == "/tmp/cwdtest");
    fs::write("sub/../f", b"x").unwrap();
    check!(fs::exists("/tmp/cwdtest/f"));
    fs::set_current_dir("sub").unwrap();
    check!(fs::current_dir().unwrap() == "/tmp/cwdtest/sub");
    check!(fs::set_current_dir("/tmp/cwdtest/f") == Err(Errno::ENOTDIR));
    fs::set_current_dir(&old).unwrap();
    fs::remove_all("/tmp/cwdtest").unwrap();
    Ok(())
}

fn sleep_timing() -> TestResult {
    let start = time::uptime_ms();
    time::sleep_ms(100);
    let elapsed = time::uptime_ms() - start;
    check!((100..1000).contains(&elapsed), "slept {} ms", elapsed);
    Ok(())
}

fn process_groups() -> TestResult {
    let status = in_child(|| {
        if sys::setpgid(0, 0).is_err() {
            return 1;
        }
        (sys::getpgrp() == process::getpid()) as i32 + 10
    });
    check!(status == ExitStatus::Exited(11), "{:?}", status);
    Ok(())
}

fn proc_filesystem() -> TestResult {
    let me = process::getpid();
    let status = fs::read_to_string(&huldra_user::format!("/proc/{}/status", me)).unwrap();
    check!(status.contains("Name:\tutest"), "{}", status);
    check!(fs::read_to_string("/proc/self/cmdline")
        .unwrap()
        .starts_with("utest"));
    check!(fs::read_to_string("/proc/meminfo")
        .unwrap()
        .contains("MemFree"));
    Ok(())
}

fn main() -> i32 {
    let tests: &[(&str, fn() -> TestResult)] = &[
        ("fork_and_exit_codes", fork_and_exit_codes),
        ("many_children", many_children),
        ("fork_copies_memory", fork_copies_memory),
        ("pipes_between_processes", pipes_between_processes),
        ("exec_with_args_and_env", exec_with_args_and_env),
        ("exec_errors", exec_errors),
        ("shebang_scripts", shebang_scripts),
        ("signal_handlers", signal_handlers),
        ("default_actions", default_actions),
        ("kill_sleeping_child", kill_sleeping_child),
        ("interrupted_read", interrupted_read),
        (
            "segfaults_kill_only_the_child",
            segfaults_kill_only_the_child,
        ),
        ("bad_pointers_give_efault", bad_pointers_give_efault),
        ("memory_mappings", memory_mappings),
        ("files_and_directories", files_and_directories),
        ("dup2_redirects_output", dup2_redirects_output),
        ("cwd_and_relative_paths", cwd_and_relative_paths),
        ("sleep_timing", sleep_timing),
        ("process_groups", process_groups),
        ("proc_filesystem", proc_filesystem),
    ];
    let filter = env::args().get(1);
    let (mut passed, mut failed) = (0, 0);
    for (name, f) in tests {
        if filter.is_some_and(|w| !name.contains(w.as_str())) {
            continue;
        }
        match f() {
            Ok(()) => {
                passed += 1;
                println!("utest {} ... ok", name);
            }
            Err(e) => {
                failed += 1;
                println!("utest {} ... FAILED: {}", name, e);
            }
        }
    }
    println!("utest: {} passed, {} failed", passed, failed);
    (failed > 0) as i32
}
