//! /sbin/init: the first user process.
//!
//! Mounts the file systems listed in /etc/fstab, prints the banner and keeps
//! a shell running on the console, reaping orphaned processes meanwhile.

#![no_std]
#![no_main]

use huldra_user::process::{self, ExitStatus};
use huldra_user::{env, eprintln, fs, println, signal, sys, time, String, Vec};

huldra_user::main!(main);

fn mount_fstab() {
    let Ok(text) = fs::read_to_string("/etc/fstab") else {
        return;
    };
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 3 {
            continue;
        }
        let (source, target, fstype) = (f[0], f[1], f[2]);
        if source.starts_with("/dev/") && !fs::exists(source) {
            continue; // device not present
        }
        let _ = fs::create_dir_all(target);
        match sys::mount(source, target, fstype, 0) {
            Ok(()) => println!("init: mounted {} on {} ({})", source, target, fstype),
            Err(e) => eprintln!("init: mount {} on {}: {}", source, target, e),
        }
    }
}

fn spawn_shell(shell: &str) -> Option<process::Pid> {
    spawn_shell_with(shell, &["-sh"])
}

/// Runs a program on the console in a new session.
fn spawn_shell_with(shell: &str, args: &[&str]) -> Option<process::Pid> {
    match process::fork() {
        Ok(None) => {
            // New session with the console as controlling terminal.
            let _ = sys::setsid();
            let _ = process::set_foreground(0, process::getpid());
            let _ = signal::default(signal::SIGINT);
            let e = process::exec(shell, args, &env::envp());
            eprintln!("init: cannot run {}: {}", shell, e);
            process::exit(127)
        }
        Ok(Some(pid)) => Some(pid),
        Err(e) => {
            eprintln!("init: fork: {}", e);
            None
        }
    }
}

fn main() -> i32 {
    // init ignores ^C: it must never die.
    let _ = signal::ignore(signal::SIGINT);
    let _ = signal::ignore(signal::SIGQUIT);
    env::set_var("PATH", "/bin:/sbin:/usr/bin:/usr/local/bin:/pkg/system/sw/bin");
    env::set_var("HOME", "/root");
    env::set_var("SHELL", "/bin/sh");
    env::set_var("TERM", "linux");
    let _ = fs::set_current_dir("/root");

    mount_fstab();
    let version = fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| {
                l.strip_prefix("PRETTY_NAME=")
                    .map(|v| String::from(v.trim_matches('"')))
            })
        })
        .unwrap_or_else(|| String::from("Huldra"));
    println!(
        "\n\x1b[1;36m{}\x1b[0m (init: pid {})",
        version,
        process::getpid()
    );

    // `gui` on the kernel command line: start the graphical session
    // first; the console shell follows when it ends.
    let gui = fs::read_to_string("/proc/cmdline").is_ok_and(|c| c.split_whitespace().any(|w| w == "gui"));
    if gui {
        if let Some(pid) = spawn_shell_with("/bin/startgui", &["startgui"]) {
            let _ = process::wait(pid as i32);
        }
    }
    let shell = env::var("INIT_SHELL").unwrap_or("/bin/sh");
    let mut shell_pid = spawn_shell(shell);
    loop {
        match process::wait(-1) {
            Ok((pid, status)) if Some(pid) == shell_pid => {
                if let ExitStatus::Signaled(sig) = status {
                    eprintln!(
                        "init: shell killed by signal {} ({})",
                        sig,
                        signal::name(sig)
                    );
                }
                time::sleep_ms(200);
                let _ = process::set_foreground(0, 1);
                shell_pid = spawn_shell(shell);
            }
            Ok(_) => {} // reaped an orphan
            Err(_) => time::sleep_ms(500),
        }
    }
}
