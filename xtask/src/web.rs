//! Web pieces for the image and the tests: the host's CA bundle (copied
//! into the system as /etc/ssl/certs/ca-certificates.crt), and the test
//! web server (`tests/net/server.py`, HTTP and HTTPS) that tests/net.txt
//! talks to.

use crate::image::ImageFile;
use crate::root;
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

/// Starts tests/net/server.py, or returns None (with a note) if Python is
/// missing or the ports are taken.
pub fn start_server() -> Option<Server> {
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
