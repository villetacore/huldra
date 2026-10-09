//! basename PATH [SUFFIX]: print the last component of PATH, without SUFFIX.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, println};

huldra_user::main!(main);

fn main() -> i32 {
    let args = env::args();
    let Some(path) = args.get(1) else {
        eprintln!("usage: basename PATH [SUFFIX]");
        return 2;
    };
    let trimmed = path.trim_end_matches('/');
    let mut base = if trimmed.is_empty() { "/" } else { trimmed.rsplit('/').next().unwrap_or(trimmed) };
    if let Some(suffix) = args.get(2) {
        if base != suffix.as_str() {
            base = base.strip_suffix(suffix.as_str()).unwrap_or(base);
        }
    }
    println!("{}", base);
    0
}
