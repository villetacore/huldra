//! rm [-r] [-f] path...: remove files (-r: directories).

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, fs, Errno};

huldra_user::main!(main);

fn main() -> i32 {
    let args = &env::args()[1..];
    let (mut recursive, mut force) = (false, false);
    for a in args.iter().filter(|a| a.starts_with('-')) {
        recursive |= a.contains('r') || a.contains('R');
        force |= a.contains('f');
    }
    let mut status = 0;
    for path in args.iter().filter(|a| !a.starts_with('-')) {
        let r = match fs::metadata(path) {
            Ok(st) if fs::is_dir(&st) && !recursive => Err(Errno::EISDIR),
            Ok(_) if recursive => fs::remove_all(path),
            Ok(_) => fs::remove_file(path),
            Err(e) => Err(e),
        };
        match r {
            Ok(()) => {}
            Err(Errno::ENOENT) if force => {}
            Err(e) => {
                eprintln!("rm: {}: {}", path, e);
                status = 1;
            }
        }
    }
    status
}
