//! pkg: the Huldra package manager. The system is declared in
//! /etc/system.conf; pkg makes it so.
//!
//!   pkg switch [-n]            build a generation from /etc/system.conf and activate it
//!   pkg add NAME...            add packages to system.conf and switch
//!   pkg remove NAME...         remove packages from system.conf and switch
//!   pkg rollback [N]           activate the previous (or a given) generation
//!   pkg generations            list generations
//!   pkg gc [-d]                delete unused store paths (-d: and old generations)
//!   pkg update                 fetch the package index from the repositories
//!   pkg search [WORD]          list available packages
//!   pkg info NAME              show a package
//!   pkg list                   packages in the running system
//!   pkg files NAME             files a package puts in the profile
//!
//! Layout (see huldra_pkg::system):
//!
//!   /pkg/store/HASH-NAME-VERSION   unpacked packages, never modified
//!   /pkg/generations/N/            manifest, system.conf, sw/ (links into
//!                                  the store) and etc/ (generated files)
//!   /pkg/system -> generations/N   the running system; programs are in
//!                                  /pkg/system/sw/bin, /etc/NAME links to
//!                                  /pkg/system/etc/NAME
//!
//! Every step writes new files beside the old ones and finishes with a
//! rename, so an interrupted pkg leaves the previous system running.

#![no_std]
#![no_main]

use huldra_pkg::system::{self as decl, Manifest, Request, SystemConfig};
use huldra_pkg::{self as pkg, IndexEntry, PkgInfo};
use huldra_user::abi::fs::{O_CREAT, O_EXCL, O_TRUNC, O_WRONLY};
use huldra_user::time::{self, DateTime};
use huldra_user::{env, eprint, eprintln, format, fs, http, println, process, Errno, String, ToString, Vec};

huldra_user::main!(main);

const CONF: &str = "/etc/system.conf";
const STORE: &str = "/pkg/store";
const GENERATIONS: &str = "/pkg/generations";
const SYSTEM: &str = "/pkg/system";
const INDEX: &str = "/pkg/cache/INDEX";
const LOCK: &str = "/pkg/lock";
/// The last generation number handed out.
const COUNTER: &str = "/pkg/generations/last";
const DEFAULT_REPO: &str = "http://10.0.2.2:8800";

type Result<T> = core::result::Result<T, String>;

fn ctx<T>(r: huldra_user::Result<T>, what: &str) -> Result<T> {
    r.map_err(|e| format!("{}: {}", what, e))
}

/// Writes a file through a temporary name and a rename.
fn write_atomic(path: &str, data: &[u8]) -> Result<()> {
    let tmp = format!("{}.tmp", path);
    ctx(fs::write(&tmp, data), &tmp)?;
    ctx(fs::rename(&tmp, path), path)
}

/// Points `link` at `target`, replacing whatever link was there at once.
fn set_link(target: &str, link: &str) -> Result<()> {
    let tmp = format!("{}.tmp", link);
    let _ = fs::remove_file(&tmp);
    ctx(fs::symlink(target, &tmp), &tmp)?;
    ctx(fs::rename(&tmp, link), link)
}

// ------------------------------------------------------------------ locking

/// Only one pkg changes the system at a time. The lock file holds the
/// owner's pid; a lock whose owner is gone is taken over.
struct Lock;

impl Lock {
    fn acquire() -> Result<Lock> {
        ctx(fs::create_dir_all("/pkg"), "/pkg")?;
        for _ in 0..2 {
            match fs::File::open_with(LOCK, O_WRONLY | O_CREAT | O_EXCL, 0o644) {
                Ok(f) => {
                    ctx(f.write_all(format!("{}\n", process::getpid()).as_bytes()), LOCK)?;
                    return Ok(Lock);
                }
                Err(Errno::EEXIST) => {
                    let pid = fs::read_to_string(LOCK).unwrap_or_default();
                    let pid = pid.trim();
                    if !pid.is_empty() && fs::exists(&format!("/proc/{}", pid)) {
                        return Err(format!("another pkg (pid {}) is running", pid));
                    }
                    let _ = fs::remove_file(LOCK);
                }
                Err(e) => return Err(format!("{}: {}", LOCK, e)),
            }
        }
        Err(format!("{}: cannot take the lock", LOCK))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(LOCK);
    }
}

// ------------------------------------------------------------ configuration

fn config_text() -> String {
    fs::read_to_string(CONF).unwrap_or_default()
}

fn parse_config(text: &str) -> Result<SystemConfig> {
    let mut cfg = SystemConfig::parse(text).map_err(|e| format!("{}: {}", CONF, e))?;
    if cfg.repos.is_empty() {
        cfg.repos.push(DEFAULT_REPO.to_string());
    }
    Ok(cfg)
}

// -------------------------------------------------------------------- index

fn download(url: &str, label: &str) -> Result<Vec<u8>> {
    let mut last = 0;
    let mut body = Vec::new();
    // Packages are compressed already; the index is small.
    let opts = http::Options { gzip: false, ..http::Options::default() };
    let req = huldra_http::Request::get(huldra_http::Url::parse(url)?);
    let r = http::fetch(req, &opts, &mut |_, b| {
        body.extend_from_slice(b);
        Ok(())
    }, &mut |n, total| {
        if n - last >= 32768 || Some(n) == total {
            last = n;
            if let Some(t) = total {
                eprint!("\r  {} {:>3}%", label, n * 100 / t.max(1));
            }
        }
    });
    eprint!("\r\x1b[K");
    let (_, head) = r.map_err(|e| format!("{}: {}", url, e))?;
    if head.status != 200 {
        return Err(format!("{}: HTTP {}", url, http::status_text(&head)));
    }
    Ok(body)
}

fn update(cfg: &SystemConfig) -> Result<Vec<IndexEntry>> {
    let mut all = Vec::new();
    for repo in &cfg.repos {
        println!("fetching {}/INDEX", repo);
        let text = download(&format!("{}/INDEX", repo), "INDEX")?;
        let mut entries = pkg::parse_index(&String::from_utf8_lossy(&text)).map_err(|e| format!("{}: bad index: {}", repo, e))?;
        for e in &mut entries {
            e.repo = repo.clone();
        }
        println!("  {} packages", entries.len());
        all.extend(entries);
    }
    ctx(fs::create_dir_all("/pkg/cache"), "/pkg/cache")?;
    write_atomic(INDEX, pkg::write_index(&all).as_bytes())?;
    Ok(all)
}

fn cached_index() -> Result<Vec<IndexEntry>> {
    let text = fs::read_to_string(INDEX).map_err(|_| String::from("no package index: run 'pkg update' first"))?;
    pkg::parse_index(&text)
}

/// The cached index, fetched first if there is none yet.
fn index(cfg: &SystemConfig) -> Result<Vec<IndexEntry>> {
    match fs::read_to_string(INDEX) {
        Ok(text) => pkg::parse_index(&text),
        Err(_) => update(cfg),
    }
}

// -------------------------------------------------------------------- store

fn store_path(name: &str) -> String {
    format!("{}/{}", STORE, name)
}

/// Unpacks a package archive into the store (unless it is already there).
fn unpack(data: &[u8], store: &str) -> Result<()> {
    let dest = store_path(store);
    if fs::exists(&dest) {
        return Ok(());
    }
    let (info, files) = pkg::read_package(data)?;
    let tmp = format!("{}.tmp", dest);
    let _ = fs::remove_all(&tmp);
    ctx(fs::create_dir_all(&tmp), &tmp)?;
    for f in &files {
        let path = format!("{}/{}", tmp, f.path.trim_end_matches('/'));
        if f.is_dir() {
            ctx(fs::create_dir_all(&path), &path)?;
            continue;
        }
        if let Some((dir, _)) = path.rsplit_once('/') {
            ctx(fs::create_dir_all(dir), dir)?;
        }
        let file = ctx(fs::File::open_with(&path, O_WRONLY | O_CREAT | O_TRUNC, f.mode & 0o7777), &path)?;
        ctx(file.write_all(&f.data), &path)?;
    }
    ctx(fs::write(&format!("{}/.PKGINFO", tmp), info.to_text().as_bytes()), &tmp)?;
    ctx(fs::rename(&tmp, &dest), &dest)
}

/// Makes sure the package of an index entry is in the store.
fn realize(e: &IndexEntry) -> Result<String> {
    let name = decl::store_name(&e.info, &e.sha256);
    if fs::exists(&store_path(&name)) {
        return Ok(name);
    }
    let data = download(&format!("{}/{}", e.repo, e.file), &e.file)?;
    if e.size != 0 && data.len() as u64 != e.size {
        return Err(format!("{}: size mismatch ({} bytes, expected {})", e.file, data.len(), e.size));
    }
    if pkg::checksum(&data) != e.sha256 {
        return Err(format!("{}: checksum mismatch", e.file));
    }
    unpack(&data, &name)?;
    println!("  fetched {} {}", e.info.name, e.info.version);
    Ok(name)
}

/// Files and links of a store path, relative to it (no directories).
fn store_files(store: &str) -> Result<Vec<String>> {
    fn walk(dir: &str, rel: &str, out: &mut Vec<String>) -> Result<()> {
        for e in ctx(fs::read_dir(dir), dir)? {
            let r = if rel.is_empty() { e.name.clone() } else { format!("{}/{}", rel, e.name) };
            if r == ".PKGINFO" {
                continue;
            }
            if e.is_dir() {
                walk(&fs::join(dir, &e.name), &r, out)?;
            } else {
                out.push(r);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(&store_path(store), "", &mut out)?;
    Ok(out)
}

fn store_info(store: &str) -> Option<PkgInfo> {
    PkgInfo::parse(&fs::read_to_string(&format!("{}/.PKGINFO", store_path(store))).ok()?).ok()
}

// -------------------------------------------------------------- generations

fn manifest(n: u32) -> Result<Manifest> {
    let path = format!("{}/{}/manifest", GENERATIONS, n);
    Manifest::parse(&ctx(fs::read_to_string(&path), &path)?)
}

/// All generations, oldest first.
fn generations() -> Vec<Manifest> {
    let mut v: Vec<Manifest> = fs::read_dir(GENERATIONS)
        .unwrap_or_default()
        .iter()
        .filter_map(|e| e.name.parse::<u32>().ok())
        .filter_map(|n| manifest(n).ok())
        .collect();
    v.sort_by_key(|m| m.generation);
    v
}

/// The number of the running generation.
fn current() -> Option<u32> {
    fs::read_link(SYSTEM).ok()?.rsplit('/').next()?.parse().ok()
}

fn current_manifest() -> Option<Manifest> {
    manifest(current()?).ok()
}

/// Everything a configuration resolves to, before anything is written.
struct Plan {
    manifest: Manifest,
    etc: Vec<(String, String)>,
}

/// Resolves the configuration and puts every package it needs into the
/// store. Nothing outside the store changes.
fn plan(cfg: &SystemConfig) -> Result<Plan> {
    let mut requests = Vec::new();
    for p in &cfg.packages {
        requests.push(Request::parse(p)?);
    }
    // Local archives go straight into the store; their dependencies come
    // from the repositories like everything else.
    let mut local: Vec<(PkgInfo, String)> = Vec::new();
    for r in &requests {
        if let Request::File(path) = r {
            let data = ctx(fs::read(path), path)?;
            let (info, _) = pkg::read_package(&data)?;
            let store = decl::store_name(&info, &pkg::checksum(&data));
            unpack(&data, &store)?;
            local.push((info, store));
        }
    }
    let mut targets: Vec<String> = Vec::new();
    for r in &requests {
        if let Some(n) = r.name() {
            targets.push(n.to_string());
        }
    }
    for (info, _) in &local {
        targets.extend(info.depends.iter().cloned());
    }
    let needs_index = !targets.is_empty();
    let full = if needs_index { index(cfg)? } else { Vec::new() };
    // Pins and local archives replace every other version of their name.
    let mut idx: Vec<IndexEntry> = Vec::new();
    for e in &full {
        let pinned_away = requests.iter().any(|r| matches!(r, Request::Pinned(n, v) if *n == e.info.name && *v != e.info.version));
        let local_name = local.iter().any(|(i, _)| i.name == e.info.name);
        if !pinned_away && !local_name {
            idx.push(e.clone());
        }
    }
    for r in &requests {
        if r.name().is_some() {
            decl::pick(r, &idx)?;
        }
    }
    let names: Vec<&str> = targets.iter().map(|s| s.as_str()).filter(|n| !local.iter().any(|(i, _)| i.name == *n)).collect();
    let order = pkg::resolve(&names, &idx, &|_| false)?;

    let mut packages = Vec::new();
    for e in order {
        let store = realize(e)?;
        packages.push(decl::GenPackage { name: e.info.name.clone(), version: e.info.version.clone(), store });
    }
    for (info, store) in local {
        packages.push(decl::GenPackage { name: info.name, version: info.version, store });
    }
    let manifest = Manifest {
        generation: 0,
        created: time::now(),
        packages,
        etc: cfg.etc.iter().map(|(n, _)| n.clone()).collect(),
    };
    Ok(Plan { manifest, etc: cfg.etc.clone() })
}

/// True if a plan would produce exactly the running generation.
fn same_as(plan: &Plan, cur: &Manifest) -> bool {
    let stores = |m: &Manifest| -> Vec<String> { m.store_paths().map(String::from).collect() };
    stores(&plan.manifest) == stores(cur)
        && plan.manifest.etc == cur.etc
        && plan.etc.iter().all(|(n, body)| fs::read_to_string(&format!("{}/{}/etc/{}", GENERATIONS, cur.generation, n)).is_ok_and(|t| t == *body))
}

/// Writes a new generation directory from a plan; returns its manifest.
fn build(plan: Plan, config: &str) -> Result<Manifest> {
    // Numbers are never reused, even after `gc -d` deleted the newest.
    let last: u32 = fs::read_to_string(COUNTER).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
    let n = generations().last().map_or(0, |m| m.generation).max(current().unwrap_or(0)).max(last) + 1;
    let dir = format!("{}/{}", GENERATIONS, n);
    let tmp = format!("{}.tmp", dir);
    let _ = fs::remove_all(&tmp);
    ctx(fs::create_dir_all(&format!("{}/sw", tmp)), &tmp)?;
    write_atomic(COUNTER, format!("{}\n", n).as_bytes())?;

    let mut contents = Vec::new();
    for p in &plan.manifest.packages {
        contents.push((p.store.clone(), store_files(&p.store)?));
    }
    let profile = decl::merge_profile(&contents)?;
    for (rel, store) in &profile {
        let link = format!("{}/sw/{}", tmp, rel);
        if let Some((parent, _)) = link.rsplit_once('/') {
            ctx(fs::create_dir_all(parent), parent)?;
        }
        ctx(fs::symlink(&format!("{}/{}", store_path(store), rel), &link), &link)?;
    }
    for (name, body) in &plan.etc {
        let path = format!("{}/etc/{}", tmp, name);
        if let Some((parent, _)) = path.rsplit_once('/') {
            ctx(fs::create_dir_all(parent), parent)?;
        }
        ctx(fs::write(&path, body.as_bytes()), &path)?;
    }
    ctx(fs::write(&format!("{}/system.conf", tmp), config.as_bytes()), &tmp)?;
    let mut m = plan.manifest;
    m.generation = n;
    ctx(fs::write(&format!("{}/manifest", tmp), m.to_text().as_bytes()), &tmp)?;
    ctx(fs::rename(&tmp, &dir), &dir)?;
    Ok(m)
}

/// Makes generation `new` the running system.
fn activate(new: &Manifest, old: Option<&Manifest>) -> Result<()> {
    set_link(&format!("generations/{}", new.generation), SYSTEM)?;
    // /etc/NAME -> /pkg/system/etc/NAME. Links of the old generation keep
    // working (they go through /pkg/system), so only new names need links.
    for name in &new.etc {
        let path = format!("/etc/{}", name);
        let want = format!("{}/etc/{}", SYSTEM, name);
        match fs::symlink_metadata(&path) {
            Ok(st) if fs::is_symlink(&st) => {
                if fs::read_link(&path).is_ok_and(|t| t == want) {
                    continue;
                }
            }
            Ok(st) if fs::is_dir(&st) => return Err(format!("{}: is a directory", path)),
            Ok(_) => {
                // A file pkg did not create: keep it out of the way.
                let saved = format!("{}.before-pkg", path);
                if !fs::exists(&saved) {
                    ctx(fs::rename(&path, &saved), &path)?;
                    println!("  kept the old {} as {}", path, saved);
                }
            }
            Err(_) => {
                if let Some((parent, _)) = path.rsplit_once('/') {
                    ctx(fs::create_dir_all(parent), parent)?;
                }
            }
        }
        set_link(&want, &path)?;
    }
    for name in old.map_or(&[][..], |m| &m.etc[..]) {
        if new.etc.contains(name) {
            continue;
        }
        let path = format!("/etc/{}", name);
        if fs::read_link(&path).is_ok_and(|t| t == format!("{}/etc/{}", SYSTEM, name)) {
            let _ = fs::remove_file(&path);
            // No longer managed: the file from before pkg comes back.
            let _ = fs::rename(&format!("{}.before-pkg", path), &path);
        }
    }
    huldra_user::sys::sync();
    Ok(())
}

fn print_diff(old: Option<&Manifest>, new: &Manifest) {
    let d = decl::diff(old, new);
    for a in &d.added {
        println!("  + {}", a);
    }
    for (n, o, v) in &d.changed {
        println!("  ~ {} {} -> {}", n, o, v);
    }
    for r in &d.removed {
        println!("  - {}", r);
    }
}

/// Builds and activates the system described by `text`. Returns false if
/// it was already running.
fn switch_to(text: &str, dry_run: bool) -> Result<bool> {
    let cfg = parse_config(text)?;
    let plan = plan(&cfg)?;
    let cur = current_manifest();
    if cur.as_ref().is_some_and(|c| same_as(&plan, c)) {
        println!("the system is up to date (generation {})", cur.unwrap().generation);
        return Ok(false);
    }
    if dry_run {
        println!("would build a new generation:");
        print_diff(cur.as_ref(), &plan.manifest);
        return Ok(true);
    }
    let new = build(plan, text)?;
    activate(&new, cur.as_ref())?;
    print_diff(cur.as_ref(), &new);
    println!("switched to generation {}", new.generation);
    Ok(true)
}

fn switch(dry_run: bool) -> Result<()> {
    let _lock = Lock::acquire()?;
    switch_to(&config_text(), dry_run).map(drop)
}

/// `pkg add` / `pkg remove`: edit the configuration, switch, and keep the
/// edit only if the switch worked.
fn edit_packages(add: &[&str], remove: &[&str]) -> Result<()> {
    let _lock = Lock::acquire()?;
    let text = config_text();
    let mut packages = parse_config(&text)?.packages;
    for r in remove {
        let before = packages.len();
        packages.retain(|p| Request::parse(p).ok().and_then(|q| q.name().map(String::from)).as_deref() != Some(*r) && p != r);
        if packages.len() == before {
            return Err(format!("{} is not in {}", r, CONF));
        }
    }
    for a in add {
        Request::parse(a)?;
        if !packages.iter().any(|p| p == a) {
            packages.push(a.to_string());
        }
    }
    let new_text = decl::set_packages(&text, &packages);
    switch_to(&new_text, false)?;
    write_atomic(CONF, new_text.as_bytes())
}

fn rollback(to: Option<u32>) -> Result<()> {
    let _lock = Lock::acquire()?;
    let cur = current_manifest();
    let cur_n = cur.as_ref().map_or(0, |m| m.generation);
    let target = match to {
        Some(n) => manifest(n)?,
        None => generations()
            .into_iter()
            .filter(|m| m.generation < cur_n)
            .last()
            .ok_or("there is no older generation")?,
    };
    if target.generation == cur_n {
        println!("generation {} is already running", cur_n);
        return Ok(());
    }
    activate(&target, cur.as_ref())?;
    print_diff(cur.as_ref(), &target);
    println!("switched to generation {}", target.generation);
    println!("(/etc/system.conf is unchanged; that generation's is in {}/{}/system.conf)", GENERATIONS, target.generation);
    Ok(())
}

fn list_generations() {
    let cur = current();
    for m in generations() {
        let t = DateTime::from_unix(m.created);
        println!(
            "{} {:>3}  {}-{:02}-{:02} {:02}:{:02}  {} packages{}",
            if Some(m.generation) == cur { "*" } else { " " },
            m.generation,
            t.year,
            t.month,
            t.day,
            t.hour,
            t.minute,
            m.packages.len(),
            if Some(m.generation) == cur { "  (current)" } else { "" }
        );
    }
}

/// Bytes used by a tree (files only).
fn tree_size(path: &str) -> u64 {
    let Ok(st) = fs::symlink_metadata(path) else { return 0 };
    if !fs::is_dir(&st) {
        return st.st_size as u64;
    }
    fs::read_dir(path).unwrap_or_default().iter().map(|e| tree_size(&fs::join(path, &e.name))).sum()
}

fn gc(delete_old: bool) -> Result<()> {
    let _lock = Lock::acquire()?;
    let cur = current();
    if delete_old {
        for m in generations() {
            if Some(m.generation) != cur {
                ctx(fs::remove_all(&format!("{}/{}", GENERATIONS, m.generation)), GENERATIONS)?;
                println!("deleted generation {}", m.generation);
            }
        }
    }
    for e in fs::read_dir(GENERATIONS).unwrap_or_default() {
        if e.name.ends_with(".tmp") {
            let _ = fs::remove_all(&fs::join(GENERATIONS, &e.name));
        }
    }
    let store: Vec<String> = fs::read_dir(STORE).unwrap_or_default().into_iter().map(|e| e.name).collect();
    let dead = decl::unreferenced(&store, &generations());
    let mut freed = 0;
    for s in &dead {
        freed += tree_size(&store_path(s));
        ctx(fs::remove_all(&store_path(s)), s)?;
    }
    println!("removed {} store paths, {} KiB freed", dead.len(), freed / 1024);
    Ok(())
}

// ----------------------------------------------------------------- queries

fn search(word: Option<&str>) -> Result<()> {
    let idx = cached_index()?;
    let running = current_manifest();
    let mut names: Vec<&str> = idx.iter().map(|e| e.info.name.as_str()).collect();
    names.sort();
    names.dedup();
    for n in names {
        let e = pkg::find(&idx, n).unwrap();
        let matches = word.is_none_or(|w| e.info.name.contains(w) || e.info.description.to_lowercase().contains(&w.to_lowercase()));
        if matches {
            let mark = if running.as_ref().is_some_and(|m| m.packages.iter().any(|p| p.name == n)) { " [installed]" } else { "" };
            println!("{:<16} {:<10} {}{}", e.info.name, e.info.version, e.info.description, mark);
        }
    }
    Ok(())
}

fn info(name: &str) -> Result<()> {
    let running = current_manifest().and_then(|m| m.packages.into_iter().find(|p| p.name == name));
    let entry = cached_index().ok().and_then(|idx| pkg::find(&idx, name).cloned());
    let info = match (&running, &entry) {
        (Some(p), _) => store_info(&p.store).ok_or_else(|| format!("{}: store path missing", p.store))?,
        (None, Some(e)) => e.info.clone(),
        _ => return Err(format!("package '{}' not found", name)),
    };
    println!("Name:        {}", info.name);
    println!("Version:     {}", info.version);
    println!("Description: {}", info.description);
    println!("Depends:     {}", if info.depends.is_empty() { String::from("-") } else { info.depends.join(" ") });
    println!("Installed:   {}", running.as_ref().map_or(String::from("no"), |p| format!("{} in {}", p.version, store_path(&p.store))));
    if let Some(e) = entry {
        println!("Available:   {} from {} ({} bytes)", e.info.version, e.repo, e.size);
    }
    Ok(())
}

fn list() -> Result<()> {
    let m = current_manifest().ok_or("no generation is active yet: run 'pkg switch'")?;
    let wanted = parse_config(&config_text()).map(|c| c.packages).unwrap_or_default();
    for p in &m.packages {
        let explicit = wanted.iter().any(|w| Request::parse(w).ok().and_then(|r| r.name().map(String::from)).as_deref() == Some(p.name.as_str()));
        let desc = store_info(&p.store).map(|i| i.description).unwrap_or_default();
        println!("{:<16} {:<10} {}{}", p.name, p.version, desc, if explicit { "" } else { " (dependency)" });
    }
    Ok(())
}

fn files(name: &str) -> Result<()> {
    let m = current_manifest().ok_or("no generation is active yet")?;
    let p = m.packages.iter().find(|p| p.name == name).ok_or_else(|| format!("{} is not installed", name))?;
    for f in store_files(&p.store)? {
        println!("{}/sw/{}", SYSTEM, f);
    }
    Ok(())
}

fn usage() -> i32 {
    eprintln!(
        "usage: pkg switch [-n] | add NAME... | remove NAME... | rollback [N] | generations | gc [-d]\n           \
         | update | search [WORD] | info NAME | list | files NAME\n\
         The system is described by {}.",
        CONF
    );
    2
}

fn main() -> i32 {
    let args = env::args();
    let Some(cmd) = args.get(1) else { return usage() };
    let rest: Vec<&str> = args[2..].iter().map(|s| s.as_str()).collect();
    let r = match (cmd.as_str(), &rest[..]) {
        ("switch", []) => switch(false),
        ("switch", ["-n"]) => switch(true),
        ("add" | "install", names) if !names.is_empty() => edit_packages(names, &[]),
        ("remove" | "rm", names) if !names.is_empty() => edit_packages(&[], names),
        ("rollback", []) => rollback(None),
        ("rollback", [n]) => match n.parse() {
            Ok(n) => rollback(Some(n)),
            Err(_) => return usage(),
        },
        ("generations", []) => {
            list_generations();
            Ok(())
        }
        ("gc", []) => gc(false),
        ("gc", ["-d"]) => gc(true),
        ("update", []) => parse_config(&config_text()).and_then(|c| update(&c)).map(drop),
        ("search", []) => search(None),
        ("search", [w]) => search(Some(w)),
        ("info" | "show", [n]) => info(n),
        ("list", []) => list(),
        ("files", [n]) => files(n),
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
