//! Huldra packages, shared by the `pkg` tool and the repository builder.
//!
//! A package (`NAME-VERSION.pkg`) is a ustar archive: a `.PKGINFO` file
//! followed by its files, with paths relative to the package's prefix
//! (`bin/hello`, `share/fortune/fortunes`). pkg unpacks it into its own
//! store directory and links it into the system profile, so programs find
//! their files under `/pkg/system/sw` (see [`system`]).
//! A repository is a directory served over HTTP with an `INDEX` listing
//! every package with its size and SHA-256. Both use the same
//! `key = value` stanza format:
//!
//! ```text
//! name = cowsay
//! version = 1.0-1
//! description = A talking cow
//! depends = fortune
//! ```

#![no_std]

extern crate alloc;

pub mod system;

use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::Ordering;
use huldra_archive::sha256;
use huldra_archive::tar::{self, Kind};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PkgInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub depends: Vec<String>,
}

/// A package as listed in a repository index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexEntry {
    pub info: PkgInfo,
    pub file: String,
    pub size: u64,
    pub sha256: String,
    /// Base URL of the repository it came from (added by `pkg update`).
    pub repo: String,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_+.".contains(c)) && !name.starts_with('.')
}

/// Splits stanza text into blocks of (key, value) pairs.
fn stanzas(text: &str) -> Vec<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut cur: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !cur.is_empty() {
                out.push(core::mem::take(&mut cur));
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            cur.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

impl PkgInfo {
    fn from_pairs(pairs: &[(String, String)]) -> Result<PkgInfo, String> {
        let get = |k: &str| pairs.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        let name = get("name").ok_or("missing name")?;
        if !valid_name(&name) {
            return Err(format!("invalid package name '{}'", name));
        }
        let version = get("version").ok_or("missing version")?;
        Ok(PkgInfo {
            name,
            version,
            description: get("description").unwrap_or_default(),
            depends: get("depends").map(|d| d.split_whitespace().map(String::from).collect()).unwrap_or_default(),
        })
    }

    pub fn parse(text: &str) -> Result<PkgInfo, String> {
        let s = stanzas(text);
        PkgInfo::from_pairs(s.first().ok_or("empty package info")?)
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("name = {}\nversion = {}\n", self.name, self.version);
        if !self.description.is_empty() {
            s.push_str(&format!("description = {}\n", self.description));
        }
        if !self.depends.is_empty() {
            s.push_str(&format!("depends = {}\n", self.depends.join(" ")));
        }
        s
    }

    pub fn file_name(&self) -> String {
        format!("{}-{}.pkg", self.name, self.version)
    }
}

pub fn parse_index(text: &str) -> Result<Vec<IndexEntry>, String> {
    stanzas(text)
        .iter()
        .map(|pairs| {
            let get = |k: &str| pairs.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
            let info = PkgInfo::from_pairs(pairs)?;
            Ok(IndexEntry {
                file: get("file").unwrap_or_else(|| info.file_name()),
                size: get("size").and_then(|s| s.parse().ok()).unwrap_or(0),
                sha256: get("sha256").unwrap_or_default(),
                repo: get("repo").unwrap_or_default(),
                info,
            })
        })
        .collect()
}

pub fn write_index(entries: &[IndexEntry]) -> String {
    let mut s = String::new();
    for e in entries {
        s.push_str(&e.info.to_text());
        s.push_str(&format!("file = {}\nsize = {}\nsha256 = {}\n", e.file, e.size, e.sha256));
        if !e.repo.is_empty() {
            s.push_str(&format!("repo = {}\n", e.repo));
        }
        s.push('\n');
    }
    s
}

/// Compares versions like `1.10-2` and `1.9-3`: digit runs numerically,
/// everything else character by character.
pub fn version_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na = a.iter().take_while(|c| c.is_ascii_digit()).count();
                let nb = b.iter().take_while(|c| c.is_ascii_digit()).count();
                let (da, db) = (trim_zeros(&a[..na]), trim_zeros(&b[..nb]));
                let o = da.len().cmp(&db.len()).then_with(|| da.cmp(db));
                if o != Ordering::Equal {
                    return o;
                }
                a = &a[na..];
                b = &b[nb..];
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(y);
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

fn trim_zeros(d: &[u8]) -> &[u8] {
    let n = d.iter().take_while(|&&c| c == b'0').count();
    &d[n.min(d.len().saturating_sub(1))..]
}

/// The newest entry named `name`.
pub fn find<'a>(index: &'a [IndexEntry], name: &str) -> Option<&'a IndexEntry> {
    index.iter().filter(|e| e.info.name == name).max_by(|a, b| version_cmp(&a.info.version, &b.info.version))
}

/// Packages to install for `targets`, dependencies first, skipping
/// installed ones (targets themselves are always included).
pub fn resolve<'a>(targets: &[&str], index: &'a [IndexEntry], installed: &dyn Fn(&str) -> bool) -> Result<Vec<&'a IndexEntry>, String> {
    fn visit<'a>(name: &str, index: &'a [IndexEntry], installed: &dyn Fn(&str) -> bool, target: bool, path: &mut Vec<String>, done: &mut BTreeSet<String>, out: &mut Vec<&'a IndexEntry>) -> Result<(), String> {
        if done.contains(name) || (!target && installed(name)) {
            return Ok(());
        }
        if path.iter().any(|p| p == name) {
            return Err(format!("dependency cycle: {} -> {}", path.join(" -> "), name));
        }
        let e = find(index, name).ok_or_else(|| match path.last() {
            Some(parent) => format!("{}: dependency '{}' not found", parent, name),
            None => format!("package '{}' not found", name),
        })?;
        path.push(name.to_string());
        for d in &e.info.depends {
            visit(d, index, installed, false, path, done, out)?;
        }
        path.pop();
        done.insert(name.to_string());
        out.push(e);
        Ok(())
    }
    let mut out = Vec::new();
    let mut done = BTreeSet::new();
    for t in targets {
        visit(t, index, installed, true, &mut Vec::new(), &mut done, &mut out)?;
    }
    Ok(out)
}

/// A file or directory in a package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PkgFile {
    /// Relative path without a leading `/`; directories end with `/`.
    pub path: String,
    pub mode: u32,
    pub data: Vec<u8>,
}

impl PkgFile {
    pub fn is_dir(&self) -> bool {
        self.path.ends_with('/')
    }
}

fn clean_path(p: &str) -> Option<String> {
    let parts: Vec<&str> = p.split('/').filter(|c| !c.is_empty() && *c != ".").collect();
    if parts.is_empty() || parts.contains(&"..") {
        return None;
    }
    Some(parts.join("/"))
}

pub fn build_package(info: &PkgInfo, files: &[PkgFile]) -> Result<Vec<u8>, String> {
    let mut w = tar::Writer::new();
    let err = |e: tar::Error| format!("{:?}", e);
    w.add_file(".PKGINFO", 0o644, 0, info.to_text().as_bytes()).map_err(err)?;
    for f in files {
        let path = clean_path(&f.path).ok_or_else(|| format!("bad path '{}'", f.path))?;
        if f.is_dir() {
            w.add_dir(&path, f.mode, 0).map_err(err)?;
        } else {
            w.add_file(&path, f.mode, 0, &f.data).map_err(err)?;
        }
    }
    Ok(w.finish())
}

/// Reads and validates a package archive.
pub fn read_package(data: &[u8]) -> Result<(PkgInfo, Vec<PkgFile>), String> {
    let mut info = None;
    let mut files = Vec::new();
    for entry in tar::Reader::new(data) {
        let e = entry.map_err(|e| format!("corrupt package: {:?}", e))?;
        if e.name.trim_start_matches("./") == ".PKGINFO" {
            info = Some(PkgInfo::parse(&String::from_utf8_lossy(e.data))?);
            continue;
        }
        let Some(path) = clean_path(&e.name) else { return Err(format!("unsafe path in package: '{}'", e.name)) };
        match e.kind {
            Kind::Directory => files.push(PkgFile { path: path + "/", mode: e.mode, data: Vec::new() }),
            Kind::File => files.push(PkgFile { path, mode: e.mode, data: e.data.to_vec() }),
            _ => return Err(format!("unsupported entry in package: '{}'", e.name)),
        }
    }
    Ok((info.ok_or("package has no .PKGINFO")?, files))
}

pub fn checksum(data: &[u8]) -> String {
    sha256::hex_digest(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn entry(name: &str, version: &str, deps: &[&str]) -> IndexEntry {
        let info = PkgInfo { name: name.into(), version: version.into(), description: "d".into(), depends: deps.iter().map(|d| d.to_string()).collect() };
        IndexEntry { file: info.file_name(), size: 1, sha256: "x".into(), repo: String::new(), info }
    }

    #[test]
    fn versions() {
        assert_eq!(version_cmp("1.10", "1.9"), Ordering::Greater);
        assert_eq!(version_cmp("1.0-2", "1.0-10"), Ordering::Less);
        assert_eq!(version_cmp("2.0", "2.0"), Ordering::Equal);
        assert_eq!(version_cmp("1.0", "1.0.1"), Ordering::Less);
        assert_eq!(version_cmp("01", "1"), Ordering::Equal);
    }

    #[test]
    fn index_round_trip() {
        let idx = vec![entry("a", "1.0", &["b", "c"]), entry("b", "2", &[])];
        assert_eq!(parse_index(&write_index(&idx)).unwrap(), idx);
        assert!(parse_index("name = ../evil\nversion = 1\n").is_err());
    }

    #[test]
    fn resolution() {
        let idx = vec![entry("app", "1", &["lib", "data"]), entry("lib", "1", &["base"]), entry("lib", "2", &["base"]), entry("base", "1", &[]), entry("data", "1", &["base"])];
        let r = resolve(&["app"], &idx, &|_| false).unwrap();
        let names: Vec<_> = r.iter().map(|e| (e.info.name.as_str(), e.info.version.as_str())).collect();
        assert_eq!(names, vec![("base", "1"), ("lib", "2"), ("data", "1"), ("app", "1")]);
        let r = resolve(&["app"], &idx, &|n| n == "base").unwrap();
        assert_eq!(r.len(), 3);
        assert!(resolve(&["nope"], &idx, &|_| false).unwrap_err().contains("not found"));
        let cyc = vec![entry("x", "1", &["y"]), entry("y", "1", &["x"])];
        assert!(resolve(&["x"], &cyc, &|_| false).unwrap_err().contains("cycle"));
    }

    #[test]
    fn package_round_trip() {
        let info = PkgInfo { name: "hello".into(), version: "1.0".into(), description: "Hi".into(), depends: vec![] };
        let files = vec![PkgFile { path: "bin/".into(), mode: 0o755, data: vec![] }, PkgFile { path: "bin/hello".into(), mode: 0o755, data: b"\x7fELF".to_vec() }];
        let data = build_package(&info, &files).unwrap();
        assert_eq!(read_package(&data).unwrap(), (info.clone(), files));
        assert!(build_package(&info, &[PkgFile { path: "../etc/passwd".into(), mode: 0o644, data: vec![] }]).is_err());
    }
}
