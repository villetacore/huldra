//! pkg: the Huldra package manager.
//!
//!   pkg update                 fetch the package index from the repositories
//!   pkg search [WORD]          list available packages
//!   pkg info NAME              show a package
//!   pkg install NAME|FILE.pkg  install packages with their dependencies
//!   pkg remove NAME            uninstall a package
//!   pkg upgrade                install newer versions of installed packages
//!   pkg list                   installed packages
//!   pkg files NAME             files owned by an installed package
//!
//! Repositories are listed in /etc/pkg.conf (`repo = http://host:port`);
//! state lives in /var/lib/pkg (the index cache and one record per
//! installed package).

#![no_std]
#![no_main]

use huldra_pkg::{self as pkg, IndexEntry, Installed, PkgFile};
use huldra_user::abi::fs::{O_CREAT, O_TRUNC, O_WRONLY};
use huldra_user::{env, eprint, eprintln, format, fs, net, println, Errno, String, ToString, Vec};

huldra_user::main!(main);

const CONF: &str = "/etc/pkg.conf";
const STATE: &str = "/var/lib/pkg";
const DB: &str = "/var/lib/pkg/installed";
const INDEX: &str = "/var/lib/pkg/index";
const DEFAULT_REPO: &str = "http://10.0.2.2:8800";

type Result<T> = core::result::Result<T, String>;

fn repos() -> Vec<String> {
    let text = fs::read_to_string(CONF).unwrap_or_default();
    let v: Vec<String> = text
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == "repo").then(|| v.trim().trim_end_matches('/').to_string())
        })
        .collect();
    if v.is_empty() {
        alloc_one(DEFAULT_REPO)
    } else {
        v
    }
}

fn alloc_one(s: &str) -> Vec<String> {
    let mut v = Vec::new();
    v.push(s.to_string());
    v
}

fn index() -> Result<Vec<IndexEntry>> {
    let text = fs::read_to_string(INDEX).map_err(|_| String::from("no package index: run 'pkg update' first"))?;
    pkg::parse_index(&text)
}

fn installed() -> Vec<Installed> {
    let mut v: Vec<Installed> = fs::read_dir(DB)
        .unwrap_or_default()
        .iter()
        .filter_map(|e| fs::read_to_string(&fs::join(DB, &e.name)).ok())
        .filter_map(|t| Installed::parse(&t).ok())
        .collect();
    v.sort_by(|a, b| a.info.name.cmp(&b.info.name));
    v
}

fn installed_one(name: &str) -> Option<Installed> {
    fs::read_to_string(&fs::join(DB, name)).ok().and_then(|t| Installed::parse(&t).ok())
}

fn download(url: &str, label: &str) -> Result<Vec<u8>> {
    let mut last = 0;
    let r = net::http_get(url, |n, total| {
        if n - last >= 32768 || Some(n) == total {
            last = n;
            if let Some(t) = total {
                eprint!("\r  {} {:>3}%", label, n * 100 / t.max(1));
            }
        }
    });
    eprint!("\r");
    let resp = r.map_err(|e| format!("{}: {}", url, if e == Errno::ENOENT { "unknown host" } else { e.message() }))?;
    if resp.status != 200 {
        return Err(format!("{}: HTTP {}", url, resp.status));
    }
    Ok(resp.body)
}

fn update() -> Result<()> {
    let mut all = Vec::new();
    for repo in repos() {
        println!("fetching {}/INDEX", repo);
        let text = download(&format!("{}/INDEX", repo), "INDEX")?;
        let mut entries = pkg::parse_index(&String::from_utf8_lossy(&text)).map_err(|e| format!("{}: bad index: {}", repo, e))?;
        for e in &mut entries {
            e.repo = repo.clone();
        }
        println!("  {} packages", entries.len());
        all.extend(entries);
    }
    fs::create_dir_all(STATE).map_err(|e| format!("{}: {}", STATE, e))?;
    fs::write(INDEX, pkg::write_index(&all).as_bytes()).map_err(|e| format!("{}: {}", INDEX, e))?;
    let upgradable = installed().iter().filter(|i| pkg::find(&all, &i.info.name).is_some_and(|e| pkg::version_cmp(&e.info.version, &i.info.version).is_gt())).count();
    if upgradable > 0 {
        println!("{} package(s) can be upgraded: run 'pkg upgrade'", upgradable);
    }
    Ok(())
}

fn search(word: Option<&str>) -> Result<()> {
    let idx = index()?;
    let inst = installed();
    let mut names: Vec<&str> = idx.iter().map(|e| e.info.name.as_str()).collect();
    names.sort();
    names.dedup();
    for n in names {
        let e = pkg::find(&idx, n).unwrap();
        let matches = word.is_none_or(|w| e.info.name.contains(w) || e.info.description.to_lowercase().contains(&w.to_lowercase()));
        if matches {
            let mark = if inst.iter().any(|i| i.info.name == n) { " [installed]" } else { "" };
            println!("{:<16} {:<10} {}{}", e.info.name, e.info.version, e.info.description, mark);
        }
    }
    Ok(())
}

fn info(name: &str) -> Result<()> {
    let inst = installed_one(name);
    let entry = index().ok().and_then(|idx| pkg::find(&idx, name).cloned());
    let info = match (&inst, &entry) {
        (Some(i), _) => i.info.clone(),
        (None, Some(e)) => e.info.clone(),
        _ => return Err(format!("package '{}' not found", name)),
    };
    println!("Name:        {}", info.name);
    println!("Version:     {}", info.version);
    println!("Description: {}", info.description);
    println!("Depends:     {}", if info.depends.is_empty() { String::from("-") } else { info.depends.join(" ") });
    println!("Installed:   {}", inst.as_ref().map_or(String::from("no"), |i| format!("{} ({} files)", i.info.version, i.files.iter().filter(|f| !f.ends_with('/')).count())));
    if let Some(e) = entry {
        println!("Available:   {} from {} ({} bytes)", e.info.version, e.repo, e.size);
    }
    Ok(())
}

/// Who owns `path` (relative), among installed packages other than `except`.
fn owner(path: &str, except: &str, db: &[Installed]) -> Option<String> {
    db.iter().find(|i| i.info.name != except && i.files.iter().any(|f| f == path)).map(|i| i.info.name.clone())
}

/// Installs one downloaded (or local) package archive.
fn install_archive(data: &[u8], expect: Option<&IndexEntry>) -> Result<()> {
    if let Some(e) = expect {
        if e.size != 0 && data.len() as u64 != e.size {
            return Err(format!("{}: size mismatch ({} bytes, expected {})", e.file, data.len(), e.size));
        }
        let sum = pkg::checksum(data);
        if !e.sha256.is_empty() && sum != e.sha256 {
            return Err(format!("{}: checksum mismatch", e.file));
        }
    }
    let (info, files) = pkg::read_package(data)?;
    let db = installed();
    let previous = db.iter().find(|i| i.info.name == info.name).cloned();

    // Refuse to overwrite files that belong to someone else.
    for f in files.iter().filter(|f| !f.is_dir()) {
        let abs = format!("/{}", f.path);
        if let Some(o) = owner(&f.path, &info.name, &db) {
            return Err(format!("{}: /{} belongs to package {}", info.name, f.path, o));
        }
        let ours = previous.as_ref().is_some_and(|p| p.files.contains(&f.path));
        if !ours && fs::exists(&abs) {
            return Err(format!("{}: /{} already exists", info.name, f.path));
        }
    }
    for f in &files {
        write_entry(f)?;
    }
    // An upgrade drops files the new version no longer has.
    if let Some(prev) = &previous {
        for old in prev.files.iter().filter(|o| !o.ends_with('/') && !files.iter().any(|f| &f.path == *o)) {
            let _ = fs::remove_file(&format!("/{}", old));
        }
    }
    let record = Installed { info: info.clone(), files: files.iter().map(|f| f.path.clone()).collect() };
    fs::create_dir_all(DB).map_err(|e| format!("{}: {}", DB, e))?;
    fs::write(&fs::join(DB, &info.name), record.to_text().as_bytes()).map_err(|e| format!("{}: {}", DB, e))?;
    match previous {
        Some(p) if p.info.version != info.version => println!("upgraded {} {} -> {}", info.name, p.info.version, info.version),
        Some(_) => println!("reinstalled {} {}", info.name, info.version),
        None => println!("installed {} {}", info.name, info.version),
    }
    Ok(())
}

fn write_entry(f: &PkgFile) -> Result<()> {
    let abs = format!("/{}", f.path.trim_end_matches('/'));
    if f.is_dir() {
        return fs::create_dir_all(&abs).map_err(|e| format!("{}: {}", abs, e));
    }
    if let Some((dir, _)) = abs.rsplit_once('/') {
        if !dir.is_empty() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir, e))?;
        }
    }
    let file = fs::File::open_with(&abs, O_WRONLY | O_CREAT | O_TRUNC, f.mode & 0o7777).map_err(|e| format!("{}: {}", abs, e))?;
    file.write_all(&f.data).map_err(|e| format!("{}: {}", abs, e))
}

fn install(targets: &[&str]) -> Result<()> {
    let (local, names): (Vec<&str>, Vec<&str>) = targets.iter().partition(|t| t.ends_with(".pkg"));
    for path in local {
        let data = fs::read(path).map_err(|e| format!("{}: {}", path, e))?;
        install_archive(&data, None)?;
    }
    if names.is_empty() {
        return Ok(());
    }
    let idx = index()?;
    let db = installed();
    let plan = pkg::resolve(&names, &idx, &|n| db.iter().any(|i| i.info.name == n))?;
    let deps: Vec<&str> = plan.iter().map(|e| e.info.name.as_str()).filter(|n| !names.contains(n)).collect();
    if !deps.is_empty() {
        println!("also installing dependencies: {}", deps.join(" "));
    }
    for e in plan {
        let data = download(&format!("{}/{}", e.repo, e.file), &e.file)?;
        install_archive(&data, Some(e))?;
    }
    Ok(())
}

fn remove(name: &str, force: bool) -> Result<()> {
    let inst = installed_one(name).ok_or_else(|| format!("package '{}' is not installed", name))?;
    let users: Vec<String> = installed().into_iter().filter(|i| i.info.depends.iter().any(|d| d == name)).map(|i| i.info.name).collect();
    if !users.is_empty() && !force {
        return Err(format!("{} is needed by: {} (use -f to remove anyway)", name, users.join(" ")));
    }
    // Files first, then directories deepest first (only if empty).
    for f in inst.files.iter().filter(|f| !f.ends_with('/')) {
        let _ = fs::remove_file(&format!("/{}", f));
    }
    let mut dirs: Vec<&String> = inst.files.iter().filter(|f| f.ends_with('/')).collect();
    dirs.sort_by_key(|d| core::cmp::Reverse(d.len()));
    for d in dirs {
        let _ = fs::remove_dir(&format!("/{}", d.trim_end_matches('/')));
    }
    fs::remove_file(&fs::join(DB, name)).map_err(|e| format!("{}: {}", DB, e))?;
    println!("removed {} {}", inst.info.name, inst.info.version);
    Ok(())
}

fn upgrade() -> Result<()> {
    let idx = index()?;
    let todo: Vec<String> = installed()
        .into_iter()
        .filter(|i| pkg::find(&idx, &i.info.name).is_some_and(|e| pkg::version_cmp(&e.info.version, &i.info.version).is_gt()))
        .map(|i| i.info.name)
        .collect();
    if todo.is_empty() {
        println!("all packages are up to date");
        return Ok(());
    }
    let names: Vec<&str> = todo.iter().map(|s| s.as_str()).collect();
    install(&names)
}

fn usage() -> i32 {
    eprintln!("usage: pkg update | search [WORD] | info NAME | install NAME|FILE.pkg... | remove [-f] NAME... | upgrade | list | files NAME");
    2
}

fn main() -> i32 {
    let args = env::args();
    let Some(cmd) = args.get(1) else { return usage() };
    let rest: Vec<&str> = args[2..].iter().map(|s| s.as_str()).collect();
    let r = match cmd.as_str() {
        "update" => update(),
        "search" => search(rest.first().copied()),
        "info" | "show" => match rest.first() {
            Some(n) => info(n),
            None => return usage(),
        },
        "install" | "add" => {
            if rest.is_empty() {
                return usage();
            }
            install(&rest)
        }
        "remove" | "rm" => {
            let force = rest.contains(&"-f");
            let names: Vec<&&str> = rest.iter().filter(|a| **a != "-f").collect();
            if names.is_empty() {
                return usage();
            }
            names.iter().try_for_each(|n| remove(n, force))
        }
        "upgrade" => upgrade(),
        "list" => {
            for i in installed() {
                println!("{:<16} {:<10} {}", i.info.name, i.info.version, i.info.description);
            }
            Ok(())
        }
        "files" => match rest.first().and_then(|n| installed_one(n)) {
            Some(i) => {
                for f in i.files.iter().filter(|f| !f.ends_with('/')) {
                    println!("/{}", f);
                }
                Ok(())
            }
            None => Err(String::from("package is not installed")),
        },
        _ => return usage(),
    };
    match r {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("pkg: {}", e);
            1
        }
    }
}
