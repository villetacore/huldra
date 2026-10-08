//! sha256sum [-c] [file...]

#![no_std]
#![no_main]

use huldra_archive::sha256::{hex, Sha256};
use huldra_user::io::{Reader, STDIN};
use huldra_user::{env, eprintln, fs, println, sys, Vec};

huldra_user::main!(main);

fn hash_file(path: &str) -> Result<huldra_user::String, huldra_user::Errno> {
    let fd = if path == "-" { STDIN } else { sys::open(path, 0, 0)? };
    let mut h = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = sys::read(fd, &mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    if fd != STDIN {
        let _ = sys::close(fd);
    }
    Ok(hex(&h.finish()))
}

fn main() -> i32 {
    let args = &env::args()[1..];
    let check = args.iter().any(|a| a == "-c");
    let mut files: Vec<&str> = args.iter().filter(|a| *a != "-c").map(|s| s.as_str()).collect();
    if files.is_empty() {
        files.push("-");
    }
    let mut status = 0;
    if check {
        for list in files {
            let text = if list == "-" { huldra_user::String::from_utf8_lossy(&Reader::new(STDIN).read_to_end().unwrap_or_default()).into_owned() } else { fs::read_to_string(list).unwrap_or_default() };
            for line in text.lines() {
                let Some((sum, name)) = line.split_once("  ") else { continue };
                match hash_file(name) {
                    Ok(h) if h == sum => println!("{}: OK", name),
                    Ok(_) => {
                        println!("{}: FAILED", name);
                        status = 1;
                    }
                    Err(e) => {
                        println!("{}: FAILED open ({})", name, e);
                        status = 1;
                    }
                }
            }
        }
        return status;
    }
    for f in files {
        match hash_file(f) {
            Ok(h) => println!("{}  {}", h, f),
            Err(e) => {
                eprintln!("sha256sum: {}: {}", f, e);
                status = 1;
            }
        }
    }
    status
}
