//! ls [-l] [-a] [-1] [path...]: list directory contents.

#![no_std]
#![no_main]

use huldra_user::abi::fs::*;
use huldra_user::fs::{self, mode_string};
use huldra_user::time::{DateTime, MONTHS};
use huldra_user::{env, eprintln, print, println, String, Vec};

huldra_user::main!(main);

struct Options {
    long: bool,
    all: bool,
    one_per_line: bool,
}

fn color(mode: u32) -> &'static str {
    match mode & S_IFMT {
        S_IFDIR => "\x1b[1;34m",
        S_IFCHR | S_IFBLK => "\x1b[1;33m",
        S_IFIFO => "\x1b[33m",
        S_IFLNK => "\x1b[1;36m",
        _ if mode & 0o111 != 0 => "\x1b[1;32m",
        _ => "",
    }
}

fn show(name: &str, full: &str, st: &Stat, o: &Options) {
    let c = color(st.st_mode);
    let reset = if c.is_empty() { "" } else { "\x1b[0m" };
    if o.long {
        let t = DateTime::from_unix(st.st_mtime);
        let size = if st.st_mode & S_IFMT == S_IFCHR || st.st_mode & S_IFMT == S_IFBLK {
            huldra_user::format!("{:>3}, {:>3}", st.st_rdev >> 8, st.st_rdev & 0xFF)
        } else {
            huldra_user::format!("{:>8}", st.st_size)
        };
        let target = if fs::is_symlink(st) {
            huldra_user::format!(" -> {}", fs::read_link(full).unwrap_or_default())
        } else {
            String::new()
        };
        println!(
            "{} {:>2} root root {} {} {:>2} {:02}:{:02} {}{}{}{}",
            mode_string(st.st_mode),
            st.st_nlink,
            size,
            MONTHS[(t.month - 1) as usize],
            t.day,
            t.hour,
            t.minute,
            c,
            name,
            reset,
            target
        );
    } else if o.one_per_line {
        println!("{}{}{}", c, name, reset);
    } else {
        print!("{}{}{}  ", c, name, reset);
    }
}

fn list(path: &str, o: &Options, header: bool) -> bool {
    // `ls -l link` describes the link itself; `ls link` and `ls -l link/`
    // list what it points to.
    let own = o.long && !path.ends_with('/');
    let st = match if own { fs::symlink_metadata(path) } else { fs::metadata(path) } {
        Ok(st) => st,
        Err(e) => {
            eprintln!("ls: {}: {}", path, e);
            return false;
        }
    };
    if !fs::is_dir(&st) {
        show(path, path, &st, o);
        if !o.long && !o.one_per_line {
            println!();
        }
        return true;
    }
    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("ls: {}: {}", path, e);
            return false;
        }
    };
    if header {
        println!("{}:", path);
    }
    let mut names: Vec<String> = Vec::new();
    if o.all {
        names.push(".".into());
        names.push("..".into());
    }
    names.extend(
        entries
            .into_iter()
            .map(|e| e.name)
            .filter(|n| o.all || !n.starts_with('.')),
    );
    for name in &names {
        let full = fs::join(path, name);
        match fs::symlink_metadata(&full) {
            Ok(st) => show(name, &full, &st, o),
            Err(_) => show(name, &full, &Stat::default(), o),
        }
    }
    if !o.long && !o.one_per_line && !names.is_empty() {
        println!();
    }
    true
}

fn main() -> i32 {
    let mut o = Options {
        long: false,
        all: false,
        one_per_line: false,
    };
    let mut paths = Vec::new();
    for a in &env::args()[1..] {
        if let Some(flags) = a.strip_prefix('-').filter(|f| !f.is_empty()) {
            for f in flags.chars() {
                match f {
                    'l' => o.long = true,
                    'a' => o.all = true,
                    '1' => o.one_per_line = true,
                    _ => {
                        eprintln!("ls: unknown option -{}", f);
                        return 2;
                    }
                }
            }
        } else {
            paths.push(a.as_str());
        }
    }
    if paths.is_empty() {
        paths.push(".");
    }
    let header = paths.len() > 1;
    let mut ok = true;
    for p in &paths {
        ok &= list(p, &o, header);
    }
    if ok {
        0
    } else {
        1
    }
}
