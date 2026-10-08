//! Host-side driver for the hcc C compiler: `cargo xtask cc` compiles on
//! the build machine, `cargo xtask cc-test` compiles `tests/cc/*.c` with
//! both hcc and gcc and compares what the programs print (run under WSL on
//! Windows; the executables only use Linux system calls).

use crate::{root, target_dir, Result};
use huldra_hcc::{FileSource, Options};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Serves /usr/include and /usr/lib/hcc from `rootfs/`, everything else
/// from the host file system.
struct HostFs;

impl FileSource for HostFs {
    fn read(&self, path: &str) -> Option<String> {
        let p = if path.starts_with("/usr/include/") || path.starts_with("/usr/lib/hcc/") {
            root().join("rootfs").join(&path[1..])
        } else {
            PathBuf::from(path)
        };
        fs::read_to_string(p).ok()
    }
}

pub fn compile(sources: &[PathBuf], out: &Path) -> Result {
    let names: Vec<String> = sources
        .iter()
        .map(|s| s.display().to_string().replace('\\', "/"))
        .collect();
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let elf =
        huldra_hcc::compile(&HostFs, &refs, &Options::default()).map_err(|e| e.to_string())?;
    fs::write(out, elf).map_err(|e| format!("{}: {e}", out.display()))
}

/// `cargo xtask cc file.c... [-o out]`
pub fn command(args: &[String]) -> Result {
    let mut sources = Vec::new();
    let mut out = PathBuf::from("a.out");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "-o" {
            out = PathBuf::from(it.next().ok_or("-o needs a file name")?);
        } else {
            sources.push(PathBuf::from(a));
        }
    }
    if sources.is_empty() {
        return Err("usage: cargo xtask cc file.c... [-o out]".into());
    }
    compile(&sources, &out)
}

fn to_wsl(p: &Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    let (drive, rest) = s.split_at(1);
    format!("/mnt/{}{}", drive.to_lowercase(), &rest[1..])
}

fn linux(program: &str, args: &[String]) -> std::io::Result<Output> {
    if cfg!(windows) {
        Command::new("wsl")
            .arg("-e")
            .arg(program)
            .args(args)
            .output()
    } else {
        Command::new(program).args(args).output()
    }
}

fn host_path(p: &Path) -> String {
    if cfg!(windows) {
        to_wsl(p)
    } else {
        p.display().to_string()
    }
}

/// Compiles every test with hcc (and gcc as the reference) and compares.
pub fn test() -> Result {
    let dir = root().join("tests").join("cc");
    let out = target_dir().join("cc-tests");
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut sources: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "c"))
        .collect();
    sources.sort();
    let mut have_gcc = true;
    for src in &sources {
        let name = src.file_stem().unwrap().to_string_lossy().into_owned();
        let bin = out.join(&name);
        compile(std::slice::from_ref(src), &bin).map_err(|e| format!("hcc {name}: {e}"))?;
        let got = linux(&host_path(&bin), &[]).map_err(|e| format!("cannot run {name}: {e}"))?;
        if !got.status.success() {
            return Err(format!(
                "{name}: exited with {}\n{}{}",
                got.status,
                String::from_utf8_lossy(&got.stdout),
                String::from_utf8_lossy(&got.stderr)
            ));
        }
        let mut verdict = "ok";
        if have_gcc {
            let reference = out.join(format!("{name}.gcc"));
            let built = linux(
                "gcc",
                &[
                    "-w".into(),
                    "-O1".into(),
                    "-o".into(),
                    host_path(&reference),
                    host_path(src),
                    "-lm".into(),
                ],
            );
            match built {
                Ok(o) if o.status.success() => {
                    let want = linux(&host_path(&reference), &[]).map_err(|e| e.to_string())?;
                    if want.stdout != got.stdout {
                        fs::write(out.join(format!("{name}.hcc.txt")), &got.stdout).ok();
                        fs::write(out.join(format!("{name}.gcc.txt")), &want.stdout).ok();
                        let first = String::from_utf8_lossy(&got.stdout)
                            .lines()
                            .zip(String::from_utf8_lossy(&want.stdout).lines())
                            .find(|(a, b)| a != b)
                            .map(|(a, b)| format!("\n  hcc: {a}\n  gcc: {b}"))
                            .unwrap_or_default();
                        return Err(format!(
                            "{name}: output differs from gcc (see {}){first}",
                            out.display()
                        ));
                    }
                    verdict = "ok (matches gcc)";
                }
                _ => {
                    println!("(no gcc: comparing against gcc skipped)");
                    have_gcc = false;
                }
            }
        }
        println!("cc {name}: {verdict}");
    }
    Ok(())
}
