#![no_std]
#![no_main]

use huldra_user::time::DateTime;
use huldra_user::{fs, println, time};

huldra_user::main!(main);

fn main() -> i32 {
    let up = fs::read_to_string("/proc/uptime").unwrap_or_default();
    let secs: u64 = up.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let procs = fs::read_dir("/proc").unwrap_or_default().iter().filter(|e| e.name.parse::<u32>().is_ok()).count();
    let now = DateTime::from_unix(time::now());
    println!(
        " {:02}:{:02}:{:02} up {}:{:02}:{:02}, {} processes",
        now.hour,
        now.minute,
        now.second,
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        procs
    );
    0
}
