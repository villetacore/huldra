//! tar -c|-x|-t [-v] [-C DIR] -f ARCHIVE [paths...]  (ustar, uncompressed)

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use huldra_archive::tar::{Kind, Reader, Writer};
use huldra_user::abi::fs::*;
use huldra_user::io::{read_input, write_all, STDOUT};
use huldra_user::{env, eprintln, fs, println, sys};

huldra_user::main!(main);

fn add(w: &mut Writer, path: &str, name: &str, verbose: bool) -> Result<(), String> {
    let st = fs::metadata(path).map_err(|e| alloc::format!("{}: {}", path, e))?;
    if verbose {
        eprintln!("{}", name);
    }
    if fs::is_dir(&st) {
        w.add_dir(name, st.st_mode & 0o7777, st.st_mtime as u64).map_err(|e| alloc::format!("{}: {:?}", name, e))?;
        for e in fs::read_dir(path).map_err(|e| alloc::format!("{}: {}", path, e))? {
            add(w, &fs::join(path, &e.name), &alloc::format!("{}/{}", name.trim_end_matches('/'), e.name), verbose)?;
        }
    } else {
        let data = fs::read(path).map_err(|e| alloc::format!("{}: {}", path, e))?;
        w.add_file(name, st.st_mode & 0o7777, st.st_mtime as u64, &data).map_err(|e| alloc::format!("{}: {:?}", name, e))?;
    }
    Ok(())
}

fn main() -> i32 {
    let args = env::args();
    let (mut create, mut extract, mut list, mut verbose) = (false, false, false, false);
    let mut archive: Option<String> = None;
    let mut dir: Option<String> = None;
    let mut paths: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if a == "-C" {
            dir = args.get(i + 1).cloned();
            i += 2;
            continue;
        }
        let flags = a.strip_prefix('-').unwrap_or(if i == 1 { a.as_str() } else { "" });
        if !flags.is_empty() && (a.starts_with('-') || i == 1) {
            let mut wants_file = false;
            for c in flags.chars() {
                match c {
                    'c' => create = true,
                    'x' => extract = true,
                    't' => list = true,
                    'v' => verbose = true,
                    'f' => wants_file = true,
                    _ => {}
                }
            }
            if wants_file {
                archive = args.get(i + 1).cloned();
                i += 1;
            }
        } else {
            paths.push(a.clone());
        }
        i += 1;
    }
    let Some(archive) = archive else {
        eprintln!("usage: tar -c|-x|-t [-v] [-C DIR] -f ARCHIVE [paths...]");
        return 2;
    };
    if create {
        let mut w = Writer::new();
        for p in &paths {
            if let Err(e) = add(&mut w, p, p.trim_start_matches('/'), verbose) {
                eprintln!("tar: {}", e);
                return 1;
            }
        }
        let data = w.finish();
        let r = if archive == "-" { write_all(STDOUT, &data) } else { fs::write(&archive, &data) };
        return match r {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("tar: {}: {}", archive, e);
                1
            }
        };
    }
    let data = match read_input(&archive) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("tar: {}: {}", archive, e);
            return 1;
        }
    };
    let base = dir.unwrap_or_else(|| String::from("."));
    for entry in Reader::new(&data) {
        let e = match entry {
            Ok(e) => e,
            Err(err) => {
                eprintln!("tar: corrupt archive: {:?}", err);
                return 1;
            }
        };
        if list {
            if verbose {
                let kind = if e.kind == Kind::Directory { S_IFDIR } else { S_IFREG };
                println!("{} {:>8} {}", fs::mode_string(kind | e.mode), e.data.len(), e.name);
            } else {
                println!("{}", e.name);
            }
            continue;
        }
        if !extract {
            continue;
        }
        if e.name.split('/').any(|c| c == "..") {
            eprintln!("tar: skipping unsafe path {}", e.name);
            continue;
        }
        let target = fs::join(&base, e.name.trim_start_matches('/'));
        if verbose {
            eprintln!("{}", e.name);
        }
        match e.kind {
            Kind::Directory => {
                let _ = fs::create_dir_all(target.trim_end_matches('/'));
            }
            Kind::File => {
                if let Some(i) = target.rfind('/') {
                    let _ = fs::create_dir_all(&target[..i]);
                }
                let r = sys::open(&target, O_WRONLY | O_CREAT | O_TRUNC, e.mode & 0o777).and_then(|fd| {
                    let r = huldra_user::io::write_all(fd, e.data);
                    let _ = sys::close(fd);
                    r
                });
                if let Err(err) = r {
                    eprintln!("tar: {}: {}", target, err);
                }
            }
            _ => eprintln!("tar: {}: unsupported entry type", e.name),
        }
    }
    0
}
