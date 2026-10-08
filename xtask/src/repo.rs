//! The package repository: `packages/NAME/` holds a PKGINFO, C sources in
//! `src/` (compiled with hcc into /usr/bin/NAME) and files to install in
//! `files/`. `cargo xtask repo` builds target/repo (INDEX and *.pkg);
//! `serve` makes it available over HTTP, which QEMU guests reach at
//! 10.0.2.2:8800.

use crate::{cc, root, target_dir, Result};
use huldra_pkg::{IndexEntry, PkgFile, PkgInfo};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

pub const PORT: u16 = 8800;
/// Tests use their own server, so a `run` session elsewhere does not matter.
pub const TEST_PORT: u16 = 8801;

fn tree(dir: &Path, prefix: &str, out: &mut Vec<PkgFile>) -> Result {
    let mut entries: Vec<_> = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = format!("{}{}", prefix, e.file_name().to_string_lossy());
        let path = e.path();
        if path.is_dir() {
            out.push(PkgFile { path: format!("{name}/"), mode: 0o755, data: Vec::new() });
            tree(&path, &format!("{name}/"), out)?;
        } else {
            let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            out.push(PkgFile { path: name, mode: 0o644, data });
        }
    }
    Ok(())
}

fn build_one(dir: &Path, out: &Path) -> Result<IndexEntry> {
    let text = fs::read_to_string(dir.join("PKGINFO")).map_err(|e| format!("{}: {e}", dir.display()))?;
    let info = PkgInfo::parse(&text).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files = Vec::new();
    let src = dir.join("src");
    if src.is_dir() && fs::read_dir(&src).map_or(false, |mut d| d.next().is_some()) {
        let mut sources: Vec<PathBuf> = fs::read_dir(&src).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "c")).collect();
        sources.sort();
        let bin = target_dir().join("repo-build").join(&info.name);
        fs::create_dir_all(bin.parent().unwrap()).map_err(|e| e.to_string())?;
        cc::compile(&sources, &bin).map_err(|e| format!("package {}: {e}", info.name))?;
        files.push(PkgFile { path: "usr/".into(), mode: 0o755, data: Vec::new() });
        files.push(PkgFile { path: "usr/bin/".into(), mode: 0o755, data: Vec::new() });
        files.push(PkgFile { path: format!("usr/bin/{}", info.name), mode: 0o755, data: fs::read(&bin).map_err(|e| e.to_string())? });
    }
    if dir.join("files").is_dir() {
        let mut more = Vec::new();
        tree(&dir.join("files"), "", &mut more)?;
        for f in more {
            if !files.iter().any(|g| g.path == f.path) {
                files.push(f);
            }
        }
    }
    let data = huldra_pkg::build_package(&info, &files)?;
    let file = info.file_name();
    fs::write(out.join(&file), &data).map_err(|e| e.to_string())?;
    Ok(IndexEntry { size: data.len() as u64, sha256: huldra_pkg::checksum(&data), file, repo: String::new(), info })
}

/// Builds every package and the index into target/repo.
pub fn build() -> Result<PathBuf> {
    let out = target_dir().join("repo");
    if out.exists() {
        fs::remove_dir_all(&out).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut dirs: Vec<PathBuf> = fs::read_dir(root().join("packages")).map_err(|e| format!("packages/: {e}"))?.flatten().map(|e| e.path()).filter(|p| p.join("PKGINFO").is_file()).collect();
    dirs.sort();
    let mut index = Vec::new();
    for d in dirs {
        index.push(build_one(&d, &out)?);
    }
    fs::write(out.join("INDEX"), huldra_pkg::write_index(&index)).map_err(|e| e.to_string())?;
    println!("repository: {} packages in {}", index.len(), out.display());
    Ok(out)
}

fn handle(mut stream: TcpStream, dir: &Path) {
    let mut line = String::new();
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    if reader.read_line(&mut line).is_err() {
        return;
    }
    // Skip the headers.
    let mut h = String::new();
    while reader.read_line(&mut h).is_ok_and(|n| n > 2) {
        h.clear();
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/").trim_start_matches('/').to_string();
    let file = if path.is_empty() || path.contains("..") || path.contains('\\') { None } else { fs::read(dir.join(&path)).ok() };
    let _ = match file {
        Some(body) => stream.write_all(format!("HTTP/1.0 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).and_then(|_| stream.write_all(&body)),
        None => stream.write_all(b"HTTP/1.0 404 Not Found\r\nContent-Length: 10\r\nConnection: close\r\n\r\nnot found\n"),
    };
}

/// Serves `dir` on 127.0.0.1:`port` from a background thread. Returns
/// false if the port is taken (another xtask already serves it).
pub fn serve_background(dir: PathBuf, port: u16) -> bool {
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(_) => return false,
    };
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let dir = dir.clone();
            std::thread::spawn(move || handle(stream, &dir));
        }
    });
    true
}

/// `cargo xtask serve`: builds the repository and serves it until killed.
pub fn serve() -> Result {
    let dir = build()?;
    if !serve_background(dir, PORT) {
        return Err(format!("port {PORT} is in use"));
    }
    println!("serving the package repository on http://127.0.0.1:{PORT} (10.0.2.2:{PORT} in QEMU); Ctrl+C stops");
    loop {
        std::thread::park();
    }
}
