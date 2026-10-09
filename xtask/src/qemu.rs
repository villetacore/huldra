//! Running the kernel in QEMU, interactively or under test.

use crate::{Artifacts, Options, Result};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Exit code QEMU reports when the kernel writes `code` to isa-debug-exit.
const fn debug_exit_status(code: i32) -> i32 {
    (code << 1) | 1
}
pub const KERNEL_EXIT_SUCCESS: i32 = debug_exit_status(0x10);

fn qemu_binary() -> Result<PathBuf> {
    let exe = if cfg!(windows) {
        "qemu-system-x86_64.exe"
    } else {
        "qemu-system-x86_64"
    };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(exe);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    for dir in [
        r"C:\Program Files\qemu",
        "/usr/bin",
        "/usr/local/bin",
        "/opt/homebrew/bin",
    ] {
        let p = Path::new(dir).join(exe);
        if p.is_file() {
            return Ok(p);
        }
    }
    Err("qemu-system-x86_64 not found (install QEMU or add it to PATH)".into())
}

fn base_command(a: &Artifacts, cmdline: Option<&str>) -> Result<Command> {
    base_command_with_net(a, cmdline, "user,model=e1000")
}

/// `nic` is QEMU's -nic option: user-mode networking (the guest is
/// 10.0.2.15, the host 10.0.2.2, DNS 10.0.2.3).
fn base_command_with_net(a: &Artifacts, cmdline: Option<&str>, nic: &str) -> Result<Command> {
    let mut cmd = Command::new(qemu_binary()?);
    cmd.args(["-nic", nic]);
    cmd.args(["-m", "256M", "-no-reboot", "-kernel"])
        .arg(&a.kernel);
    cmd.args(["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"]);
    if let Some(initrd) = &a.initrd {
        cmd.arg("-initrd").arg(initrd);
    }
    if let Some(disk) = &a.disk {
        cmd.arg("-drive")
            .arg(format!("file={},format=raw,if=ide,index=0", disk.display()));
    }
    if let Some(c) = cmdline {
        cmd.args(["-append", c]);
    }
    Ok(cmd)
}

pub fn run(a: &Artifacts, o: &Options) -> Result {
    // Boot from the disk unless told otherwise.
    let cmdline = o.cmdline.clone().unwrap_or_else(|| String::from(if a.disk.is_some() { "root=/dev/hda" } else { "" }));
    // Forward host port 8080 to the guest's web server.
    let mut cmd = base_command_with_net(a, Some(&cmdline), "user,model=e1000,hostfwd=tcp:127.0.0.1:8080-:80")?;
    cmd.args(["-serial", "stdio"]);
    if o.headless {
        cmd.args(["-display", "none"]);
    }
    if o.gdb {
        cmd.args(["-s", "-S"]);
        println!("waiting for gdb on localhost:1234");
    }
    let status = cmd
        .status()
        .map_err(|e| format!("failed to start QEMU: {e}"))?;
    match status.code() {
        Some(0) | Some(KERNEL_EXIT_SUCCESS) => Ok(()),
        Some(c) => Err(format!("QEMU exited with status {c}")),
        None => Ok(()),
    }
}

fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Option<i32> {
    let start = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status.code().unwrap_or(-1));
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Boots with `ktest` on the command line: the kernel runs its self tests
/// and reports the result through isa-debug-exit.
pub fn kernel_tests(a: &Artifacts) -> Result {
    let mut cmd = base_command(a, Some("ktest"))?;
    cmd.args(["-display", "none", "-serial", "stdio"]);
    cmd.stdout(Stdio::piped()).stderr(Stdio::inherit());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start QEMU: {e}"))?;

    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0 {
                break;
            }
            std::io::stdout().write_all(&buf[..n]).ok();
            out.extend_from_slice(&buf[..n]);
        }
        out
    });

    let status = wait_with_timeout(&mut child, Duration::from_secs(120));
    let _ = reader.join();
    match status {
        Some(KERNEL_EXIT_SUCCESS) => Ok(()),
        Some(code) => Err(format!("kernel tests failed (QEMU status {code})")),
        None => Err("kernel tests timed out".into()),
    }
}

/// Serial console of a QEMU instance, reached over TCP.
struct Console {
    stream: TcpStream,
    buffer: String,
    /// Bytes of a UTF-8 character split between two reads.
    partial: Vec<u8>,
}

/// Decodes `\r`, `\n`, `\t`, `\e` and `\xNN` in test scripts.
fn unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            match b[i + 1] {
                b'r' => out.push(b'\r'),
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'e' => out.push(0x1b),
                b'\\' => out.push(b'\\'),
                b'x' if i + 4 <= b.len() => {
                    let hex = std::str::from_utf8(&b[i + 2..i + 4]).unwrap_or("00");
                    out.push(u8::from_str_radix(hex, 16).unwrap_or(0));
                    i += 4;
                    continue;
                }
                other => out.push(other),
            }
            i += 2;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&n) = chars.peek() {
                    chars.next();
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

impl Console {
    /// Reads until the shell prompt appears; returns what was read.
    fn read_until_prompt(&mut self, timeout: Duration) -> Result<String> {
        let start = Instant::now();
        let mut buf = [0u8; 4096];
        // A prompt only counts once the output has been quiet for a moment:
        // the line editor also reprints the prompt while redrawing.
        let mut seen_prompt_at: Option<Instant> = None;
        loop {
            let clean = strip_ansi(&self.buffer);
            let last_line = clean.rsplit('\n').next().unwrap_or("");
            if last_line.contains("root@") && last_line.ends_with("# ") {
                let since = *seen_prompt_at.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_millis(300) {
                    self.buffer.clear();
                    return Ok(clean);
                }
            } else {
                seen_prompt_at = None;
            }
            if start.elapsed() > timeout {
                return Err(format!(
                    "timed out waiting for prompt; output so far:\n{clean}"
                ));
            }
            match self.stream.read(&mut buf) {
                Ok(0) => return Err(format!("QEMU closed the serial port; output:\n{clean}")),
                Ok(n) => {
                    self.partial.extend_from_slice(&buf[..n]);
                    // Keep an incomplete character for the next read.
                    let keep = match std::str::from_utf8(&self.partial) {
                        Err(e) if e.error_len().is_none() => self.partial.len() - e.valid_up_to(),
                        _ => 0,
                    };
                    let done = self.partial.len() - keep;
                    self.buffer.push_str(&String::from_utf8_lossy(&self.partial[..done]));
                    self.partial.drain(..done);
                    seen_prompt_at = None;
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    fn send_raw(&mut self, bytes: &[u8]) -> Result {
        self.stream.write_all(bytes).map_err(|e| e.to_string())
    }

    fn send_line(&mut self, line: &str) -> Result {
        self.stream
            .write_all(line.as_bytes())
            .map_err(|e| e.to_string())?;
        self.stream.write_all(b"\r").map_err(|e| e.to_string())
    }
}

/// Runs a script of shell commands over the serial port.
///
/// Script format: `> command` sends a line, `< text` expects `text` in its
/// output, `! text` expects it to be absent, `#` starts a comment.
pub fn shell_session(a: &Artifacts, script: &Path) -> Result {
    let script =
        std::fs::read_to_string(script).map_err(|e| format!("{}: {e}", script.display()))?;

    let port = TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map_err(|e| e.to_string())?
        .port();
    let mut cmd = base_command(a, Some("root=/dev/hda"))?;
    cmd.args(["-display", "none"]);
    cmd.arg("-serial")
        .arg(format!("tcp:127.0.0.1:{port},server=on,wait=on"));
    cmd.stdout(Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start QEMU: {e}"))?;

    let start = Instant::now();
    let stream = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(_) if start.elapsed() < Duration::from_secs(10) => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("cannot connect to QEMU serial: {e}"));
            }
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .ok();
    let mut console = Console {
        stream,
        buffer: String::new(),
        partial: Vec::new(),
    };

    let result = (|| -> Result {
        console.read_until_prompt(Duration::from_secs(30))?;
        let mut output = String::new();
        let mut command = String::new();
        let mut failures = Vec::new();
        for (n, line) in script.lines().enumerate() {
            let line = line.trim_end();
            if let Some(c) = line.strip_prefix("> ") {
                command = c.to_string();
                println!("$ {c}");
                console.send_line(c)?;
                let raw = console.read_until_prompt(Duration::from_secs(60))?;
                // Drop the terminal's echo of the command line itself.
                output = raw
                    .split_once('\n')
                    .map_or(String::new(), |(_, rest)| rest.to_string());
            } else if let Some(t) = line.strip_prefix("< ") {
                if !output.contains(t) {
                    failures.push(format!(
                        "line {}: `{command}`: expected {t:?} in:\n{output}",
                        n + 1
                    ));
                }
            } else if let Some(t) = line.strip_prefix("! ") {
                if output.contains(t) {
                    failures.push(format!(
                        "line {}: `{command}`: unexpected {t:?} in:\n{output}",
                        n + 1
                    ));
                }
            } else if let Some(keys) = line.strip_prefix("= ") {
                // Raw keystrokes for full-screen programs (\r, \e, \xNN escapes).
                command = keys.to_string();
                println!("= {keys}");
                console.send_raw(&unescape(keys))?;
                std::thread::sleep(Duration::from_millis(300));
            } else if line == "." {
                // Wait for the shell prompt to come back.
                output = console.read_until_prompt(Duration::from_secs(60))?;
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("\n\n"))
        }
    })();

    let _ = console.send_line("poweroff");
    if wait_with_timeout(&mut child, Duration::from_secs(10)).is_none() {
        println!("(QEMU killed after poweroff timeout)");
    }
    result
}
