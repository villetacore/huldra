//! top: live process monitor. Keys: q quit, k kill a process, +/- speed.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use huldra_user::term::{self, Key, Keys, RawMode, Screen, Style};
use huldra_user::{eprintln, format, fs, signal, time};

huldra_user::main!(main);

struct Proc {
    pid: u32,
    ppid: u32,
    state: String,
    ticks: u64,
    vsz: u64,
    name: String,
    cmd: String,
}

fn read_procs() -> Vec<Proc> {
    let mut out = Vec::new();
    for e in fs::read_dir("/proc").unwrap_or_default() {
        let Ok(pid) = e.name.parse::<u32>() else { continue };
        let Ok(stat) = fs::read_to_string(&format!("/proc/{}/stat", pid)) else { continue };
        let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else { continue };
        let rest: Vec<&str> = stat[close + 1..].split_whitespace().collect();
        if rest.len() < 6 {
            continue;
        }
        let cmdline = fs::read(&format!("/proc/{}/cmdline", pid)).unwrap_or_default();
        let cmd: Vec<String> = cmdline
            .split(|&b| b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        out.push(Proc {
            pid,
            ppid: rest[1].parse().unwrap_or(0),
            state: String::from(rest[0]),
            ticks: rest[4].parse().unwrap_or(0),
            vsz: rest[5].parse().unwrap_or(0),
            name: String::from(&stat[open + 1..close]),
            cmd: cmd.join(" "),
        });
    }
    out
}

fn meminfo() -> (u64, u64) {
    let text = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let get = |k: &str| -> u64 {
        text.lines()
            .find_map(|l| l.strip_prefix(k))
            .and_then(|v| v.split_whitespace().next()?.parse().ok())
            .unwrap_or(0)
    };
    (get("MemTotal:"), get("MemFree:"))
}

fn main() -> i32 {
    if !term::is_tty(0) {
        eprintln!("top: needs a terminal");
        return 1;
    }
    let Ok(_raw) = RawMode::enable() else { return 1 };
    let mut screen = Screen::new();
    let mut keys = Keys::new();
    let mut prev: BTreeMap<u32, u64> = BTreeMap::new();
    let mut prev_time = time::uptime_ms();
    let mut prev_idle = 0.0f32;
    let mut interval = 1000;
    let mut message = String::new();
    loop {
        let now = time::uptime_ms();
        let elapsed_ticks = ((now - prev_time) / 10).max(1);
        prev_time = now;
        let procs = read_procs();
        let mut rows: Vec<(f32, &Proc)> = procs
            .iter()
            .map(|p| {
                let d = p.ticks.saturating_sub(*prev.get(&p.pid).unwrap_or(&p.ticks));
                ((d as f32 * 100.0 / elapsed_ticks as f32).min(100.0), p)
            })
            .collect();
        // Idle time from /proc/uptime (seconds of idle since boot).
        let idle_now = fs::read_to_string("/proc/uptime").ok().and_then(|u| u.split_whitespace().nth(1).and_then(|v| v.parse::<f32>().ok())).unwrap_or(0.0);
        let idle = if prev_idle > 0.0 { ((idle_now - prev_idle) * 10_000.0 / elapsed_ticks as f32).clamp(0.0, 100.0) } else { 0.0 };
        prev_idle = idle_now;
        rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(core::cmp::Ordering::Equal).then(a.1.pid.cmp(&b.1.pid)));
        prev = procs.iter().map(|p| (p.pid, p.ticks)).collect();

        screen.clear();
        let up = now / 1000;
        let (total, free) = meminfo();
        let header = Style::fg(term::CYAN | term::BRIGHT);
        screen.text(0, 0, &format!("top - up {}:{:02}:{:02}, {} tasks", up / 3600, up / 60 % 60, up % 60, procs.len()), header);
        screen.text(1, 0, &format!("CPU: {:5.1}% busy, {:5.1}% idle", 100.0 - idle, idle), Style::NORMAL);
        let used = total - free;
        let bar_width = 30;
        let filled = if total > 0 { (used * bar_width / total) as usize } else { 0 };
        let bar: String = core::iter::repeat_n('█', filled).chain(core::iter::repeat_n('░', bar_width as usize - filled)).collect();
        screen.text(2, 0, &format!("Mem: {} / {} KiB  {}", used, total, bar), Style::NORMAL);
        screen.text(3, 0, &message, Style::fg(term::YELLOW | term::BRIGHT));
        let head = format!("{:>5} {:>5} S {:>5} {:>8} {:>7}  COMMAND", "PID", "PPID", "%CPU", "TIME", "VSZ");
        screen.fill_row(4, 0, Style::REVERSE);
        screen.text(4, 0, &head, Style::REVERSE);
        for (i, (cpu, p)) in rows.iter().enumerate() {
            let row = 5 + i;
            if row >= screen.rows {
                break;
            }
            let secs = p.ticks / 100;
            let cmd = if p.cmd.is_empty() || p.pid == 0 { format!("[{}]", p.name) } else { p.cmd.clone() };
            let line = format!("{:>5} {:>5} {} {:>5.1} {:>5}:{:02} {:>7}  {}", p.pid, p.ppid, p.state, cpu, secs / 60, secs % 60, p.vsz / 1024, cmd);
            screen.text(row, 0, &line, if *cpu > 0.0 && p.pid != 0 { Style::fg(term::GREEN | term::BRIGHT) } else { Style::NORMAL });
        }
        screen.set_cursor(3, message.len());
        screen.present();

        match keys.read_timeout(interval) {
            Ok(Some(Key::Char('q'))) | Ok(Some(Key::Escape)) | Ok(Some(Key::Ctrl('c'))) | Err(_) => break,
            Ok(Some(Key::Char('+'))) => interval = (interval - 250).max(250),
            Ok(Some(Key::Char('-'))) => interval = (interval + 250).min(5000),
            Ok(Some(Key::Char('k'))) => {
                // Read a pid on the message line.
                let mut input = String::new();
                loop {
                    message = format!("PID to kill: {}", input);
                    screen.text(3, 0, &format!("{:<60}", message), Style::fg(term::YELLOW | term::BRIGHT));
                    screen.set_cursor(3, message.len());
                    screen.present();
                    match keys.read() {
                        Ok(Key::Char(c)) if c.is_ascii_digit() => input.push(c),
                        Ok(Key::Backspace) => {
                            input.pop();
                        }
                        Ok(Key::Enter) => break,
                        _ => {
                            input.clear();
                            break;
                        }
                    }
                }
                message = match input.parse::<i32>() {
                    Ok(pid) => match signal::kill(pid, signal::SIGTERM) {
                        Ok(()) => format!("Sent SIGTERM to {}", pid),
                        Err(e) => format!("kill {}: {}", pid, e),
                    },
                    Err(_) => String::new(),
                };
            }
            _ => {}
        }
    }
    term::reset_screen();
    0
}
