#![no_std]
#![no_main]

use huldra_user::{fs, println};

huldra_user::main!(main);

fn field(text: &str, key: &str) -> u64 {
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|v| v.trim().split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

fn main() -> i32 {
    let Ok(info) = fs::read_to_string("/proc/meminfo") else { return 1 };
    let total = field(&info, "MemTotal:");
    let free = field(&info, "MemFree:");
    let heap = field(&info, "KernelHeap:");
    println!("{:<8}{:>12}{:>12}{:>12}", "", "total", "used", "free");
    println!("{:<8}{:>9} kB{:>9} kB{:>9} kB", "Mem:", total, total - free, free);
    println!("{:<8}{:>12}{:>9} kB", "Kernel:", "", heap);
    0
}
