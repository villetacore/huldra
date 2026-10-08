//! Built-in kernel shell: a stand-in for /sbin/init until user mode exists.
//! Reads lines from the console TTY (which does the line editing).

use crate::fs::{self, vfs, Inode};
use crate::{arch, bootinfo, mm, time};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::{self, Write};
use huldra_abi::fs::*;

enum Out {
    Console,
    Buffer(String),
}

impl Write for Out {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        match self {
            Out::Console => print!("{}", s),
            Out::Buffer(buf) => buf.push_str(s),
        }
        Ok(())
    }
}

macro_rules! out {
    ($out:expr, $($arg:tt)*) => {{ let _ = writeln!($out, $($arg)*); }};
}

type CmdResult = Result<(), String>;

pub fn run() -> i32 {
    if let Ok(motd) = fs::read_file("/etc/motd") {
        print!("\n{}", String::from_utf8_lossy(&motd));
    }
    let mut cwd = String::from("/root");
    let tty = crate::drivers::tty::console();
    loop {
        print!("\x1b[92mroot@{}\x1b[0m:\x1b[94m{}\x1b[0m# ", hostname(), cwd);
        let mut buf = [0u8; 1024];
        let n = tty.read_at(0, &mut buf).unwrap_or(0);
        let line = String::from_utf8_lossy(&buf[..n]).into_owned();
        execute(line.trim(), &mut cwd);
    }
}

fn hostname() -> String {
    fs::read_file("/etc/hostname")
        .map(|h| String::from_utf8_lossy(&h).trim().to_string())
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "localhost".into())
}

fn execute(line: &str, cwd: &mut String) {
    if line.is_empty() || line.starts_with('#') {
        return;
    }
    let (command, redirect) = match line.find('>') {
        None => (line, None),
        Some(i) => {
            let (cmd, rest) = line.split_at(i);
            let (append, target) = match rest.strip_prefix(">>") {
                Some(t) => (true, t),
                None => (false, &rest[1..]),
            };
            (cmd, Some((target.trim(), append)))
        }
    };
    let args: Vec<&str> = command.split_whitespace().collect();
    if args.is_empty() {
        return;
    }
    let mut out = if redirect.is_some() { Out::Buffer(String::new()) } else { Out::Console };
    if let Err(msg) = run_command(&args, &mut out, cwd) {
        println!("\x1b[91m{}: {}\x1b[0m", args[0], msg);
    }
    if let (Some((path, append)), Out::Buffer(buf)) = (redirect, out) {
        let result = vfs::normalize(cwd, path).and_then(|p| {
            let flags = O_WRONLY | O_CREAT | if append { O_APPEND } else { O_TRUNC };
            fs::open(&p, flags, 0o644)?.write(buf.as_bytes())
        });
        if let Err(e) = result {
            println!("\x1b[91msh: {}: {}\x1b[0m", path, e);
        }
    }
}

fn path(cwd: &str, p: &str) -> Result<String, String> {
    vfs::normalize(cwd, p).map_err(|e| format!("{}: {}", p, e))
}

fn err(p: &str) -> impl Fn(huldra_abi::errno::Errno) -> String + '_ {
    move |e| format!("{}: {}", p, e)
}

fn run_command(args: &[&str], out: &mut Out, cwd: &mut String) -> CmdResult {
    let rest = &args[1..];
    match args[0] {
        "help" => out!(out, "commands: ls cd pwd cat echo mkdir rmdir rm touch mv uname uptime free bootinfo reboot poweroff"),
        "echo" => out!(out, "{}", rest.join(" ")),
        "clear" => crate::console::clear(),
        "uname" => {
            if rest.first() == Some(&"-a") {
                out!(out, "{} {} {} x86_64", crate::NAME, hostname(), crate::VERSION);
            } else {
                out!(out, "{}", crate::NAME);
            }
        }
        "uptime" => {
            let s = time::uptime_ms() / 1000;
            out!(out, "up {}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60);
        }
        "free" => {
            let (free, total) = mm::frame::stats();
            out!(out, "Mem: {} KiB total, {} KiB free", total * 4, free * 4);
        }
        "bootinfo" => {
            let b = bootinfo::get();
            out!(out, "protocol: {}\ncmdline: {}", b.protocol, b.cmdline.as_str());
            for r in b.memory.iter() {
                out!(out, "  {:#012x}..{:#012x} {}", r.base, r.base + r.len, r.kind_name());
            }
        }
        "pwd" => out!(out, "{}", cwd),
        "cd" => {
            let p = path(cwd, rest.first().copied().unwrap_or("/root"))?;
            if !fs::stat(&p).map_err(err(&p))?.is_dir() {
                return Err(format!("{}: Not a directory", p));
            }
            *cwd = p;
        }
        "ls" => {
            let p = path(cwd, rest.first().copied().unwrap_or("."))?;
            let node = vfs::lookup(&p).map_err(err(&p))?;
            if !node.metadata().is_dir() {
                out!(out, "{}", p);
                return Ok(());
            }
            for e in node.readdir().map_err(err(&p))? {
                let suffix = if e.kind == fs::FileType::Directory { "/" } else { "" };
                out!(out, "{}{}", e.name, suffix);
            }
        }
        "cat" => {
            for a in rest {
                let p = path(cwd, a)?;
                let data = fs::read_file(&p).map_err(err(a))?;
                let _ = write!(out, "{}", String::from_utf8_lossy(&data));
            }
        }
        "mkdir" => {
            for a in rest {
                fs::mkdir(&path(cwd, a)?, 0o755).map_err(err(a))?;
            }
        }
        "rmdir" => {
            for a in rest {
                fs::rmdir(&path(cwd, a)?).map_err(err(a))?;
            }
        }
        "rm" => {
            for a in rest {
                fs::unlink(&path(cwd, a)?).map_err(err(a))?;
            }
        }
        "touch" => {
            for a in rest {
                fs::open(&path(cwd, a)?, O_WRONLY | O_CREAT, 0o644).map_err(err(a))?;
            }
        }
        "mv" if rest.len() == 2 => fs::rename(&path(cwd, rest[0])?, &path(cwd, rest[1])?).map_err(err(rest[0]))?,
        "reboot" => arch::reboot(),
        "poweroff" | "halt" => arch::poweroff(),
        _ => return Err("command not found".into()),
    }
    Ok(())
}
