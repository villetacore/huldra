#![no_std]
#![no_main]

use huldra_user::abi::fs::*;
use huldra_user::fs::{self, mode_string};
use huldra_user::time::DateTime;
use huldra_user::{env, eprintln, println};

huldra_user::main!(main);

fn kind(mode: u32) -> &'static str {
    match mode & S_IFMT {
        S_IFREG => "regular file",
        S_IFDIR => "directory",
        S_IFCHR => "character special file",
        S_IFBLK => "block special file",
        S_IFIFO => "fifo",
        S_IFLNK => "symbolic link",
        _ => "unknown",
    }
}

fn main() -> i32 {
    let mut status = 0;
    for path in &env::args()[1..] {
        match fs::metadata(path) {
            Ok(st) => {
                let t = DateTime::from_unix(st.st_mtime);
                println!("  File: {}", path);
                println!("  Size: {:<12} Blocks: {:<8} {}", st.st_size, st.st_blocks, kind(st.st_mode));
                println!("Device: {:<12} Inode: {:<9} Links: {}", st.st_dev, st.st_ino, st.st_nlink);
                println!("Access: ({:04o}/{})  Uid: {}  Gid: {}", st.st_mode & 0o7777, mode_string(st.st_mode), st.st_uid, st.st_gid);
                println!(
                    "Modify: {}-{:02}-{:02} {:02}:{:02}:{:02}",
                    t.year, t.month, t.day, t.hour, t.minute, t.second
                );
            }
            Err(e) => {
                eprintln!("stat: {}: {}", path, e);
                status = 1;
            }
        }
    }
    status
}
