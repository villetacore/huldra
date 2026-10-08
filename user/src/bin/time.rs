//! time COMMAND [args...]: run a command and report the elapsed time.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, process, time, String, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let args = &env::args()[1..];
    if args.is_empty() {
        eprintln!("usage: time COMMAND [args...]");
        return 2;
    }
    let Some(path) = process::find_in_path(&args[0]) else {
        eprintln!("time: {}: command not found", args[0]);
        return 127;
    };
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let start = time::uptime_ms();
    let status = process::run(&path, &argv);
    let ms = time::uptime_ms() - start;
    eprintln!("\nreal\t{}m{}.{:03}s", ms / 60_000, ms / 1000 % 60, ms % 1000);
    status.map_or(127, |s| s.code())
}
