//! uname [-a|-s|-n|-r|-v|-m]: print system information.

#![no_std]
#![no_main]

use huldra_user::{env, println, sys, Vec};

huldra_user::main!(main);

fn field(b: &[u8]) -> &str {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    core::str::from_utf8(&b[..n]).unwrap_or("?")
}

fn main() -> i32 {
    let Ok(u) = sys::uname() else { return 1 };
    let flags: Vec<char> = env::args()[1..]
        .iter()
        .flat_map(|a| a.trim_start_matches('-').chars())
        .collect();
    let all = flags.contains(&'a');
    let mut parts = Vec::new();
    if all || flags.is_empty() || flags.contains(&'s') {
        parts.push(field(&u.sysname));
    }
    if all || flags.contains(&'n') {
        parts.push(field(&u.nodename));
    }
    if all || flags.contains(&'r') {
        parts.push(field(&u.release));
    }
    if all || flags.contains(&'v') {
        parts.push(field(&u.version));
    }
    if all || flags.contains(&'m') {
        parts.push(field(&u.machine));
    }
    println!("{}", parts.join(" "));
    0
}
