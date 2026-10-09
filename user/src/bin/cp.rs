//! cp [-r] source... dest: copy files (-r: directories).

#![no_std]
#![no_main]

use huldra_user::abi::fs::{O_CREAT, O_TRUNC, O_WRONLY};
use huldra_user::fs::{self, File};
use huldra_user::{env, eprintln, Errno, String, Vec};

huldra_user::main!(main);

fn copy_file(src: &str, dst: &str) -> Result<(), Errno> {
    let input = File::open(src)?;
    let mode = input.stat()?.st_mode & 0o777;
    let output = File::open_with(dst, O_WRONLY | O_CREAT | O_TRUNC, mode)?;
    let mut buf = [0u8; 16384];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        output.write_all(&buf[..n])?;
    }
}

fn copy(src: &str, dst: &str, recursive: bool) -> Result<(), Errno> {
    let st = fs::metadata(src)?;
    if !fs::is_dir(&st) {
        return copy_file(src, dst);
    }
    if !recursive {
        return Err(Errno::EISDIR);
    }
    match fs::create_dir(dst) {
        Ok(()) | Err(Errno::EEXIST) => {}
        Err(e) => return Err(e),
    }
    for e in fs::read_dir(src)? {
        copy(&fs::join(src, &e.name), &fs::join(dst, &e.name), true)?;
    }
    Ok(())
}

fn main() -> i32 {
    let args = &env::args()[1..];
    let recursive = args.iter().any(|a| a == "-r" || a == "-R");
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if paths.len() < 2 {
        eprintln!("usage: cp [-r] source... dest");
        return 2;
    }
    let (sources, dest) = paths.split_at(paths.len() - 1);
    let dest = dest[0].as_str();
    let dest_is_dir = fs::metadata(dest).map(|s| fs::is_dir(&s)).unwrap_or(false);
    let mut status = 0;
    for src in sources {
        let target = if dest_is_dir {
            fs::join(dest, src.rsplit('/').next().unwrap_or(src))
        } else {
            String::from(dest)
        };
        if let Err(e) = copy(src, &target, recursive) {
            eprintln!("cp: {}: {}", src, e);
            status = 1;
        }
    }
    status
}
