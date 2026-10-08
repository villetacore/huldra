//! ps: lists processes from /proc.

#![no_std]
#![no_main]

use huldra_user::{fs, println, String, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let mut pids: Vec<u32> = fs::read_dir("/proc")
        .unwrap_or_default()
        .iter()
        .filter_map(|e| e.name.parse().ok())
        .collect();
    pids.sort_unstable();
    println!(
        "{:>5} {:>5} {:>5} S {:>8} {:>7}  CMD",
        "PID", "PPID", "PGID", "TIME", "VSZ"
    );
    for pid in pids {
        let Ok(stat) = fs::read_to_string(&huldra_user::format!("/proc/{}/stat", pid)) else {
            continue;
        };
        // "pid (name) state ppid pgid sid ticks vm_bytes"
        let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else {
            continue;
        };
        let name = &stat[open + 1..close];
        let rest: Vec<&str> = stat[close + 1..].split_whitespace().collect();
        if rest.len() < 6 {
            continue;
        }
        let cmdline = fs::read(&huldra_user::format!("/proc/{}/cmdline", pid)).unwrap_or_default();
        let mut cmd: String = cmdline
            .split(|&b| b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        if cmd.is_empty() || pid == 0 {
            cmd = huldra_user::format!("[{}]", name);
        }
        let ticks: u64 = rest[4].parse().unwrap_or(0);
        let secs = ticks / 100;
        let vsz: u64 = rest[5].parse::<u64>().unwrap_or(0) / 1024;
        println!(
            "{:>5} {:>5} {:>5} {} {:>5}:{:02} {:>7}  {}",
            pid,
            rest[1],
            rest[2],
            rest[0],
            secs / 60,
            secs % 60,
            vsz,
            cmd
        );
    }
    0
}
