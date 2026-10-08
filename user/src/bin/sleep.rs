//! sleep SECONDS (fractions allowed: sleep 0.5)

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, time};

huldra_user::main!(main);

fn main() -> i32 {
    let Some(arg) = env::args().get(1) else {
        eprintln!("usage: sleep SECONDS");
        return 2;
    };
    let (whole, frac) = arg.split_once('.').unwrap_or((arg, ""));
    let secs: u64 = whole.parse().unwrap_or(0);
    let mut frac_ms = 0u64;
    for (i, c) in frac.chars().take(3).enumerate() {
        frac_ms += c.to_digit(10).unwrap_or(0) as u64 * [100, 10, 1][i];
    }
    time::sleep_ms(secs * 1000 + frac_ms);
    0
}
