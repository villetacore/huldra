//! kill [-SIGNAL] pid...

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, signal};

huldra_user::main!(main);

fn main() -> i32 {
    let args = &env::args()[1..];
    let mut sig = signal::SIGTERM;
    let mut targets = args;
    if let Some(s) = args.first().and_then(|a| a.strip_prefix('-')) {
        if s == "l" {
            for n in 1..=31 {
                huldra_user::println!("{:>2} {}", n, signal::name(n));
            }
            return 0;
        }
        match signal::parse(s) {
            Some(n) => sig = n,
            None => {
                eprintln!("kill: unknown signal {}", s);
                return 2;
            }
        }
        targets = &args[1..];
    }
    if targets.is_empty() {
        eprintln!("usage: kill [-SIGNAL] pid...");
        return 2;
    }
    let mut status = 0;
    for t in targets {
        let Ok(pid) = t.parse::<i32>() else {
            eprintln!("kill: bad pid {}", t);
            status = 1;
            continue;
        };
        if let Err(e) = signal::kill(pid, sig) {
            eprintln!("kill: {}: {}", pid, e);
            status = 1;
        }
    }
    status
}
