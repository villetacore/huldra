//! seq [-s SEP] [FIRST [INCREMENT]] LAST

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, print, String, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let args = env::args();
    let mut sep = String::from("\n");
    let mut nums: Vec<i64> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "-s" {
            sep = args.get(i + 1).cloned().unwrap_or_default();
            i += 2;
            continue;
        }
        match args[i].parse() {
            Ok(n) => nums.push(n),
            Err(_) => {
                eprintln!("seq: invalid number '{}'", args[i]);
                return 1;
            }
        }
        i += 1;
    }
    let (first, step, last) = match nums[..] {
        [l] => (1, 1, l),
        [f, l] => (f, 1, l),
        [f, s, l] => (f, s, l),
        _ => {
            eprintln!("usage: seq [-s SEP] [FIRST [INCREMENT]] LAST");
            return 2;
        }
    };
    if step == 0 {
        return 1;
    }
    let mut v = first;
    let mut first_out = true;
    while (step > 0 && v <= last) || (step < 0 && v >= last) {
        if !first_out {
            print!("{}", sep);
        }
        first_out = false;
        print!("{}", v);
        v += step;
    }
    if !first_out {
        print!("\n");
    }
    0
}
