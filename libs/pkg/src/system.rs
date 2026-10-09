//! The declarative side of pkg: the system configuration, generations and
//! the store, as plain data (the `pkg` program does the file system work).
//!
//! `/etc/system.conf` describes the whole system:
//!
//! ```text
//! hostname = huldra
//! repo = http://10.0.2.2:8800
//! packages = fortune cowsay hello@1.0-1
//!
//! [etc/motd]
//! Welcome to Huldra.
//! ```
//!
//! `pkg switch` turns it into a *generation*: every package (with its
//! dependencies) unpacked once into the store as
//! `/pkg/store/HASH-NAME-VERSION`, a profile `sw/` of symbolic links into
//! those store paths, and the generated `/etc` files. Activating a
//! generation is one atomic rename of the `/pkg/system` link; rolling back
//! is activating an older one.

use crate::{stanzas, valid_name, IndexEntry, PkgInfo};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// One entry of `packages =`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// The newest version of a package from the repositories.
    Latest(String),
    /// `name@version`: exactly that version.
    Pinned(String, String),
    /// A local package archive (`/root/foo-1.0.pkg`).
    File(String),
}

impl Request {
    pub fn parse(s: &str) -> Result<Request, String> {
        if s.contains('/') || s.ends_with(".pkg") {
            return Ok(Request::File(s.to_string()));
        }
        let (name, version) = match s.split_once('@') {
            Some((n, v)) if !v.is_empty() => (n, Some(v)),
            Some(_) => return Err(format!("'{}': missing version after @", s)),
            None => (s, None),
        };
        if !valid_name(name) {
            return Err(format!("invalid package name '{}'", name));
        }
        Ok(match version {
            Some(v) => Request::Pinned(name.to_string(), v.to_string()),
            None => Request::Latest(name.to_string()),
        })
    }

    /// The package name, when known without reading an archive.
    pub fn name(&self) -> Option<&str> {
        match self {
            Request::Latest(n) | Request::Pinned(n, _) => Some(n),
            Request::File(_) => None,
        }
    }
}

/// `/etc/system.conf`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemConfig {
    pub repos: Vec<String>,
    /// Package requests as written (`name`, `name@version` or a path).
    pub packages: Vec<String>,
    /// Files generated under /etc: (path relative to /etc, contents).
    pub etc: Vec<(String, String)>,
}

/// A path relative to /etc without `.`, `..` or empty components.
fn valid_etc_path(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.ends_with('/') && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

/// `[etc/NAME]` on a line of its own starts a file section.
fn section_header(line: &str) -> Option<&str> {
    line.strip_prefix("[etc/")?.strip_suffix(']')
}

impl SystemConfig {
    pub fn parse(text: &str) -> Result<SystemConfig, String> {
        let mut cfg = SystemConfig::default();
        let mut lines = text.lines().enumerate().peekable();
        // Settings, up to the first file section.
        while let Some(&(n, line)) = lines.peek() {
            let t = line.trim();
            if section_header(t).is_some() {
                break;
            }
            lines.next();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let Some((k, v)) = t.split_once('=') else {
                return Err(format!("line {}: expected 'key = value' or [etc/NAME]", n + 1));
            };
            let v = v.trim();
            match k.trim() {
                "hostname" => {
                    if v.is_empty() || v.contains(char::is_whitespace) {
                        return Err(format!("line {}: invalid host name '{}'", n + 1, v));
                    }
                    cfg.etc.push(("hostname".into(), format!("{}\n", v)));
                }
                "repo" => cfg.repos.push(v.trim_end_matches('/').to_string()),
                "packages" => {
                    for p in v.split_whitespace() {
                        Request::parse(p).map_err(|e| format!("line {}: {}", n + 1, e))?;
                        if !cfg.packages.iter().any(|q| q == p) {
                            cfg.packages.push(p.to_string());
                        }
                    }
                }
                other => return Err(format!("line {}: unknown setting '{}'", n + 1, other)),
            }
        }
        // File sections: everything up to the next header, verbatim.
        while let Some((n, line)) = lines.next() {
            let name = section_header(line.trim()).ok_or_else(|| format!("line {}: expected [etc/NAME]", n + 1))?;
            if !valid_etc_path(name) {
                return Err(format!("line {}: invalid /etc path '{}'", n + 1, name));
            }
            if cfg.etc.iter().any(|(e, _)| e == name) {
                return Err(format!("line {}: /etc/{} is defined twice", n + 1, name));
            }
            let mut body = String::new();
            while let Some(&(_, l)) = lines.peek() {
                if section_header(l.trim()).is_some() {
                    break;
                }
                body.push_str(l);
                body.push('\n');
                lines.next();
            }
            let trimmed = body.trim_end_matches('\n');
            let body = if trimmed.is_empty() { String::new() } else { format!("{}\n", trimmed) };
            cfg.etc.push((name.to_string(), body));
        }
        Ok(cfg)
    }
}

/// Rewrites the `packages =` lines of a configuration, keeping everything
/// else (comments, order, file sections) as it was.
pub fn set_packages(text: &str, packages: &[String]) -> String {
    let line = format!("packages = {}", packages.join(" "));
    let mut out: Vec<String> = Vec::new();
    let mut placed = false;
    let mut in_sections = false;
    for l in text.lines() {
        let t = l.trim();
        in_sections |= section_header(t).is_some();
        if !in_sections && t.split_once('=').is_some_and(|(k, _)| k.trim() == "packages") {
            if !placed {
                out.push(line.clone());
                placed = true;
            }
            continue;
        }
        if in_sections && !placed {
            // No packages line yet: put it before the first file section.
            while out.last().is_some_and(|l| l.trim().is_empty()) {
                out.pop();
            }
            out.push(line.clone());
            out.push(String::new());
            placed = true;
        }
        out.push(l.to_string());
    }
    if !placed {
        out.push(line);
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Name of a package's directory in the store: the start of its archive's
/// SHA-256, so identical archives share it and different builds never mix.
pub fn store_name(info: &PkgInfo, sha256: &str) -> String {
    format!("{}-{}-{}", &sha256[..16.min(sha256.len())], info.name, info.version)
}

/// A package in a generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenPackage {
    pub name: String,
    pub version: String,
    pub store: String,
}

/// What a generation contains (`/pkg/generations/N/manifest`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub generation: u32,
    /// Unix time it was built.
    pub created: i64,
    /// Every package, dependencies first.
    pub packages: Vec<GenPackage>,
    /// Files it puts in /etc (relative to /etc).
    pub etc: Vec<String>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let pairs = stanzas(text).into_iter().next().unwrap_or_default();
        let mut m = Manifest::default();
        for (k, v) in pairs {
            match k.as_str() {
                "generation" => m.generation = v.parse().map_err(|_| "bad generation number")?,
                "created" => m.created = v.parse().unwrap_or(0),
                "package" => {
                    let f: Vec<&str> = v.split_whitespace().collect();
                    let [name, version, store] = f[..] else { return Err(format!("bad package line '{}'", v)) };
                    m.packages.push(GenPackage { name: name.into(), version: version.into(), store: store.into() });
                }
                "etc" => m.etc.push(v),
                _ => {}
            }
        }
        if m.generation == 0 {
            return Err("manifest has no generation number".into());
        }
        Ok(m)
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("generation = {}\ncreated = {}\n", self.generation, self.created);
        for p in &self.packages {
            s.push_str(&format!("package = {} {} {}\n", p.name, p.version, p.store));
        }
        for e in &self.etc {
            s.push_str(&format!("etc = {}\n", e));
        }
        s
    }

    pub fn store_paths(&self) -> impl Iterator<Item = &str> {
        self.packages.iter().map(|p| p.store.as_str())
    }
}

/// Chooses the index entry for each request (dependencies are added by
/// [`crate::resolve`]).
pub fn pick<'a>(req: &Request, index: &'a [IndexEntry]) -> Result<&'a IndexEntry, String> {
    match req {
        Request::Latest(n) => crate::find(index, n).ok_or_else(|| format!("package '{}' not found", n)),
        Request::Pinned(n, v) => index
            .iter()
            .find(|e| &e.info.name == n && &e.info.version == v)
            .ok_or_else(|| format!("package '{}' version {} not found", n, v)),
        Request::File(p) => Err(format!("{}: not in a repository", p)),
    }
}

/// Merges the files of several store paths into one profile tree.
/// `packages` holds (store name, files relative to the store path);
/// returns profile path -> store name. Two packages providing the same
/// path is an error.
pub fn merge_profile(packages: &[(String, Vec<String>)]) -> Result<BTreeMap<String, String>, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for (store, files) in packages {
        for f in files {
            if let Some(other) = out.insert(f.clone(), store.clone()) {
                if &other != store {
                    return Err(format!("{} and {} both provide {}", other, store, f));
                }
            }
        }
    }
    Ok(out)
}

/// Store entries no remaining generation refers to (and leftovers of
/// interrupted unpacking, which end in `.tmp`).
pub fn unreferenced(store: &[String], manifests: &[Manifest]) -> Vec<String> {
    let live: BTreeSet<&str> = manifests.iter().flat_map(|m| m.store_paths()).collect();
    store.iter().filter(|s| s.ends_with(".tmp") || !live.contains(s.as_str())).cloned().collect()
}

/// What changes between two generations, for the summary `pkg switch` prints.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// (name, old version, new version)
    pub changed: Vec<(String, String, String)>,
}

pub fn diff(old: Option<&Manifest>, new: &Manifest) -> Diff {
    let mut d = Diff::default();
    let old_pkgs: &[GenPackage] = old.map_or(&[], |m| &m.packages);
    for p in &new.packages {
        match old_pkgs.iter().find(|o| o.name == p.name) {
            None => d.added.push(format!("{} {}", p.name, p.version)),
            Some(o) if o.store != p.store => d.changed.push((p.name.clone(), o.version.clone(), p.version.clone())),
            Some(_) => {}
        }
    }
    for o in old_pkgs {
        if !new.packages.iter().any(|p| p.name == o.name) {
            d.removed.push(format!("{} {}", o.name, o.version));
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    const CONF: &str = "# my system
hostname = box
repo = http://a:1/
packages = fortune cowsay@3.0-1
packages = /root/x-1.pkg fortune

[etc/motd]
Hello
[not a header

[etc/net/hosts]
127.0.0.1 localhost
";

    #[test]
    fn config() {
        let c = SystemConfig::parse(CONF).unwrap();
        assert_eq!(c.repos, vec!["http://a:1"]);
        assert_eq!(c.packages, vec!["fortune", "cowsay@3.0-1", "/root/x-1.pkg"]);
        assert_eq!(
            c.etc,
            vec![
                ("hostname".into(), "box\n".into()),
                ("motd".into(), "Hello\n[not a header\n".into()),
                ("net/hosts".into(), "127.0.0.1 localhost\n".into()),
            ]
        );
        assert!(SystemConfig::parse("pakages = x").unwrap_err().contains("unknown setting"));
        assert!(SystemConfig::parse("packages = ../x").is_ok()); // a path
        assert!(SystemConfig::parse("packages = a@").is_err());
        assert!(SystemConfig::parse("[etc/../passwd]\nx").is_err());
        assert!(SystemConfig::parse("hostname = a\n[etc/hostname]\nb").unwrap_err().contains("twice"));
        assert_eq!(SystemConfig::parse("").unwrap(), SystemConfig::default());
    }

    #[test]
    fn requests() {
        assert_eq!(Request::parse("a").unwrap(), Request::Latest("a".into()));
        assert_eq!(Request::parse("a@1.0-2").unwrap(), Request::Pinned("a".into(), "1.0-2".into()));
        assert_eq!(Request::parse("./a.pkg").unwrap(), Request::File("./a.pkg".into()));
        assert!(Request::parse(".a").is_err());
    }

    #[test]
    fn editing_packages() {
        let text = set_packages(CONF, &["sl".into(), "2048".into()]);
        let c = SystemConfig::parse(&text).unwrap();
        assert_eq!(c.packages, vec!["sl", "2048"]);
        assert!(text.starts_with("# my system\nhostname = box\nrepo = http://a:1/\npackages = sl 2048\n\n[etc/motd]"));
        let t = set_packages("repo = x\n\n[etc/motd]\nhi\n", &["a".into()]);
        assert_eq!(t, "repo = x\npackages = a\n\n[etc/motd]\nhi\n");
        assert_eq!(set_packages("# empty\n", &["a".into()]), "# empty\npackages = a\n");
    }

    fn gen(n: u32, pkgs: &[(&str, &str, &str)]) -> Manifest {
        Manifest {
            generation: n,
            created: 5,
            packages: pkgs.iter().map(|(a, b, c)| GenPackage { name: (*a).into(), version: (*b).into(), store: (*c).into() }).collect(),
            etc: vec!["motd".into(), "net/hosts".into()],
        }
    }

    #[test]
    fn manifests_and_gc() {
        let m = gen(3, &[("a", "1", "h1-a-1"), ("b", "2", "h2-b-2")]);
        assert_eq!(Manifest::parse(&m.to_text()).unwrap(), m);
        assert!(Manifest::parse("created = 1").is_err());
        let old = gen(2, &[("a", "0", "h0-a-0"), ("c", "1", "h3-c-1"), ("b", "2", "h2-b-2")]);
        let d = diff(Some(&old), &m);
        assert_eq!(d.added, Vec::<String>::new());
        assert_eq!(d.removed, vec!["c 1"]);
        assert_eq!(d.changed, vec![("a".into(), "0".into(), "1".into())]);
        assert_eq!(diff(None, &m).added, vec!["a 1", "b 2"]);
        let store: Vec<String> = ["h0-a-0", "h1-a-1", "h2-b-2", "h3-c-1", "h9-z-1.tmp"].iter().map(|s| s.to_string()).collect();
        assert_eq!(unreferenced(&store, &[m.clone()]), vec!["h0-a-0", "h3-c-1", "h9-z-1.tmp"]);
        assert_eq!(unreferenced(&store, &[m, old]), vec!["h9-z-1.tmp"]);
    }

    #[test]
    fn profiles() {
        let p = merge_profile(&[
            ("s1".into(), vec!["bin/a".into(), "share/a/x".into()]),
            ("s2".into(), vec!["bin/b".into()]),
        ])
        .unwrap();
        assert_eq!(p.get("bin/b").map(String::as_str), Some("s2"));
        assert_eq!(p.len(), 3);
        let e = merge_profile(&[("s1".into(), vec!["bin/a".into()]), ("s2".into(), vec!["bin/a".into()])]).unwrap_err();
        assert!(e.contains("both provide bin/a"));
    }

    #[test]
    fn store_names() {
        let info = PkgInfo { name: "hello".into(), version: "1.0-1".into(), ..PkgInfo::default() };
        assert_eq!(store_name(&info, "0123456789abcdef0123"), "0123456789abcdef-hello-1.0-1");
    }
}
