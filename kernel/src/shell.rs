//! Built-in kernel shell (`ksh`), the stand-in for /sbin/init until
//! user mode exists. Reads from the PS/2 keyboard and the serial port.

use crate::arch::{self, pit};
use crate::drivers::{keyboard, serial, vga::Color};
use crate::{bootinfo, console, fs, mm, syscall};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::arch::asm;
use core::fmt::{self, Write};

const MAX_LINE: usize = 256;

/// Command output: the console, or a buffer when redirected with `>`/`>>`.
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

fn fs_err(path: &str) -> impl Fn(fs::FsError) -> String + '_ {
    move |e| format!("{}: {}", path, e)
}

pub fn run() -> ! {
    if let Ok(motd) = fs::read("/etc/motd") {
        print!("\n{}", String::from_utf8_lossy(&motd));
    }
    let mut line = String::new();
    loop {
        prompt();
        read_line(&mut line);
        execute(&line);
    }
}

fn hostname() -> String {
    fs::read("/etc/hostname")
        .ok()
        .map(|h| String::from_utf8_lossy(&h).trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

fn prompt() {
    console::set_color(Color::LightGreen, Color::Black);
    print!("root@{}", hostname());
    console::reset_color();
    print!(":");
    console::set_color(Color::LightBlue, Color::Black);
    print!("{}", fs::cwd());
    console::reset_color();
    print!("# ");
}

fn read_char() -> u8 {
    loop {
        if let Some(c) = keyboard::read_char().or_else(serial::try_read) {
            return c;
        }
        arch::wait_for_interrupt();
    }
}

fn read_line(line: &mut String) {
    line.clear();
    loop {
        match read_char() {
            b'\r' | b'\n' => {
                println!();
                return;
            }
            0x08 | 0x7F => {
                if line.pop().is_some() {
                    console::backspace();
                }
            }
            0x03 => {
                println!("^C");
                line.clear();
                return;
            }
            0x0C => {
                console::clear();
                prompt();
                print!("{}", line);
            }
            c @ 0x20..=0x7E if line.len() < MAX_LINE => {
                line.push(c as char);
                print!("{}", c as char);
            }
            _ => {}
        }
    }
}

fn error(msg: &str) {
    console::set_color(Color::LightRed, Color::Black);
    println!("{}", msg);
    console::reset_color();
}

fn execute(line: &str) {
    let line = line.trim();
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
            let target = target.trim();
            if target.is_empty() || target.contains('>') {
                error("sh: syntax error near '>'");
                return;
            }
            (cmd, Some((target, append)))
        }
    };

    let args: Vec<&str> = command.split_whitespace().collect();
    if args.is_empty() {
        return;
    }

    let mut out = if redirect.is_some() { Out::Buffer(String::new()) } else { Out::Console };
    if let Err(msg) = run_command(&args, &mut out) {
        error(&format!("{}: {}", args[0], msg));
    }
    if let (Some((path, append)), Out::Buffer(buf)) = (redirect, out) {
        if let Err(e) = fs::write(path, buf.as_bytes(), append) {
            error(&format!("sh: {}: {}", path, e));
        }
    }
}

fn run_command(args: &[&str], out: &mut Out) -> CmdResult {
    let rest = &args[1..];
    match args[0] {
        "help" => help(out),
        "echo" => out!(out, "{}", rest.join(" ")),
        "clear" => console::clear(),
        "uname" => uname(rest, out),
        "uptime" => uptime(out),
        "free" => free(out),
        "bootinfo" => boot_info(out),
        "cpuinfo" => cpuinfo(out),
        "ls" => return ls(rest, out),
        "cd" => return fs::chdir(rest.first().copied().unwrap_or("/root")).map_err(fs_err(rest.first().copied().unwrap_or("/root"))),
        "pwd" => out!(out, "{}", fs::cwd()),
        "mkdir" => return for_each_path(rest, fs::mkdir),
        "touch" => return for_each_path(rest, fs::touch),
        "cat" => return cat(rest, out),
        "rm" => return rm(rest),
        "syscall" => syscall_demo(out),
        "panic" => panic!("panic requested from the shell"),
        "reboot" => arch::reboot(),
        "poweroff" | "halt" => {
            println!("System is going down.");
            arch::poweroff()
        }
        _ => return Err("command not found".to_string()),
    }
    Ok(())
}

fn help(out: &mut Out) {
    const COMMANDS: &[(&str, &str)] = &[
        ("help", "show this help"),
        ("echo TEXT", "print TEXT (supports > and >> redirection)"),
        ("ls [PATH]", "list directory contents"),
        ("cd [DIR]", "change the working directory"),
        ("pwd", "print the working directory"),
        ("cat FILE...", "print files"),
        ("touch FILE...", "create empty files"),
        ("mkdir DIR...", "create directories"),
        ("rm [-r] PATH...", "remove files or directories"),
        ("uname [-a]", "print system information"),
        ("uptime", "time since boot"),
        ("free", "memory usage"),
        ("bootinfo", "boot protocol, command line, memory map"),
        ("cpuinfo", "CPU vendor and model"),
        ("syscall", "issue write(2) and getpid(2) via int 0x80"),
        ("clear", "clear the screen (also Ctrl+L)"),
        ("panic", "trigger a kernel panic"),
        ("reboot", "restart the machine"),
        ("poweroff", "turn the machine off"),
    ];
    out!(out, "Huldra kernel shell. Commands:");
    for (cmd, desc) in COMMANDS {
        out!(out, "  {:<18}{}", cmd, desc);
    }
}

fn uname(args: &[&str], out: &mut Out) {
    if args.first() == Some(&"-a") {
        out!(out, "{} {} {} x86_64", crate::NAME, hostname(), crate::VERSION);
    } else {
        out!(out, "{}", crate::NAME);
    }
}

fn uptime(out: &mut Out) {
    let secs = pit::uptime_ms() / 1000;
    out!(
        out,
        "up {}:{:02}:{:02} ({} ticks at {} Hz)",
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        pit::ticks(),
        pit::HZ
    );
}

fn free(out: &mut Out) {
    let (heap_used, heap_total) = mm::heap::stats();
    let (frames_used, frames_total) = mm::frame::stats();
    let kib = mm::PAGE_SIZE / 1024;
    out!(out, "{:<8}{:>12}{:>12}{:>12}", "", "total", "used", "free");
    out!(
        out,
        "{:<8}{:>9} KiB{:>9} KiB{:>9} KiB",
        "Mem:",
        frames_total * kib,
        frames_used * kib,
        (frames_total - frames_used) * kib
    );
    out!(
        out,
        "{:<8}{:>9} KiB{:>9} KiB{:>9} KiB",
        "Heap:",
        heap_total / 1024,
        heap_used / 1024,
        (heap_total - heap_used) / 1024
    );
}

fn boot_info(out: &mut Out) {
    let (kstart, kend) = mm::frame::kernel_range();
    bootinfo::with(|b| {
        out!(out, "protocol:   {}", b.protocol);
        out!(out, "bootloader: {}", b.bootloader.as_deref().unwrap_or("-"));
        out!(out, "cmdline:    {}", b.cmdline.as_deref().unwrap_or("-"));
        out!(out, "kernel:     {:#x}..{:#x} ({} KiB)", kstart, kend, (kend - kstart) / 1024);
        out!(out, "page table: {:#x}", arch::read_cr3());
        out!(out, "memory map:");
        for r in &b.memory {
            out!(
                out,
                "  {:#012x}..{:#012x} {:>10} KiB  {}",
                r.base,
                r.base + r.len,
                r.len / 1024,
                r.kind_name()
            );
        }
    });
}

fn cpuinfo(out: &mut Out) {
    use core::arch::x86_64::__cpuid;
    let regs = __cpuid;

    let v = regs(0);
    let mut vendor = Vec::new();
    for r in [v.ebx, v.edx, v.ecx] {
        vendor.extend_from_slice(&r.to_le_bytes());
    }
    out!(out, "vendor: {}", String::from_utf8_lossy(&vendor));

    if regs(0x8000_0000).eax >= 0x8000_0004 {
        let mut brand = Vec::new();
        for leaf in 0x8000_0002..=0x8000_0004 {
            let r = regs(leaf);
            for x in [r.eax, r.ebx, r.ecx, r.edx] {
                brand.extend_from_slice(&x.to_le_bytes());
            }
        }
        let brand = String::from_utf8_lossy(&brand);
        out!(out, "model:  {}", brand.trim_matches(|c: char| c == '\0' || c == ' '));
    }

    let f = regs(1);
    out!(
        out,
        "family: {} model: {} stepping: {}",
        (f.eax >> 8) & 0xF,
        (f.eax >> 4) & 0xF,
        f.eax & 0xF
    );
}

fn ls(args: &[&str], out: &mut Out) -> CmdResult {
    let path = args.first().copied().unwrap_or(".");
    let entries = fs::list(path).map_err(fs_err(path))?;
    for e in entries {
        if e.is_dir {
            out!(out, "d {:>8}  {}/", e.size, e.name);
        } else {
            out!(out, "- {:>8}  {}", e.size, e.name);
        }
    }
    Ok(())
}

fn cat(args: &[&str], out: &mut Out) -> CmdResult {
    if args.is_empty() {
        return Err("missing operand".to_string());
    }
    for path in args {
        let data = fs::read(path).map_err(fs_err(path))?;
        let _ = write!(out, "{}", String::from_utf8_lossy(&data));
    }
    Ok(())
}

fn rm(args: &[&str]) -> CmdResult {
    let recursive = args.first() == Some(&"-r");
    let paths = if recursive { &args[1..] } else { args };
    for_each_path(paths, |p| fs::remove(p, recursive))
}

fn for_each_path(paths: &[&str], f: impl Fn(&str) -> Result<(), fs::FsError>) -> CmdResult {
    if paths.is_empty() {
        return Err("missing operand".to_string());
    }
    for path in paths {
        f(path).map_err(fs_err(path))?;
    }
    Ok(())
}

fn syscall_demo(out: &mut Out) {
    let msg = "hello from write(2) via int 0x80\n";
    let written: i64;
    let pid: i64;
    unsafe {
        asm!(
            "int 0x80",
            inlateout("rax") syscall::SYS_WRITE as i64 => written,
            in("rdi") 1u64,
            in("rsi") msg.as_ptr(),
            in("rdx") msg.len(),
        );
        asm!("int 0x80", inlateout("rax") syscall::SYS_GETPID as i64 => pid);
    }
    out!(out, "write returned {}, getpid returned {}", written, pid);
}
