//! mv SOURCE... DEST: move or rename files and directories. Between file
//! systems (say /tmp and the disk) it copies, then removes the source.

#![no_std]
#![no_main]

use huldra_user::abi::fs::{O_CREAT, O_TRUNC, O_WRONLY};
use huldra_user::{env, eprintln, fs, Errno, Result, String};

huldra_user::main!(main);

/// Copies a tree (files, directories, symbolic links), keeping modes.
fn copy(src: &str, dst: &str) -> Result<()> {
    let st = fs::symlink_metadata(src)?;
    if fs::is_symlink(&st) {
        fs::symlink(&fs::read_link(src)?, dst)
    } else if fs::is_dir(&st) {
        match fs::create_dir(dst) {
            Ok(()) | Err(Errno::EEXIST) => {}
            Err(e) => return Err(e),
        }
        for e in fs::read_dir(src)? {
            copy(&fs::join(src, &e.name), &fs::join(dst, &e.name))?;
        }
        Ok(())
    } else {
        let data = fs::read(src)?;
        let f = fs::File::open_with(dst, O_WRONLY | O_CREAT | O_TRUNC, st.st_mode & 0o7777)?;
        f.write_all(&data)
    }
}

fn main() -> i32 {
    let args = &env::args()[1..];
    if args.len() < 2 {
        eprintln!("usage: mv source... dest");
        return 2;
    }
    let (sources, dest) = args.split_at(args.len() - 1);
    let dest = dest[0].as_str();
    let dest_is_dir = fs::metadata(dest).map(|s| fs::is_dir(&s)).unwrap_or(false);
    let mut status = 0;
    for src in sources {
        let target = if dest_is_dir {
            fs::join(dest, src.trim_end_matches('/').rsplit('/').next().unwrap_or(src))
        } else {
            String::from(dest)
        };
        let r = match fs::rename(src, &target) {
            Err(Errno::EXDEV) => copy(src, &target).and_then(|_| fs::remove_all(src)),
            r => r,
        };
        if let Err(e) = r {
            eprintln!("mv: {}: {}", src, e);
            status = 1;
        }
    }
    status
}
