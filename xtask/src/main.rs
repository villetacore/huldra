//! Build tool for Huldra: `cargo xtask <command>`.
//!
//! Builds the kernel and user space for the bare-metal target, packs the
//! initrd and disk image, runs QEMU and drives automated tests.

mod image;
mod qemu;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

pub const TARGET: &str = "x86_64-unknown-none";

#[derive(Default)]
pub struct Options {
    pub release: bool,
    pub headless: bool,
    pub gdb: bool,
    pub cmdline: Option<String>,
}

pub struct Artifacts {
    pub kernel: PathBuf,
    pub initrd: Option<PathBuf>,
    pub disk: Option<PathBuf>,
}

type Result<T = ()> = std::result::Result<T, String>;

const USAGE: &str = "\
usage: cargo xtask <command> [options]

commands:
  build        build kernel, user space, initrd and disk image
  run          build and boot in QEMU (serial on this terminal)
  test         host unit tests + in-kernel tests + scripted shell session
  iso          build a GRUB ISO (needs grub-mkrescue, e.g. in WSL)

options:
  --release    optimized build
  --headless   no QEMU window, serial only
  --gdb        wait for gdb on localhost:1234
  --append S   kernel command line
";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(command) = args.first() else {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    };
    let options = match parse_options(&args[1..]) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    let result = match command.as_str() {
        "build" => build(&options).map(|a| {
            println!("kernel: {}", a.kernel.display());
            if let Some(i) = a.initrd {
                println!("initrd: {}", i.display());
            }
            if let Some(d) = a.disk {
                println!("disk:   {}", d.display());
            }
        }),
        "run" => build(&options).and_then(|a| qemu::run(&a, &options)),
        "test" => test(&options),
        "iso" => iso(&options),
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command '{other}'\n\n{USAGE}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse_options(args: &[String]) -> Result<Options> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--release" | "-r" => o.release = true,
            "--headless" => o.headless = true,
            "--gdb" => o.gdb = true,
            "--append" => o.cmdline = Some(it.next().ok_or("--append needs a value")?.clone()),
            other => return Err(format!("unknown option '{other}'")),
        }
    }
    Ok(o)
}

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn profile_dir(options: &Options) -> &'static str {
    if options.release {
        "release"
    } else {
        "debug"
    }
}

fn target_dir() -> PathBuf {
    root().join("target")
}

fn cargo(args: &[&str], options: &Options) -> Result {
    let mut cmd = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(root()).args(args);
    if options.release {
        cmd.arg("--release");
    }
    let status = cmd.status().map_err(|e| format!("failed to run cargo: {e}"))?;
    if !status.success() {
        return Err(format!("cargo {} failed", args.join(" ")));
    }
    Ok(())
}

/// Programs installed in /sbin instead of /bin.
const SBIN: &[&str] = &["init", "mount", "umount", "reboot", "poweroff"];

fn build_user() -> Result<Vec<image::ImageFile>> {
    let mut cmd = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(root()).args(["build", "-p", "huldra-user", "--bins", "--target", TARGET, "--profile", "user"]);
    if !cmd.status().map_err(|e| e.to_string())?.success() {
        return Err("building user space failed".into());
    }
    let out = target_dir().join(TARGET).join("user");
    let mut files = Vec::new();
    let mut names: Vec<String> = fs::read_dir(root().join("user").join("src").join("bin"))
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".rs").map(String::from))
        .collect();
    names.sort();
    for name in names {
        let dir = if SBIN.contains(&name.as_str()) { "sbin" } else { "bin" };
        files.push(image::ImageFile { dest: format!("{dir}/{name}"), source: out.join(&name), mode: 0o755 });
    }
    Ok(files)
}

pub fn build(options: &Options) -> Result<Artifacts> {
    let out = target_dir().join(TARGET).join(profile_dir(options));
    let mut files = image::collect_tree(&root().join("rootfs"))?;
    files.extend(build_user()?);
    let initrd = target_dir().join("initrd.cpio");
    image::write_initrd(&files, &initrd)?;

    cargo(&["build", "-p", "huldra-kernel", "--target", TARGET], options)?;
    Ok(Artifacts { kernel: out.join("huldra"), initrd: Some(initrd), disk: None })
}

fn test(options: &Options) -> Result {
    let libs = lib_crates()?;
    if !libs.is_empty() {
        println!("==> host unit tests");
        let mut args = vec!["test"];
        for l in &libs {
            args.push("-p");
            args.push(l);
        }
        cargo(&args, options)?;
    }

    let artifacts = build(options)?;
    println!("==> in-kernel tests");
    qemu::kernel_tests(&artifacts)?;
    println!("==> scripted shell session");
    qemu::shell_session(&artifacts, &root().join("tests").join("shell.txt"))?;
    println!("all tests passed");
    Ok(())
}

fn lib_crates() -> Result<Vec<String>> {
    let mut names = Vec::new();
    let Ok(entries) = fs::read_dir(root().join("libs")) else {
        return Ok(names);
    };
    for e in entries.flatten() {
        let manifest = e.path().join("Cargo.toml");
        if let Ok(text) = fs::read_to_string(&manifest) {
            if let Some(name) = text
                .lines()
                .find_map(|l| l.trim().strip_prefix("name = ").map(|n| n.trim_matches('"').to_string()))
            {
                names.push(name);
            }
        }
    }
    names.sort();
    Ok(names)
}

fn iso(options: &Options) -> Result {
    let a = build(options)?;
    let iso_root = target_dir().join("iso");
    let grub = iso_root.join("boot").join("grub");
    fs::create_dir_all(&grub).map_err(|e| e.to_string())?;
    fs::copy(&a.kernel, iso_root.join("boot").join("huldra")).map_err(|e| e.to_string())?;
    let mut cfg = String::from("set timeout=0\nset default=0\n\nmenuentry \"Huldra\" {\n    multiboot2 /boot/huldra\n");
    if let Some(initrd) = &a.initrd {
        fs::copy(initrd, iso_root.join("boot").join("initrd.cpio")).map_err(|e| e.to_string())?;
        cfg.push_str("    module2 /boot/initrd.cpio initrd\n");
    }
    cfg.push_str("    boot\n}\n");
    fs::write(grub.join("grub.cfg"), cfg).map_err(|e| e.to_string())?;

    let iso = target_dir().join("huldra.iso");
    let (program, args): (&str, Vec<String>) = if cfg!(windows) {
        let wsl_path = |p: &Path| -> String {
            let s = p.display().to_string().replace('\\', "/");
            let (drive, rest) = s.split_at(1);
            format!("/mnt/{}{}", drive.to_lowercase(), &rest[1..])
        };
        ("wsl", vec!["grub-mkrescue".into(), "-o".into(), wsl_path(&iso), wsl_path(&iso_root)])
    } else {
        ("grub-mkrescue", vec!["-o".into(), iso.display().to_string(), iso_root.display().to_string()])
    };
    let status = Command::new(program).args(&args).status().map_err(|e| format!("{program}: {e}"))?;
    if !status.success() {
        return Err("grub-mkrescue failed (needs grub-pc-bin, grub-common, xorriso, mtools)".into());
    }
    println!("iso: {}", iso.display());
    Ok(())
}
