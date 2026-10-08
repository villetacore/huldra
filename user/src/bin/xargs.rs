//! xargs [-n N] [-I STR] [command [args...]]: build command lines from stdin.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use huldra_user::io::read_input;
use huldra_user::{env, eprintln, process};

huldra_user::main!(main);

fn run(cmd: &[String]) -> i32 {
    let argv: Vec<&str> = cmd.iter().map(String::as_str).collect();
    match process::find_in_path(argv[0]) {
        Some(p) => process::run(&p, &argv).map_or(126, |s| s.code()),
        None => {
            eprintln!("xargs: {}: command not found", argv[0]);
            127
        }
    }
}

fn main() -> i32 {
    let args = env::args();
    let mut per_call = usize::MAX;
    let mut replace: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-n" => {
                per_call = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(usize::MAX).max(1);
                i += 2;
            }
            "-I" => {
                replace = args.get(i + 1).cloned();
                i += 2;
            }
            _ => break,
        }
    }
    let mut base: Vec<String> = args[i..].to_vec();
    if base.is_empty() {
        base.push(String::from("echo"));
    }
    let input = String::from_utf8_lossy(&read_input("-").unwrap_or_default()).into_owned();
    let mut status = 0;
    if let Some(r) = replace {
        for line in input.lines().filter(|l| !l.is_empty()) {
            let cmd: Vec<String> = base.iter().map(|a| a.replace(r.as_str(), line)).collect();
            status = status.max(run(&cmd));
        }
        return status;
    }
    let items: Vec<&str> = input.split_whitespace().collect();
    if items.is_empty() {
        return 0;
    }
    for chunk in items.chunks(per_call.min(items.len())) {
        let mut cmd = base.clone();
        cmd.extend(chunk.iter().map(|s| String::from(*s)));
        status = status.max(run(&cmd));
    }
    status
}
