//! cc [-o out] [-E] [-I dir] [-D name[=value]] [-run] file.c... [-- args]:
//! compile C programs (hcc).
//!
//! The Huldra C compiler (hcc). Compiles the given sources together with
//! the C library from /usr/lib/hcc into a static executable; there are no
//! object files, so `-c` is not supported. `-run` compiles to a temporary
//! file and runs it with the remaining arguments.

#![no_std]
#![no_main]

use huldra_hcc::{FileSource, Options};
use huldra_user::abi::fs::{O_CREAT, O_TRUNC, O_WRONLY};
use huldra_user::{env, eprintln, fs, print, process, String, ToString, Vec};

huldra_user::main!(main);

struct Disk;

impl FileSource for Disk {
    fn read(&self, path: &str) -> Option<String> {
        fs::read_to_string(path).ok()
    }
}

const USAGE: &str = "usage: cc [-o out] [-E] [-I dir] [-D name[=value]] [-run] file.c... [-- args]";

fn main() -> i32 {
    let args = env::args();
    let mut options = Options::default();
    let mut out = String::from("a.out");
    let mut sources: Vec<String> = Vec::new();
    let mut preprocess_only = false;
    let mut run = false;
    let mut run_args: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        let mut value = |flag: &str| -> Option<String> {
            if a.len() > flag.len() {
                return Some(String::from(&a[flag.len()..]));
            }
            i += 1;
            args.get(i).cloned()
        };
        match a {
            "-o" => match value("-o") {
                Some(v) => out = v,
                None => return usage(),
            },
            "-E" => preprocess_only = true,
            "-run" => run = true,
            "-c" | "-S" => {
                eprintln!("cc: {} is not supported: hcc builds whole programs", a);
                return 1;
            }
            "--" => {
                run_args.extend(args[i + 1..].iter().cloned());
                break;
            }
            "-h" | "--help" => {
                eprintln!("{}", USAGE);
                return 0;
            }
            _ if a.starts_with("-I") => match value("-I") {
                Some(v) => options.include_dirs.insert(0, v),
                None => return usage(),
            },
            _ if a.starts_with("-D") => match value("-D") {
                Some(v) => {
                    let (k, val) = v.split_once('=').unwrap_or((&v, "1"));
                    options.defines.push((k.to_string(), val.to_string()));
                }
                None => return usage(),
            },
            // Accepted for compatibility with gcc command lines.
            _ if a.starts_with("-O")
                || a.starts_with("-W")
                || a.starts_with("-std=")
                || a.starts_with("-l")
                || a.starts_with("-f")
                || a == "-g"
                || a == "-static"
                || a == "-w" => {}
            _ if a.starts_with('-') => {
                eprintln!("cc: unknown option '{}'", a);
                return usage();
            }
            _ if run && !sources.is_empty() && !a.ends_with(".c") => {
                run_args.extend(args[i..].iter().cloned());
                break;
            }
            _ => sources.push(String::from(a)),
        }
        i += 1;
    }
    if sources.is_empty() {
        return usage();
    }
    if preprocess_only {
        for s in &sources {
            match huldra_hcc::preprocess(&Disk, s, &options) {
                Ok(text) => print!("{}", text),
                Err(e) => {
                    eprintln!("{}", e);
                    return 1;
                }
            }
        }
        return 0;
    }
    let refs: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();
    let elf = match huldra_hcc::compile(&Disk, &refs, &options) {
        Ok(elf) => elf,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    if run {
        out = huldra_user::format!("/tmp/cc-run-{}", huldra_user::sys::getpid());
    }
    let written = fs::File::open_with(&out, O_WRONLY | O_CREAT | O_TRUNC, 0o755)
        .and_then(|f| f.write_all(&elf));
    if let Err(e) = written {
        eprintln!("cc: {}: {}", out, e);
        return 1;
    }
    if !run {
        return 0;
    }
    let mut argv: Vec<&str> = alloc_vec(&out);
    argv.extend(run_args.iter().map(|s| s.as_str()));
    let status = process::run(&out, &argv);
    let _ = fs::remove_file(&out);
    match status {
        Ok(s) => s.code(),
        Err(e) => {
            eprintln!("cc: {}: {}", out, e);
            1
        }
    }
}

fn alloc_vec(first: &str) -> Vec<&str> {
    let mut v = Vec::new();
    v.push(first);
    v
}

fn usage() -> i32 {
    eprintln!("{}", USAGE);
    2
}
