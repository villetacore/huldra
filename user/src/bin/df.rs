//! df [-h]: file system usage of every mount.

#![no_std]
#![no_main]

use huldra_user::{env, format, fs, println, sys, String};

huldra_user::main!(main);

fn size(kib: u64, human: bool) -> String {
    if !human {
        return format!("{}", kib);
    }
    if kib >= 1024 * 1024 {
        format!("{:.1}G", kib as f64 / 1048576.0)
    } else if kib >= 1024 {
        format!("{:.1}M", kib as f64 / 1024.0)
    } else {
        format!("{}K", kib)
    }
}

fn main() -> i32 {
    let human = env::args().iter().any(|a| a == "-h");
    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    println!("{:<12} {:<6} {:>9} {:>9} {:>9} {:>4}  {}", "Filesystem", "Type", if human { "Size" } else { "1K-blocks" }, "Used", "Avail", "Use%", "Mounted on");
    for line in mounts.lines() {
        let f: huldra_user::Vec<&str> = line.split_whitespace().collect();
        if f.len() < 3 {
            continue;
        }
        let Ok(st) = sys::statfs(f[1]) else { continue };
        let bs = st.f_bsize as u64;
        let total = st.f_blocks * bs / 1024;
        let free = st.f_bfree * bs / 1024;
        let used = total - free;
        let pct = if total > 0 { format!("{}%", used * 100 / total) } else { String::from("-") };
        println!("{:<12} {:<6} {:>9} {:>9} {:>9} {:>4}  {}", f[0], f[2], size(total, human), size(used, human), size(free, human), pct, f[1]);
    }
    0
}
