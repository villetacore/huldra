#![no_std]
#![no_main]

use huldra_user::io::{write_all, STDIN, STDOUT};
use huldra_user::{env, eprintln, sys, Errno};

huldra_user::main!(main);

fn copy(fd: i32) -> Result<(), Errno> {
    let mut buf = [0u8; 8192];
    loop {
        match sys::read(fd, &mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => write_all(STDOUT, &buf[..n])?,
            Err(Errno::EINTR) => continue,
            Err(e) => return Err(e),
        }
    }
}

fn main() -> i32 {
    let args = &env::args()[1..];
    if args.is_empty() {
        return copy(STDIN).map_or(1, |_| 0);
    }
    let mut status = 0;
    for path in args {
        let result = if path == "-" {
            copy(STDIN)
        } else {
            sys::open(path, 0, 0).and_then(|fd| {
                let r = copy(fd);
                let _ = sys::close(fd);
                r
            })
        };
        if let Err(e) = result {
            eprintln!("cat: {}: {}", path, e);
            status = 1;
        }
    }
    status
}
