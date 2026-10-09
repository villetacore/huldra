//! Web pieces for the image and the tests: the host's CA bundle (copied
//! into the system as /etc/ssl/certs/ca-certificates.crt), and the test
//! web server (`tests/net/server.py`, HTTP and HTTPS) that tests/net.txt
//! talks to.

use crate::image::ImageFile;
use crate::{root, target_dir, Result};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

pub const HTTP_PORT: u16 = 8810;
pub const HTTPS_PORT: u16 = 8811;

/// The host's trusted certificates (Mozilla's set, as distributions ship
/// it). `HULDRA_CA_BUNDLE` overrides the search.
pub fn ca_bundle() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("HULDRA_CA_BUNDLE") {
        candidates.push(p.into());
    }
    for p in [
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/tls/certs/ca-bundle.crt",
        "/etc/ssl/cert.pem",
        "C:/Program Files/Git/mingw64/etc/ssl/certs/ca-bundle.crt",
        "C:/Program Files/Git/usr/ssl/certs/ca-bundle.crt",
    ] {
        candidates.push(p.into());
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn ca_files() -> Vec<ImageFile> {
    match ca_bundle() {
        Some(source) => vec![ImageFile { dest: "etc/ssl/certs/ca-certificates.crt".into(), source, mode: 0o644 }],
        None => {
            println!("(no CA bundle found on this machine: HTTPS in the system will not trust any server; set HULDRA_CA_BUNDLE)");
            Vec::new()
        }
    }
}

/// Only for test disks: the root of libs/tls/testdata, whose private key
/// is public. A real system must never trust it.
pub fn test_root() -> ImageFile {
    ImageFile { dest: "etc/ssl/certs/huldra-test-root.pem".into(), source: root().join("libs/tls/testdata/root.pem"), mode: 0o644 }
}

fn python() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|p| Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()))
}

/// The running test server; stopped on drop.
pub struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn git(dir: &std::path::Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(["-c", "init.defaultBranch=main", "-c", "core.autocrlf=false", "-c", "user.name=Host", "-c", "user.email=host@test"])
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A bare repository with two commits for tests/net.txt to clone and
/// push to: target/git-server/test.git.
pub fn prepare_git_repo() -> Option<PathBuf> {
    let root = target_dir().join("git-server");
    let _ = std::fs::remove_dir_all(&root);
    let work = root.join("work");
    let made = (|| -> Result<()> {
        std::fs::create_dir_all(work.join("src")).map_err(|e| e.to_string())?;
        git(&work, &["init", "-q", "."])?;
        std::fs::write(work.join("README.md"), "# test repository\n\nserved by the host for Huldra's tests\n").map_err(|e| e.to_string())?;
        git(&work, &["add", "."])?;
        git(&work, &["commit", "-q", "-m", "first commit"])?;
        std::fs::write(work.join("src/main.c"), "int main(void) { return 0; }\n").map_err(|e| e.to_string())?;
        git(&work, &["add", "."])?;
        git(&work, &["commit", "-q", "-m", "add main.c"])?;
        git(&root, &["clone", "-q", "--bare", "work", "test.git"])?;
        git(&root.join("test.git"), &["config", "http.receivepack", "true"])?;
        Ok(())
    })();
    match made {
        Ok(()) => Some(root),
        Err(e) => {
            println!("(no git on this machine: skipping the git tests: {e})");
            None
        }
    }
}

/// After tests/net.txt: real git must accept what Huldra pushed.
pub fn check_git_push(root: &std::path::Path) -> Result {
    let bare = root.join("test.git");
    git(&bare, &["fsck", "--strict", "--no-dangling"])?;
    let log = git(&bare, &["log", "--format=%s|%an", "main"])?;
    if !log.lines().next().is_some_and(|l| l == "edited on huldra|Huldra Tester") {
        return Err(format!("the commit pushed from Huldra is not on main:\n{log}"));
    }
    let file = git(&bare, &["show", "main:hello.txt"])?;
    if file != "hello from huldra\n" {
        return Err(format!("hello.txt pushed from Huldra has unexpected content: {file:?}"));
    }
    println!("git fsck --strict: ok; pushed commit found on main");
    Ok(())
}

/// Starts tests/net/server.py, or returns None (with a note) if Python is
/// missing or the ports are taken.
pub fn start_server(git_root: Option<&std::path::Path>) -> Option<Server> {
    let Some(py) = python() else {
        println!("(no Python: skipping the network tests)");
        return None;
    };
    let data = root().join("libs/tls/testdata");
    let mut child = Command::new(py)
        .arg(root().join("tests/net/server.py"))
        .arg(HTTP_PORT.to_string())
        .arg(HTTPS_PORT.to_string())
        .arg(data.join("server.pem"))
        .arg(data.join("server.key"))
        .args(git_root.map(|r| r.as_os_str().to_owned()))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut line = String::new();
    let ok = BufReader::new(child.stdout.take()?).read_line(&mut line).is_ok() && line.trim() == "ready";
    if !ok {
        let _ = child.kill();
        println!("(test web server did not start: ports {HTTP_PORT}/{HTTPS_PORT} busy?)");
        return None;
    }
    Some(Server(child))
}
