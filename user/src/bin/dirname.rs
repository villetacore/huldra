//! dirname PATH: print PATH without its last component.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, println};

huldra_user::main!(main);

fn main() -> i32 {
    let Some(path) = env::args().get(1) else {
        eprintln!("usage: dirname PATH");
        return 2;
    };
    let trimmed = path.trim_end_matches('/');
    let dir = match trimmed.rfind('/') {
        Some(0) => "/",
        Some(i) => trimmed[..i].trim_end_matches('/'),
        None if path.starts_with('/') => "/",
        None => ".",
    };
    println!("{}", if dir.is_empty() { "/" } else { dir });
    0
}
