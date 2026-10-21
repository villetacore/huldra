//! Host-side driver for hrc, the Rust compiler of Huldra: `cargo xtask rs`
//! compiles on the build machine, `cargo xtask rs-test` compiles
//! `tests/rs/*.rs` with both hrc and rustc and compares what the programs
//! print (run under WSL on Windows; the executables only use Linux system
//! calls).

use crate::{root, target_dir, Result};
use huldra_hrc::{FileSource, Options};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Serves the system directories from `rootfs/`, everything else from the
/// host file system (relative to the repository root).
struct HostFs;

impl FileSource for HostFs {
    fn read(&self, path: &str) -> Option<String> {
        let p = if path.starts_with("/usr/include/") || path.starts_with("/usr/lib/hcc/") || path.starts_with("/usr/lib/hrc/") {
            root().join("rootfs").join(&path[1..])
        } else {
            root().join(path)
        };
        fs::read_to_string(p).ok()
    }
}

fn rel(p: &Path) -> String {
    let p = p.strip_prefix(root()).unwrap_or(p);
    p.display().to_string().replace('\\', "/")
}

pub fn compile(source: &Path, out: &Path, optimize: bool, emit_c: bool) -> Result {
    let name = rel(source);
    let mut options = Options::default();
    options.overflow_checks = !optimize;
    if std::env::var("HRC_TIME").is_ok() {
        phases(&name, &options)?;
    }
    let t0 = std::time::Instant::now();
    if emit_c || std::env::var("HRC_TIME").is_ok() {
        let c = huldra_hrc::to_c(&HostFs, &name, &options).map_err(|e| e.to_string())?;
        if std::env::var("HRC_TIME").is_ok() {
            println!("front end: {:?}, {} lines of C", t0.elapsed(), c.lines().count());
        }
        fs::write(out.with_extension("c"), c).map_err(|e| e.to_string())?;
    }
    let t1 = std::time::Instant::now();
    let elf = huldra_hrc::compile(&HostFs, &name, &options).map_err(|e| e.to_string())?;
    if std::env::var("HRC_TIME").is_ok() {
        println!("whole compile: {:?}", t1.elapsed());
    }
    fs::write(out, elf).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(())
}

/// `cargo xtask rs file.rs [-o out] [-O] [--emit-c] [--run]`
pub fn command(args: &[String]) -> Result {
    let mut source = None;
    let mut out = None;
    let mut optimize = false;
    let mut emit_c = false;
    let mut run = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => out = Some(PathBuf::from(it.next().ok_or("-o needs a file name")?)),
            "-O" => optimize = true,
            "--emit-c" => emit_c = true,
            "--run" => run = true,
            _ => source = Some(PathBuf::from(a)),
        }
    }
    let source = source.ok_or("usage: cargo xtask rs file.rs [-o out] [-O] [--emit-c] [--run]")?;
    let source = if source.is_absolute() { source } else { std::env::current_dir().map_err(|e| e.to_string())?.join(source) };
    let out = out.unwrap_or_else(|| target_dir().join("rs-out").join(source.file_stem().unwrap()));
    if let Some(d) = out.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    compile(&source, &out, optimize, emit_c)?;
    if run {
        let o = linux(&host_path(&out), &[], None).map_err(|e| e.to_string())?;
        print!("{}", String::from_utf8_lossy(&o.stdout));
        eprint!("{}", String::from_utf8_lossy(&o.stderr));
        println!("(exit {})", o.status.code().unwrap_or(-1));
    }
    Ok(())
}

fn to_wsl(p: &Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    let (drive, rest) = s.split_at(1);
    format!("/mnt/{}{}", drive.to_lowercase(), &rest[1..])
}

fn host_path(p: &Path) -> String {
    if cfg!(windows) {
        to_wsl(p)
    } else {
        p.display().to_string()
    }
}

fn linux(program: &str, args: &[String], stdin: Option<&Path>) -> std::io::Result<Output> {
    // Input goes through a shell redirection: wsl.exe does not pass a
    // redirected stdin on.
    let quote = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
    let mut line = quote(program);
    for a in args {
        line.push(' ');
        line.push_str(&quote(a));
    }
    line.push_str(" < ");
    line.push_str(&match stdin {
        Some(f) => quote(&host_path(f)),
        None => String::from("/dev/null"),
    });
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("wsl");
        c.arg("-e").arg("sh").arg("-c").arg(&line);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(&line);
        c
    };
    cmd.current_dir(root()).stdin(Stdio::null());
    cmd.output()
}

/// Compiles every test with hrc (and rustc as the reference) and compares
/// standard output, standard error and the exit status.
pub fn test(filter: Option<&str>) -> Result {
    let dir = root().join("tests").join("rs");
    let out = target_dir().join("rs-tests");
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut sources: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    sources.sort();
    let mut have_rustc = true;
    let mut failures = Vec::new();
    for src in &sources {
        let name = src.file_stem().unwrap().to_string_lossy().into_owned();
        if let Some(f) = filter {
            if !name.contains(f) {
                continue;
            }
        }
        let input = src.with_extension("in");
        let input = if input.exists() { Some(input) } else { None };
        let bin = out.join(&name);
        if let Err(e) = compile(src, &bin, false, true) {
            println!("rs {name}: FAILED to compile\n{e}");
            failures.push(name);
            continue;
        }
        let got = linux(&host_path(&bin), &[], input.as_deref()).map_err(|e| format!("cannot run {name}: {e}"))?;
        let mut verdict = String::from("ok");
        if have_rustc {
            let reference = out.join(format!("{name}.rustc"));
            let built = linux(
                "rustc",
                &["--edition".into(), "2021".into(), "-A".into(), "warnings".into(), "-o".into(), host_path(&reference), rel(src)],
                None,
            );
            match built {
                Ok(o) if o.status.success() => {
                    let want = linux(&host_path(&reference), &[], input.as_deref()).map_err(|e| e.to_string())?;
                    let mut diff = Vec::new();
                    if want.stdout != got.stdout {
                        diff.push(first_difference("stdout", &got.stdout, &want.stdout));
                    }
                    let (got_err, want_err) = (normalize_tid(&got.stderr), normalize_tid(&want.stderr));
                    if want_err != got_err {
                        diff.push(first_difference("stderr", &got_err, &want_err));
                    }
                    if want.status.code() != got.status.code() {
                        diff.push(format!("  exit: hrc {:?}, rustc {:?}", got.status.code(), want.status.code()));
                    }
                    if diff.is_empty() {
                        verdict = String::from("ok (matches rustc)");
                    } else {
                        fs::write(out.join(format!("{name}.hrc.txt")), &got.stdout).ok();
                        fs::write(out.join(format!("{name}.rustc.txt")), &want.stdout).ok();
                        verdict = format!("DIFFERS from rustc\n{}", diff.join("\n"));
                        failures.push(name.clone());
                    }
                }
                Ok(o) => {
                    println!("rs {name}: rustc rejected the test:\n{}", String::from_utf8_lossy(&o.stderr));
                    failures.push(name.clone());
                    continue;
                }
                Err(_) => {
                    println!("(no rustc: comparing against rustc skipped)");
                    have_rustc = false;
                }
            }
        }
        println!("rs {name}: {verdict}");
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} test(s) failed: {}", failures.len(), failures.join(", ")))
    }
}

/// `thread 'main' (1234) panicked` without the thread id.
fn normalize_tid(b: &[u8]) -> Vec<u8> {
    let s = String::from_utf8_lossy(b);
    let mut out = String::new();
    let mut rest: &str = &s;
    while let Some(i) = rest.find("thread 'main' (") {
        out.push_str(&rest[..i + 14]);
        rest = &rest[i + 14..];
        if let Some(j) = rest.find(") ") {
            if rest[1..j].chars().all(|c| c.is_ascii_digit()) {
                rest = &rest[j + 1..];
            }
        }
    }
    out.push_str(rest);
    out.into_bytes()
}

fn first_difference(what: &str, got: &[u8], want: &[u8]) -> String {
    let g = String::from_utf8_lossy(got);
    let w = String::from_utf8_lossy(want);
    let gl: Vec<&str> = g.lines().collect();
    let wl: Vec<&str> = w.lines().collect();
    for i in 0..gl.len().max(wl.len()) {
        let a = gl.get(i).copied().unwrap_or("<end>");
        let b = wl.get(i).copied().unwrap_or("<end>");
        if a != b {
            return format!("  {what} line {}:\n    hrc:   {a}\n    rustc: {b}", i + 1);
        }
    }
    format!("  {what}: differs (whitespace)")
}

/// `HRC_TIME=1`: how long each stage of the front end takes.
fn phases(name: &str, options: &Options) -> Result {
    use huldra_hrc::{check, cgen, parse, resolve, Sources};
    let e = |e: huldra_hrc::Error| e.to_string();
    let t = std::time::Instant::now();
    let mut sources = Sources { names: Vec::new(), texts: Vec::new() };
    let std_text = HostFs.read(&options.std).ok_or("no std")?;
    let f = sources.add(options.std.clone(), std_text);
    let std_items = parse::parse_crate(&HostFs, &mut sources, f).map_err(e)?;
    let t1 = t.elapsed();
    let text = HostFs.read(name).ok_or("no source")?;
    let f = sources.add(name.to_string(), text);
    let items = parse::parse_crate(&HostFs, &mut sources, f).map_err(e)?;
    let db = resolve::build(std_items, items).map_err(e)?;
    let t2 = t.elapsed();
    let program = check::check_program(&db, &sources, f).map_err(e)?;
    let t3 = t.elapsed();
    let c = cgen::generate(&db, &program, &sources, options);
    let t4 = t.elapsed();
    println!("parse std {:?}, parse+resolve {:?}, check {:?}, cgen {:?} ({} fns, {} bytes of C)", t1, t2 - t1, t3 - t2, t4 - t3, program.fns.len(), c.len());
    Ok(())
}
