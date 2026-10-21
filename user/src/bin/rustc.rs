//! rustc [-o out] [-O] [--emit c] [-run] file.rs [-- args]: compile Rust
//! programs (hrc).
//!
//! The Huldra Rust compiler (hrc) compiles a subset of Rust: structs,
//! enums, traits and generics, closures, iterators, `Box<dyn Trait>`,
//! `Vec`, `String`, `HashMap` and the rest of a small standard library in
//! /usr/lib/hrc/std. Modules may live in other files (`mod name;`).
//! The program is translated to C and compiled by hcc into a static
//! executable. `-O` turns off integer overflow checks; `-run` compiles to a
//! temporary file and runs it with the remaining arguments.

#![no_std]
#![no_main]

use huldra_hrc::{FileSource, Options};
use huldra_user::abi::fs::{O_CREAT, O_TRUNC, O_WRONLY};
use huldra_user::{env, eprintln, fs, process, String, Vec};

huldra_user::main!(main);

struct Disk;

impl FileSource for Disk {
    fn read(&self, path: &str) -> Option<String> {
        fs::read_to_string(path).ok()
    }
}

const USAGE: &str = "usage: rustc [-o out] [-O] [--emit c] [-run] file.rs [-- args]";

fn main() -> i32 {
    let args = env::args();
    let mut options = Options::default();
    let mut out: Option<String> = None;
    let mut source: Option<String> = None;
    let mut run = false;
    let mut emit_c = false;
    let mut run_args: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-o" => {
                i += 1;
                match args.get(i) {
                    Some(v) => out = Some(v.clone()),
                    None => return usage(),
                }
            }
            "-O" => options.overflow_checks = false,
            "-run" | "--run" => run = true,
            "--emit" => {
                i += 1;
                match args.get(i).map(|s| s.as_str()) {
                    Some("c") => emit_c = true,
                    Some("link") => {}
                    _ => return usage(),
                }
            }
            "--emit=c" => emit_c = true,
            "-h" | "--help" => {
                eprintln!("{}", USAGE);
                return 0;
            }
            "--" => {
                run_args.extend(args[i + 1..].iter().cloned());
                break;
            }
            // Accepted for compatibility with rustc command lines.
            "--edition" | "--crate-name" | "--crate-type" | "-C" | "-A" | "-W" | "-D" => i += 1,
            _ if a.starts_with("--edition=") || a.starts_with("-C") || a.starts_with("-g") || a.starts_with("-A") || a.starts_with("-W") => {}
            _ if a.starts_with('-') => {
                eprintln!("rustc: unknown option '{}'", a);
                return usage();
            }
            _ if run && source.is_some() => {
                run_args.extend(args[i..].iter().cloned());
                break;
            }
            _ => source = Some(String::from(a)),
        }
        i += 1;
    }
    let Some(source) = source else { return usage() };
    if emit_c {
        return match huldra_hrc::to_c(&Disk, &source, &options) {
            Ok(c) => {
                let name = out.unwrap_or_else(|| stem(&source) + ".c");
                write_file(&name, c.as_bytes(), 0o644)
            }
            Err(e) => {
                eprintln!("{}", e);
                1
            }
        };
    }
    let elf = match huldra_hrc::compile(&Disk, &source, &options) {
        Ok(elf) => elf,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let out = if run {
        huldra_user::format!("/tmp/rustc-run-{}", huldra_user::sys::getpid())
    } else {
        out.unwrap_or_else(|| stem(&source))
    };
    let r = write_file(&out, &elf, 0o755);
    if r != 0 || !run {
        return r;
    }
    let mut argv: Vec<&str> = Vec::new();
    argv.push(&out);
    argv.extend(run_args.iter().map(|s| s.as_str()));
    let status = process::run(&out, &argv);
    let _ = fs::remove_file(&out);
    match status {
        Ok(s) => s.code(),
        Err(e) => {
            eprintln!("rustc: {}: {}", out, e);
            1
        }
    }
}

/// `dir/main.rs` -> `main` (in the current directory, like rustc).
fn stem(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    String::from(base.strip_suffix(".rs").unwrap_or(base))
}

fn write_file(path: &str, data: &[u8], mode: u32) -> i32 {
    match fs::File::open_with(path, O_WRONLY | O_CREAT | O_TRUNC, mode).and_then(|f| f.write_all(data)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("rustc: {}: {}", path, e);
            1
        }
    }
}

fn usage() -> i32 {
    eprintln!("{}", USAGE);
    2
}
