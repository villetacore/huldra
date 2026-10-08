//! tee [-a] file...: copy stdin to stdout and files

#![no_std]
#![no_main]

use huldra_user::abi::fs::*;
use huldra_user::io::{write_all, STDIN, STDOUT};
use huldra_user::{env, eprintln, sys, Errno, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let args = &env::args()[1..];
    let append = args.iter().any(|a| a == "-a");
    let flags = O_WRONLY | O_CREAT | if append { O_APPEND } else { O_TRUNC };
    let mut fds: Vec<i32> = Vec::new();
    for f in args.iter().filter(|a| !a.starts_with('-')) {
        match sys::open(f, flags, 0o644) {
            Ok(fd) => fds.push(fd),
            Err(e) => eprintln!("tee: {}: {}", f, e),
        }
    }
    let mut buf = [0u8; 4096];
    loop {
        let n = match sys::read(STDIN, &mut buf) {
            Ok(0) => return 0,
            Ok(n) => n,
            Err(Errno::EINTR) => continue,
            Err(_) => return 1,
        };
        let _ = write_all(STDOUT, &buf[..n]);
        for &fd in &fds {
            let _ = write_all(fd, &buf[..n]);
        }
    }
}
