//! git COMMAND [args]: version control, compatible with git repositories
//! and servers.
//!
//!   init [DIR]                       create a repository
//!   clone [--depth N] URL [DIR]      copy a repository over HTTP(S)
//!   status [-s]                      what changed
//!   add PATH... | -A                 stage files (. for everything)
//!   rm [--cached] PATH...            remove files
//!   restore [--staged] PATH...       undo changes to files
//!   commit [-a] [-m MSG]             record the staged changes
//!   log [--oneline] [-n N] [REV]     history
//!   diff [--cached] [PATH...]        changes as a patch
//!   show [REV]                       a commit and its patch
//!   branch [-a] [-d] [NAME [REV]]    list, create or delete branches
//!   checkout [-b] BRANCH|REV         switch branches (switch [-c] too)
//!   reset [--hard] [REV]             move the branch, reset the index
//!   fetch | pull                     get new commits (pull: fast-forward)
//!   push [-f] [REMOTE [BRANCH]]      send commits to the server
//!   tag [NAME [REV]]                 list or create tags
//!   remote [-v] | remote add NAME URL
//!   config [--global] KEY [VALUE]    settings (user.name, user.email)
//!   rev-parse REV | cat-file -p|-t ID
//!
//! Remotes are smart HTTP or HTTPS servers (GitHub, GitLab, git
//! http-backend). For pushing put a token in the URL
//! (https://USER:TOKEN@github.com/u/r.git) or in ~/.git-credentials.
//! Merging diverged branches is not supported yet: pull fast-forwards.

#![no_std]
#![no_main]

extern crate alloc;

mod remote;
mod repo;
mod worktree;

use alloc::collections::{BTreeMap, BTreeSet};
use huldra_git::object::{Commit, Id, Kind, Signature};
use huldra_git::{diff, pack};
use remote::Remote;
use repo::{ctx, Ignore, Repo, Result};
use huldra_user::{env, eprintln, format, fs, print, println, process, term, time, String, ToString, Vec};

huldra_user::main!(main);

// ------------------------------------------------------------------- output

fn color() -> bool {
    term::is_tty(huldra_user::io::STDOUT)
}

fn paint(code: &str, s: &str) -> String {
    if color() { format!("\x1b[{}m{}\x1b[0m", code, s) } else { s.to_string() }
}

/// Prints text, through less when it is longer than the screen.
fn page(text: &str) {
    let (rows, _) = term::size();
    if !color() || text.lines().count() < rows.saturating_sub(1) {
        print!("{}", text);
        return;
    }
    let tmp = format!("/tmp/git-{}", process::getpid());
    if fs::write(&tmp, text.as_bytes()).is_ok() {
        let _ = process::run("/bin/less", &["less", &tmp]);
        let _ = fs::remove_file(&tmp);
    } else {
        print!("{}", text);
    }
}

fn date(sig: &Signature) -> String {
    let t = time::DateTime::from_unix(sig.time + sig.tz as i64 * 60);
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let a = sig.tz.abs();
    format!(
        "{} {} {} {:02}:{:02}:{:02} {} {}{:02}{:02}",
        DAYS[t.weekday as usize % 7],
        time::MONTHS[(t.month - 1) as usize],
        t.day,
        t.hour,
        t.minute,
        t.second,
        t.year,
        if sig.tz < 0 { '-' } else { '+' },
        a / 60,
        a % 60
    )
}

// ------------------------------------------------------------------ helpers

fn signature(repo: &Repo) -> Signature {
    let name = env::var("GIT_AUTHOR_NAME").map(String::from).or_else(|| repo.config("user.name")).unwrap_or_else(|| "root".into());
    let host = fs::read_to_string("/etc/hostname").unwrap_or_else(|_| "huldra".into());
    let email = env::var("GIT_AUTHOR_EMAIL").map(String::from).or_else(|| repo.config("user.email")).unwrap_or_else(|| format!("root@{}", host.trim()));
    Signature { name, email, time: time::now(), tz: 0 }
}

/// Commits reachable from `tips`, newest first, not going past `stop`
/// or shallow boundaries.
fn history(repo: &Repo, tips: &[Id], stop: &BTreeSet<Id>, limit: usize) -> Result<Vec<(Id, Commit)>> {
    let shallow: BTreeSet<Id> = repo.shallow().into_iter().collect();
    let mut seen: BTreeSet<Id> = BTreeSet::new();
    let mut queue: Vec<(Id, Commit)> = Vec::new();
    for t in tips {
        if !stop.contains(t) && seen.insert(*t) {
            queue.push((*t, repo.commit(t)?));
        }
    }
    let mut out = Vec::new();
    while !queue.is_empty() && out.len() < limit {
        // The newest pending commit next.
        let i = (0..queue.len()).max_by_key(|&i| queue[i].1.committer.time).unwrap();
        let (id, c) = queue.swap_remove(i);
        if !shallow.contains(&id) {
            for p in &c.parents {
                if !stop.contains(p) && seen.insert(*p) && repo.has_object(p) {
                    queue.push((*p, repo.commit(p)?));
                }
            }
        }
        out.push((id, c));
    }
    Ok(out)
}

fn is_ancestor(repo: &Repo, ancestor: &Id, of: &Id) -> Result<bool> {
    if ancestor == of {
        return Ok(true);
    }
    Ok(history(repo, &[*of], &BTreeSet::new(), usize::MAX)?.iter().any(|(id, _)| id == ancestor))
}

/// Every object reachable from commits (commits, trees, blobs).
fn objects_of(repo: &Repo, commits: &[Id], out: &mut BTreeSet<Id>) -> Result<()> {
    fn tree(repo: &Repo, id: &Id, out: &mut BTreeSet<Id>) -> Result<()> {
        if !out.insert(*id) {
            return Ok(());
        }
        for e in repo.tree(id)? {
            if e.is_tree() {
                tree(repo, &e.id, out)?;
            } else if e.mode != huldra_git::object::MODE_GITLINK {
                out.insert(e.id);
            }
        }
        Ok(())
    }
    for c in commits {
        out.insert(*c);
        tree(repo, &repo.commit(c)?.tree, out)?;
    }
    Ok(())
}

fn current_branch(repo: &Repo) -> Option<String> {
    repo.head_ref().and_then(|r| r.strip_prefix("refs/heads/").map(String::from))
}

/// Decorations for log: "HEAD -> main, origin/main, tag: v1".
fn decorations(repo: &Repo) -> BTreeMap<Id, Vec<String>> {
    let mut map: BTreeMap<Id, Vec<String>> = BTreeMap::new();
    let head_branch = current_branch(repo);
    if let (Some(id), None) = (repo.head(), &head_branch) {
        map.entry(id).or_default().push("HEAD".into());
    }
    for (name, id) in repo.refs("refs/") {
        let id = repo.peel(id);
        let short = if let Some(b) = name.strip_prefix("refs/heads/") {
            if head_branch.as_deref() == Some(b) { format!("HEAD -> {}", b) } else { b.to_string() }
        } else if let Some(r) = name.strip_prefix("refs/remotes/") {
            r.to_string()
        } else if let Some(t) = name.strip_prefix("refs/tags/") {
            format!("tag: {}", t)
        } else {
            continue;
        };
        map.entry(id).or_default().push(short);
    }
    for v in map.values_mut() {
        v.sort_by_key(|s| !s.starts_with("HEAD"));
    }
    map
}

// ----------------------------------------------------------------- commands

fn init(args: &[&str]) -> Result<()> {
    let dir = args.first().copied().unwrap_or(".");
    ctx(fs::create_dir_all(dir), dir)?;
    let abs = if dir.starts_with('/') { dir.to_string() } else { fs::join(&fs::current_dir().unwrap_or_default(), dir) };
    let existed = fs::exists(&fs::join(&abs, ".git"));
    Repo::init(&abs, "main")?;
    println!("{} Git repository in {}/.git/", if existed { "Reinitialized existing" } else { "Initialized empty" }, abs.trim_end_matches("/."));
    Ok(())
}

/// Fetches from `remote_name` into refs/remotes/NAME/*; returns the advertisement.
fn fetch_into(repo: &Repo, remote_name: &str, url: &str, depth: Option<u32>) -> Result<huldra_git::protocol::Advertisement> {
    let remote = Remote::new(url)?;
    let adv = remote.discover("git-upload-pack")?;
    let mut wants: Vec<Id> = Vec::new();
    for (name, id) in &adv.refs {
        if (name.starts_with("refs/heads/") || name.starts_with("refs/tags/")) && !name.ends_with("^{}") && !repo.has_object(id) && !wants.contains(id) {
            wants.push(*id);
        }
    }
    if !wants.is_empty() {
        let mut haves: Vec<Id> = repo.refs("refs/").into_iter().map(|(_, id)| id).filter(|id| repo.has_object(id)).collect();
        haves.sort();
        haves.dedup();
        let start = time::uptime_ms();
        let resp = remote.fetch(&adv, &wants, &haves, depth)?;
        let objects = pack::parse(&resp.pack, &|id| repo.read_object(id)).map_err(|e| e.0)?;
        repo.store_pack(&resp.pack, &objects)?;
        let secs = (time::uptime_ms() - start).max(1);
        println!("Received {} objects, {} KiB in {}.{}s", objects.len(), resp.pack.len() / 1024, secs / 1000, secs % 1000 / 100);
        if !resp.shallow.is_empty() {
            let mut all = repo.shallow();
            all.extend(resp.shallow);
            all.sort();
            all.dedup();
            let text: String = all.iter().map(|id| format!("{}\n", id)).collect();
            repo::write_atomic(&repo.path("shallow"), text.as_bytes())?;
        }
    }
    for (name, id) in &adv.refs {
        if let Some(b) = name.strip_prefix("refs/heads/") {
            let local = format!("refs/remotes/{}/{}", remote_name, b);
            let old = repo.read_ref(&local);
            if old != Some(*id) {
                repo.write_ref(&local, id)?;
                match old {
                    Some(o) => println!("   {}..{}  {} -> {}/{}", o.short(), id.short(), b, remote_name, b),
                    None => println!(" * [new branch]      {} -> {}/{}", b, remote_name, b),
                }
            }
        } else if name.starts_with("refs/tags/") && !name.ends_with("^{}") && repo.read_ref(name).is_none() && repo.has_object(id) {
            repo.write_ref(name, id)?;
        }
    }
    Ok(adv)
}

fn clone(args: &[&str]) -> Result<()> {
    let mut depth = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--depth" => {
                i += 1;
                depth = Some(args.get(i).and_then(|d| d.parse().ok()).ok_or("--depth needs a number")?);
            }
            a => rest.push(a),
        }
        i += 1;
    }
    let url = *rest.first().ok_or("usage: git clone [--depth N] URL [DIR]")?;
    let dir = match rest.get(1) {
        Some(d) => d.to_string(),
        None => url.trim_end_matches('/').rsplit('/').next().unwrap_or("repo").trim_end_matches(".git").to_string(),
    };
    if fs::read_dir(&dir).is_ok_and(|d| !d.is_empty()) {
        return Err(format!("destination '{}' already exists and is not empty", dir));
    }
    println!("Cloning into '{}'...", dir);
    ctx(fs::create_dir_all(&dir), &dir)?;
    let abs = if dir.starts_with('/') { dir.clone() } else { fs::join(&fs::current_dir().unwrap_or_default(), &dir) };
    let repo = Repo::init(&abs, "main")?;
    repo.set_config("remote.origin.url", url)?;
    repo.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*")?;
    let result = fetch_into(&repo, "origin", url, depth);
    let adv = match result {
        Ok(a) => a,
        Err(e) => {
            let _ = fs::remove_all(&abs);
            return Err(e);
        }
    };
    let branch = adv
        .head_target()
        .and_then(|t| t.strip_prefix("refs/heads/"))
        .map(String::from)
        .or_else(|| adv.refs.iter().find_map(|(n, _)| n.strip_prefix("refs/heads/").map(String::from)));
    let Some(branch) = branch else {
        println!("warning: You appear to have cloned an empty repository.");
        return Ok(());
    };
    let tip = adv.get(&format!("refs/heads/{}", branch)).ok_or("the default branch is missing")?;
    repo.write_ref(&format!("refs/heads/{}", branch), &tip)?;
    repo.set_head_branch(&branch)?;
    repo.set_config(&format!("branch.{}.remote", branch), "origin")?;
    repo.set_config(&format!("branch.{}.merge", branch), &format!("refs/heads/{}", branch))?;
    let n = worktree::checkout(&repo, None, tip)?;
    println!("Checked out '{}' ({} files)", branch, n);
    Ok(())
}

fn status(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let s = worktree::status(&repo)?;
    if args.contains(&"-s") || args.contains(&"--short") {
        for (p, what) in &s.staged {
            let c = match *what { "new file" => 'A', "deleted" => 'D', _ => 'M' };
            let un = s.unstaged.iter().find(|(q, _)| q == p).map_or(' ', |(_, w)| if *w == "deleted" { 'D' } else { 'M' });
            println!("{}{} {}", paint("32", &c.to_string()), paint("31", &un.to_string()), p);
        }
        for (p, what) in &s.unstaged {
            if !s.staged.iter().any(|(q, _)| q == p) {
                println!(" {} {}", paint("31", if *what == "deleted" { "D" } else { "M" }), p);
            }
        }
        for p in &s.untracked {
            println!("{} {}", paint("31", "??"), p);
        }
        return Ok(());
    }
    match current_branch(&repo) {
        Some(b) => {
            println!("On branch {}", b);
            if let (Some(local), Some(up)) = (repo.head(), repo.read_ref(&format!("refs/remotes/origin/{}", b))) {
                if local == up {
                    println!("Your branch is up to date with 'origin/{}'.", b);
                } else {
                    let ahead = history(&repo, &[local], &[up].into_iter().collect(), usize::MAX)?.len();
                    let behind = history(&repo, &[up], &[local].into_iter().collect(), usize::MAX)?.len();
                    match (ahead, behind) {
                        (a, 0) => println!("Your branch is ahead of 'origin/{}' by {} commit{}.", b, a, if a == 1 { "" } else { "s" }),
                        (0, b2) => println!("Your branch is behind 'origin/{}' by {} commit{}.", b, b2, if b2 == 1 { "" } else { "s" }),
                        (a, b2) => println!("Your branch and 'origin/{}' have diverged ({} and {} different commits).", b, a, b2),
                    }
                }
            }
        }
        None => println!("HEAD detached at {}", repo.head().map(|i| i.short()).unwrap_or_default()),
    }
    if repo.head().is_none() {
        println!("\nNo commits yet");
    }
    if !s.staged.is_empty() {
        println!("\nChanges to be committed:");
        for (p, w) in &s.staged {
            println!("\t{}", paint("32", &format!("{:<12}{}", format!("{}:", w), p)));
        }
    }
    if !s.unstaged.is_empty() {
        println!("\nChanges not staged for commit:");
        for (p, w) in &s.unstaged {
            println!("\t{}", paint("31", &format!("{:<12}{}", format!("{}:", w), p)));
        }
    }
    if !s.untracked.is_empty() {
        println!("\nUntracked files:");
        for p in &s.untracked {
            println!("\t{}", paint("31", p));
        }
    }
    if s.clean() && s.untracked.is_empty() {
        println!("nothing to commit, working tree clean");
    } else if s.staged.is_empty() {
        println!("\nno changes added to commit (use \"git add\" and/or \"git commit -a\")");
    }
    Ok(())
}

/// Paths relative to the repository root for command-line arguments.
fn rel_paths(repo: &Repo, args: &[&str]) -> Result<Vec<String>> {
    let cwd = fs::current_dir().unwrap_or_default();
    let mut out = Vec::new();
    for a in args {
        let abs = if a.starts_with('/') { a.to_string() } else { fs::join(&cwd, a) };
        // Normalize "." and ".." lexically.
        let mut parts: Vec<&str> = Vec::new();
        for c in abs.split('/') {
            match c {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                c => parts.push(c),
            }
        }
        let norm = format!("/{}", parts.join("/"));
        let rel = norm.strip_prefix(repo.work.trim_end_matches('/')).ok_or_else(|| format!("{}: outside the repository", a))?;
        out.push(rel.trim_start_matches('/').to_string());
    }
    Ok(out)
}

fn under(path: &str, prefix: &str) -> bool {
    prefix.is_empty() || path == prefix || path.starts_with(&format!("{}/", prefix))
}

fn add(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let all = args.iter().any(|a| *a == "-A" || *a == "--all");
    let paths: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with('-')).collect();
    if paths.is_empty() && !all {
        return Err("nothing specified, nothing added (git add PATH... or git add -A)".into());
    }
    let prefixes = if all { alloc::vec![String::new()] } else { rel_paths(&repo, &paths)? };
    let ignore = Ignore::load(&repo);
    let mut index = repo.read_index()?;
    let files = worktree::walk(&repo, &ignore);
    for prefix in &prefixes {
        let mut matched = false;
        let explicit = !prefix.is_empty() && fs::symlink_metadata(&repo.work_path(prefix)).is_ok_and(|s| !fs::is_dir(&s));
        let mut candidates: Vec<String> = files.iter().filter(|f| under(f, prefix)).cloned().collect();
        if explicit && !candidates.contains(prefix) {
            candidates.push(prefix.clone()); // named explicitly: even if ignored
        }
        for f in candidates {
            matched = true;
            let full = repo.work_path(&f);
            let st = ctx(fs::symlink_metadata(&full), &full)?;
            let id = repo.write_object(Kind::Blob, &worktree::read_content(&full, &st)?)?;
            index.retain(|e| e.path != f);
            index.push(worktree::entry_for(&f, &st, id));
        }
        // Tracked files that are gone.
        let before = index.len();
        index.retain(|e| !under(&e.path, prefix) || fs::symlink_metadata(&repo.work_path(&e.path)).is_ok());
        matched |= index.len() != before;
        if !matched && !prefix.is_empty() {
            return Err(format!("pathspec '{}' did not match any files", prefix));
        }
    }
    repo.write_index(&index)
}

fn rm(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let cached = args.contains(&"--cached");
    let paths: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with('-')).collect();
    let mut index = repo.read_index()?;
    for p in rel_paths(&repo, &paths)? {
        let before = index.len();
        let gone: Vec<String> = index.iter().filter(|e| under(&e.path, &p)).map(|e| e.path.clone()).collect();
        index.retain(|e| !under(&e.path, &p));
        if index.len() == before {
            return Err(format!("pathspec '{}' did not match any files", p));
        }
        for g in gone {
            if !cached {
                let _ = fs::remove_file(&repo.work_path(&g));
            }
            println!("rm '{}'", g);
        }
    }
    repo.write_index(&index)
}

fn restore(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let staged = args.contains(&"--staged");
    let paths: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with('-')).collect();
    let mut index = repo.read_index()?;
    let head = worktree::tree_files(&repo, repo.head())?;
    for p in rel_paths(&repo, &paths)? {
        if staged {
            index.retain(|e| !under(&e.path, &p));
            for (path, (mode, id)) in head.iter().filter(|(path, _)| under(path, &p)) {
                let full = repo.work_path(path);
                let mut e = match fs::symlink_metadata(&full) {
                    Ok(st) => worktree::entry_for(path, &st, *id),
                    Err(_) => huldra_git::index::Entry { path: path.clone(), id: *id, ..Default::default() },
                };
                e.mode = *mode;
                e.mtime = (0, 0); // force a content check
                index.push(e);
            }
        } else {
            let mut any = false;
            for e in index.iter().filter(|e| under(&e.path, &p)) {
                worktree::restore(&repo, e)?;
                any = true;
            }
            if !any {
                return Err(format!("pathspec '{}' did not match any file known to git", p));
            }
        }
    }
    if staged {
        repo.write_index(&index)?;
    } else {
        // Refresh stat data of restored files.
        let mut fresh = Vec::new();
        for e in index {
            match fs::symlink_metadata(&repo.work_path(&e.path)) {
                Ok(st) => {
                    let mut n = worktree::entry_for(&e.path, &st, e.id);
                    n.mode = e.mode;
                    fresh.push(n);
                }
                Err(_) => fresh.push(e),
            }
        }
        repo.write_index(&fresh)?;
    }
    Ok(())
}

fn edit_message(repo: &Repo) -> Result<String> {
    let path = repo.path("COMMIT_EDITMSG");
    ctx(fs::write(&path, b"\n# Write the commit message above. Lines starting with '#' are ignored;\n# an empty message aborts the commit.\n"), &path)?;
    let editor = env::var("EDITOR").unwrap_or("/bin/edit");
    let _ = process::run(editor, &[editor, &path]);
    let text = fs::read_to_string(&path).unwrap_or_default();
    Ok(text.lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>().join("\n").trim().to_string())
}

fn commit(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let mut messages: Vec<String> = Vec::new();
    let mut all = false;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "-m" => {
                i += 1;
                messages.push(args.get(i).ok_or("-m needs a message")?.to_string());
            }
            "-a" | "--all" => all = true,
            "-am" => {
                all = true;
                i += 1;
                messages.push(args.get(i).ok_or("-m needs a message")?.to_string());
            }
            other => return Err(format!("unknown option {}", other)),
        }
        i += 1;
    }
    let mut index = repo.read_index()?;
    if all {
        let mut kept = Vec::new();
        for e in index {
            let full = repo.work_path(&e.path);
            match fs::symlink_metadata(&full) {
                Err(_) => {}
                Ok(st) => {
                    let id = match worktree::changed(&repo, &e)? {
                        Some(id) if id != e.id => repo.write_object(Kind::Blob, &worktree::read_content(&full, &st)?)?,
                        _ => e.id,
                    };
                    kept.push(worktree::entry_for(&e.path, &st, id));
                }
            }
        }
        index = kept;
        repo.write_index(&index)?;
    }
    let tree = worktree::write_tree(&repo, &index)?;
    let parent = repo.head();
    if let Some(p) = parent {
        if repo.commit(&p)?.tree == tree {
            status(&[])?;
            return Err("nothing to commit".into());
        }
    } else if index.is_empty() {
        return Err("nothing to commit (create files and use \"git add\")".into());
    }
    let message = if messages.is_empty() { edit_message(&repo)? } else { messages.join("\n\n") };
    if message.trim().is_empty() {
        return Err("aborting commit due to empty commit message".into());
    }
    let sig = signature(&repo);
    let c = Commit { tree, parents: parent.into_iter().collect(), author: sig.clone(), committer: sig, extra: Vec::new(), message: format!("{}\n", message.trim_end()) };
    let id = repo.write_object(Kind::Commit, &c.to_bytes())?;
    repo.update_head(&id)?;
    let changed = {
        let old = worktree::tree_files(&repo, parent)?;
        let new = worktree::tree_files(&repo, Some(id))?;
        old.keys().filter(|k| !new.contains_key(*k)).count() + new.iter().filter(|(k, v)| old.get(*k) != Some(v)).count()
    };
    println!(
        "[{}{} {}] {}\n {} file{} changed",
        current_branch(&repo).unwrap_or_else(|| "detached HEAD".into()),
        if parent.is_none() { " (root-commit)" } else { "" },
        id.short(),
        c.summary(),
        changed,
        if changed == 1 { "" } else { "s" }
    );
    Ok(())
}

fn format_commit(id: &Id, c: &Commit, deco: &BTreeMap<Id, Vec<String>>, out: &mut String) {
    let d = deco.get(id).map(|v| format!(" ({})", v.join(", "))).unwrap_or_default();
    out.push_str(&paint("33", &format!("commit {}", id)));
    out.push_str(&paint("1;36", &d));
    out.push('\n');
    if c.parents.len() > 1 {
        out.push_str(&format!("Merge: {}\n", c.parents.iter().map(|p| p.short()).collect::<Vec<_>>().join(" ")));
    }
    out.push_str(&format!("Author: {} <{}>\nDate:   {}\n\n", c.author.name, c.author.email, date(&c.author)));
    for l in c.message.trim_end().lines() {
        out.push_str(&format!("    {}\n", l));
    }
}

fn log(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let mut oneline = false;
    let mut limit = usize::MAX;
    let mut revs = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--oneline" => oneline = true,
            "-n" => {
                i += 1;
                limit = args.get(i).and_then(|n| n.parse().ok()).ok_or("-n needs a number")?;
            }
            a if a.starts_with('-') && a[1..].parse::<usize>().is_ok() => limit = a[1..].parse().unwrap(),
            a => revs.push(repo.rev_parse(a)?),
        }
        i += 1;
    }
    if revs.is_empty() {
        revs.push(repo.head().ok_or("your current branch does not have any commits yet")?);
    }
    let deco = decorations(&repo);
    let mut out = String::new();
    for (id, c) in history(&repo, &revs, &BTreeSet::new(), limit)? {
        if oneline {
            let d = deco.get(&id).map(|v| format!(" ({})", v.join(", "))).unwrap_or_default();
            out.push_str(&format!("{}{} {}\n", paint("33", &id.short()), paint("1;36", &d), c.summary()));
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            format_commit(&id, &c, &deco, &mut out);
        }
    }
    page(&out);
    Ok(())
}

/// A patch between two sets of files: path -> (mode, id, content).
fn patch(repo: &Repo, old: &BTreeMap<String, (u32, Id)>, new: &BTreeMap<String, (u32, Id, Option<Vec<u8>>)>, filter: &[String]) -> Result<String> {
    let mut out = String::new();
    let mut paths: BTreeSet<&String> = old.keys().collect();
    paths.extend(new.keys());
    for p in paths {
        if !filter.is_empty() && !filter.iter().any(|f| under(p, f)) {
            continue;
        }
        let a = old.get(p);
        let b = new.get(p);
        if let (Some((ma, ia)), Some((mb, ib, _))) = (a, b) {
            if ia == ib && ma == mb {
                continue;
            }
        }
        let blob = |id: &Id| repo.object(id, Kind::Blob);
        let old_data = match a { Some((_, id)) => blob(id)?, None => Vec::new() };
        let new_data = match b { Some((_, _, Some(d))) => d.clone(), Some((_, id, None)) => blob(id)?, None => Vec::new() };
        out.push_str(&paint("1", &format!("diff --git a/{} b/{}", p, p)));
        out.push('\n');
        match (a, b) {
            (None, Some((m, _, _))) => out.push_str(&paint("1", &format!("new file mode {:o}\n", m))),
            (Some((m, _)), None) => out.push_str(&paint("1", &format!("deleted file mode {:o}\n", m))),
            (Some((ma, _)), Some((mb, _, _))) if ma != mb => out.push_str(&format!("old mode {:o}\nnew mode {:o}\n", ma, mb)),
            _ => {}
        }
        let (ta, tb) = (worktree::text_of(&old_data), worktree::text_of(&new_data));
        let (Some(ta), Some(tb)) = (ta, tb) else {
            out.push_str(&format!("Binary files a/{} and b/{} differ\n", p, p));
            continue;
        };
        let an = if a.is_some() { format!("a/{}", p) } else { "/dev/null".into() };
        let bn = if b.is_some() { format!("b/{}", p) } else { "/dev/null".into() };
        for line in diff::unified(&an, &bn, &ta, &tb, 3).lines() {
            let colored = match line.chars().next() {
                _ if line.starts_with("---") || line.starts_with("+++") => paint("1", line),
                Some('@') => paint("36", line),
                Some('+') => paint("32", line),
                Some('-') => paint("31", line),
                _ => line.to_string(),
            };
            out.push_str(&colored);
            out.push('\n');
        }
    }
    Ok(out)
}

fn index_files(repo: &Repo) -> Result<BTreeMap<String, (u32, Id)>> {
    Ok(repo.read_index()?.into_iter().map(|e| (e.path, (e.mode, e.id))).collect())
}

fn diff_cmd(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let cached = args.iter().any(|a| *a == "--cached" || *a == "--staged");
    let paths: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with('-')).collect();
    let filter = rel_paths(&repo, &paths)?;
    let index = index_files(&repo)?;
    let text = if cached {
        let head = worktree::tree_files(&repo, repo.head())?;
        let new = index.into_iter().map(|(p, (m, id))| (p, (m, id, None))).collect();
        patch(&repo, &head, &new, &filter)?
    } else {
        let mut new = BTreeMap::new();
        for e in repo.read_index()? {
            let full = repo.work_path(&e.path);
            if let Ok(st) = fs::symlink_metadata(&full) {
                match worktree::changed(&repo, &e)? {
                    Some(id) if id != e.id => {
                        new.insert(e.path.clone(), (worktree::mode_of(&st), id, Some(worktree::read_content(&full, &st)?)));
                    }
                    _ => {
                        new.insert(e.path.clone(), (e.mode, e.id, None));
                    }
                }
            }
        }
        patch(&repo, &index, &new, &filter)?
    };
    page(&text);
    Ok(())
}

fn show(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let id = repo.rev_parse(args.first().copied().unwrap_or("HEAD"))?;
    let c = repo.commit(&id)?;
    let mut out = String::new();
    format_commit(&id, &c, &decorations(&repo), &mut out);
    out.push('\n');
    let old = worktree::tree_files(&repo, c.parents.first().copied().filter(|p| repo.has_object(p)))?;
    let new = worktree::tree_files(&repo, Some(id))?.into_iter().map(|(p, (m, i))| (p, (m, i, None))).collect();
    out.push_str(&patch(&repo, &old, &new, &[])?);
    page(&out);
    Ok(())
}

fn branch(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let current = current_branch(&repo);
    match args {
        [] | ["-a"] | ["-v"] => {
            for (name, id) in repo.refs("refs/heads/") {
                let b = name.trim_start_matches("refs/heads/");
                let c = repo.commit(&id).map(|c| c.summary().to_string()).unwrap_or_default();
                if current.as_deref() == Some(b) {
                    println!("* {} {} {}", paint("32", &format!("{:<16}", b)), id.short(), c);
                } else {
                    println!("  {:<16} {} {}", b, id.short(), c);
                }
            }
            if args == ["-a"] {
                for (name, id) in repo.refs("refs/remotes/") {
                    println!("  {} {}", paint("31", &format!("{:<16}", name.trim_start_matches("refs/"))), id.short());
                }
            }
            Ok(())
        }
        ["-d" | "-D", name] => {
            if current.as_deref() == Some(*name) {
                return Err(format!("cannot delete the current branch '{}'", name));
            }
            let r = format!("refs/heads/{}", name);
            let id = repo.read_ref(&r).ok_or_else(|| format!("branch '{}' not found", name))?;
            repo.delete_ref(&r)?;
            println!("Deleted branch {} (was {}).", name, id.short());
            Ok(())
        }
        [name] | [name, _] => {
            let start = repo.rev_parse(args.get(1).copied().unwrap_or("HEAD"))?;
            let r = format!("refs/heads/{}", name);
            if repo.read_ref(&r).is_some() {
                return Err(format!("a branch named '{}' already exists", name));
            }
            repo.write_ref(&r, &start)
        }
        _ => Err("usage: git branch [-a] [-d NAME] [NAME [REV]]".into()),
    }
}

fn switch_to(repo: &Repo, target: Id, branch: Option<&str>) -> Result<()> {
    let current = repo.head();
    let tree_changes = match current {
        Some(c) => repo.commit(&c)?.tree != repo.commit(&target)?.tree,
        None => true,
    };
    if tree_changes {
        let s = worktree::status(repo)?;
        if !s.clean() {
            return Err("your local changes would be overwritten; commit them first (or git restore)".into());
        }
        worktree::checkout(repo, current, target)?;
    }
    match branch {
        Some(b) => {
            repo.set_head_branch(b)?;
            println!("Switched to branch '{}'", b);
        }
        None => {
            repo.set_head_detached(&target)?;
            println!("HEAD is now at {} (detached)", target.short());
        }
    }
    Ok(())
}

fn checkout(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    match args {
        ["-b" | "-c", name] | ["-b" | "-c", name, _] => {
            let start = repo.rev_parse(args.get(2).copied().unwrap_or("HEAD"))?;
            let r = format!("refs/heads/{}", name);
            if repo.read_ref(&r).is_some() {
                return Err(format!("a branch named '{}' already exists", name));
            }
            repo.write_ref(&r, &start)?;
            switch_to(&repo, start, Some(name))
        }
        ["--", paths @ ..] => restore(paths),
        [target] => {
            if let Some(id) = repo.read_ref(&format!("refs/heads/{}", target)) {
                return switch_to(&repo, id, Some(target));
            }
            if let Some(id) = repo.read_ref(&format!("refs/remotes/origin/{}", target)) {
                repo.write_ref(&format!("refs/heads/{}", target), &id)?;
                repo.set_config(&format!("branch.{}.remote", target), "origin")?;
                repo.set_config(&format!("branch.{}.merge", target), &format!("refs/heads/{}", target))?;
                println!("branch '{}' set up to track 'origin/{}'.", target, target);
                return switch_to(&repo, id, Some(target));
            }
            match repo.rev_parse(target) {
                Ok(id) => switch_to(&repo, repo.peel(id), None),
                Err(_) => restore(&[target]),
            }
        }
        _ => Err("usage: git checkout [-b] BRANCH|REV, or git checkout -- PATH...".into()),
    }
}

fn reset(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let hard = args.contains(&"--hard");
    let rev = args.iter().find(|a| !a.starts_with('-')).copied().unwrap_or("HEAD");
    let target = repo.rev_parse(rev)?;
    let current = repo.head();
    if hard {
        worktree::checkout(&repo, current, target)?;
        // Files changed but not committed come back too.
        for e in repo.read_index()? {
            if worktree::changed(&repo, &e)? != Some(e.id) {
                worktree::restore(&repo, &e)?;
            }
        }
        let c = repo.commit(&target)?;
        repo.update_head(&target)?;
        println!("HEAD is now at {} {}", target.short(), c.summary());
    } else {
        // Mixed: the index follows the commit, files stay.
        let files = worktree::tree_files(&repo, Some(target))?;
        let mut index = Vec::new();
        for (p, (m, id)) in files {
            let mut e = huldra_git::index::Entry { path: p, id, mode: m, ..Default::default() };
            if let Ok(st) = fs::symlink_metadata(&repo.work_path(&e.path)) {
                e = worktree::entry_for(&e.path, &st, id);
                e.mode = m;
                e.mtime = (0, 0);
            }
            index.push(e);
        }
        repo.write_index(&index)?;
        repo.update_head(&target)?;
    }
    Ok(())
}

fn remote_url(repo: &Repo, name: &str) -> Result<String> {
    repo.config(&format!("remote.{}.url", name)).ok_or_else(|| format!("no remote named '{}' (git remote add {} URL)", name, name))
}

fn fetch(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let name = args.first().copied().unwrap_or("origin");
    fetch_into(&repo, name, &remote_url(&repo, name)?, None)?;
    Ok(())
}

fn pull(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let branch = current_branch(&repo).ok_or("not on a branch")?;
    let remote_name = repo.config(&format!("branch.{}.remote", branch)).unwrap_or_else(|| args.first().unwrap_or(&"origin").to_string());
    fetch_into(&repo, &remote_name, &remote_url(&repo, &remote_name)?, None)?;
    let upstream = repo.read_ref(&format!("refs/remotes/{}/{}", remote_name, branch)).ok_or_else(|| format!("the remote has no branch '{}'", branch))?;
    let local = repo.head();
    match local {
        Some(l) if l == upstream => println!("Already up to date."),
        Some(l) if is_ancestor(&repo, &upstream, &l)? => println!("Already up to date (your branch is ahead)."),
        Some(l) if is_ancestor(&repo, &l, &upstream)? => {
            let s = worktree::status(&repo)?;
            if !s.clean() {
                return Err("your local changes would be overwritten; commit them first".into());
            }
            let n = worktree::checkout(&repo, Some(l), upstream)?;
            repo.update_head(&upstream)?;
            println!("Updating {}..{}\nFast-forward\n {} file{} changed", l.short(), upstream.short(), n, if n == 1 { "" } else { "s" });
        }
        Some(_) => return Err("your branch and the remote have diverged; merging is not supported yet (git reset --hard origin/BRANCH discards your commits)".into()),
        None => {
            worktree::checkout(&repo, None, upstream)?;
            repo.update_head(&upstream)?;
        }
    }
    Ok(())
}

fn push(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let force = args.iter().any(|a| *a == "-f" || *a == "--force");
    let rest: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with('-')).collect();
    let remote_name = rest.first().copied().unwrap_or("origin");
    let branch = match rest.get(1) {
        Some(b) => b.to_string(),
        None => current_branch(&repo).ok_or("not on a branch: git push REMOTE BRANCH")?,
    };
    let local = repo.read_ref(&format!("refs/heads/{}", branch)).ok_or_else(|| format!("no branch '{}'", branch))?;
    let url = remote_url(&repo, remote_name)?;
    let remote = Remote::new(&url)?;
    let adv = remote.discover("git-receive-pack")?;
    let refname = format!("refs/heads/{}", branch);
    let old = adv.get(&refname).unwrap_or(Id::ZERO);
    if old == local {
        println!("Everything up-to-date");
        return Ok(());
    }
    if !old.is_zero() {
        if !repo.has_object(&old) {
            return Err("the remote has commits you do not have; git pull first".into());
        }
        if !force && !is_ancestor(&repo, &old, &local)? {
            return Err("rejected (non-fast-forward): the remote has commits you do not have; git pull first, or push -f".into());
        }
    }
    let mut theirs = BTreeSet::new();
    if !old.is_zero() {
        let have: Vec<Id> = history(&repo, &[old], &BTreeSet::new(), usize::MAX)?.into_iter().map(|(id, _)| id).collect();
        objects_of(&repo, &have, &mut theirs)?;
    }
    let commits: Vec<Id> = history(&repo, &[local], &theirs, usize::MAX)?.into_iter().map(|(id, _)| id).collect();
    let mut ours = BTreeSet::new();
    objects_of(&repo, &commits, &mut ours)?;
    let mut objects = Vec::new();
    for id in ours.difference(&theirs) {
        let (kind, data) = repo.read_object(id).ok_or_else(|| format!("object {} is missing", id))?;
        objects.push((kind, data));
    }
    let pack = pack::build(&objects);
    println!("Pushing {} commit{} ({} objects, {} KiB) to {}", commits.len(), if commits.len() == 1 { "" } else { "s" }, objects.len(), pack.len() / 1024, remote.url);
    let messages = remote.push(&adv, &[(old, local, refname)], &pack)?;
    for m in messages.lines().filter(|l| !l.trim().is_empty()) {
        println!("remote: {}", m.trim());
    }
    repo.write_ref(&format!("refs/remotes/{}/{}", remote_name, branch), &local)?;
    println!("   {}..{}  {} -> {}", if old.is_zero() { "(new)".into() } else { old.short() }, local.short(), branch, branch);
    Ok(())
}

fn tag(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    match args {
        [] | ["-l"] => {
            for (name, _) in repo.refs("refs/tags/") {
                println!("{}", name.trim_start_matches("refs/tags/"));
            }
            Ok(())
        }
        ["-d", name] => repo.delete_ref(&format!("refs/tags/{}", name)),
        [name] | [name, _] => {
            let id = repo.rev_parse(args.get(1).copied().unwrap_or("HEAD"))?;
            repo.write_ref(&format!("refs/tags/{}", name), &id)
        }
        _ => Err("usage: git tag [-d] [NAME [REV]]".into()),
    }
}

fn remote_cmd(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    match args {
        [] | ["-v"] => {
            let text = fs::read_to_string(&repo.path("config")).unwrap_or_default();
            for l in text.lines() {
                if let Some(name) = l.trim().strip_prefix("[remote \"").and_then(|r| r.strip_suffix("\"]")) {
                    if args.is_empty() {
                        println!("{}", name);
                    } else {
                        let url = repo.config(&format!("remote.{}.url", name)).unwrap_or_default();
                        println!("{}\t{} (fetch)\n{}\t{} (push)", name, url, name, url);
                    }
                }
            }
            Ok(())
        }
        ["add", name, url] => {
            repo.set_config(&format!("remote.{}.url", name), url)?;
            repo.set_config(&format!("remote.{}.fetch", name), &format!("+refs/heads/*:refs/remotes/{}/*", name))
        }
        ["set-url", name, url] => repo.set_config(&format!("remote.{}.url", name), url),
        _ => Err("usage: git remote [-v] | remote add NAME URL | remote set-url NAME URL".into()),
    }
}

fn config(args: &[&str]) -> Result<()> {
    let global = args.contains(&"--global");
    let rest: Vec<&str> = args.iter().copied().filter(|a| *a != "--global").collect();
    let home = env::var("HOME").unwrap_or("/root");
    let gpath = fs::join(home, ".gitconfig");
    match rest[..] {
        [key] => {
            let v = if global { repo::config_get(&repo::global_config(), key) } else { Repo::find().ok().and_then(|r| r.config(key)).or_else(|| repo::config_get(&repo::global_config(), key)) };
            match v {
                Some(v) => {
                    println!("{}", v);
                    Ok(())
                }
                None => Err(String::new()),
            }
        }
        [key, value] => {
            if global {
                let text = fs::read_to_string(&gpath).unwrap_or_default();
                repo::write_atomic(&gpath, repo::config_set(&text, key, value).as_bytes())
            } else {
                Repo::find()?.set_config(key, value)
            }
        }
        _ => Err("usage: git config [--global] KEY [VALUE]".into()),
    }
}

fn rev_parse(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    match args {
        ["--show-toplevel"] => println!("{}", repo.work),
        ["--abbrev-ref", "HEAD"] => println!("{}", current_branch(&repo).unwrap_or_else(|| "HEAD".into())),
        ["--short", rev] => println!("{}", repo.rev_parse(rev)?.short()),
        [rev] => println!("{}", repo.rev_parse(rev)?),
        _ => return Err("usage: git rev-parse [--short] REV | --abbrev-ref HEAD | --show-toplevel".into()),
    }
    Ok(())
}

fn cat_file(args: &[&str]) -> Result<()> {
    let repo = Repo::find()?;
    let (flag, rev) = match args {
        [f, r] => (*f, *r),
        _ => return Err("usage: git cat-file -p|-t|-s REV".into()),
    };
    let id = repo.rev_parse(rev)?;
    let (kind, data) = repo.read_object(&id).ok_or("object not found")?;
    match flag {
        "-t" => println!("{}", kind.name()),
        "-s" => println!("{}", data.len()),
        "-p" if kind == Kind::Tree => {
            for e in huldra_git::object::parse_tree(&data).map_err(|e| e.0)? {
                let k = if e.is_tree() { "tree" } else if e.mode == huldra_git::object::MODE_GITLINK { "commit" } else { "blob" };
                println!("{:06o} {} {}\t{}", e.mode, k, e.id, e.name);
            }
        }
        "-p" => {
            let _ = huldra_user::io::write_all(1, &data);
        }
        _ => return Err(format!("unknown option {}", flag)),
    }
    Ok(())
}

fn usage() -> i32 {
    eprintln!("usage: git COMMAND [ARGS]\n");
    eprintln!("  init clone status add rm restore commit log diff show branch checkout");
    eprintln!("  switch reset fetch pull push tag remote config rev-parse cat-file\n");
    eprintln!("help git explains each command.");
    2
}

fn main() -> i32 {
    let argv = env::args();
    let Some(cmd) = argv.get(1) else { return usage() };
    let args: Vec<&str> = argv[2..].iter().map(|s| s.as_str()).collect();
    let r = match cmd.as_str() {
        "init" => init(&args),
        "clone" => clone(&args),
        "status" => status(&args),
        "add" => add(&args),
        "rm" => rm(&args),
        "restore" => restore(&args),
        "commit" => commit(&args),
        "log" => log(&args),
        "diff" => diff_cmd(&args),
        "show" => show(&args),
        "branch" => branch(&args),
        "checkout" => checkout(&args),
        "switch" => checkout(&args),
        "reset" => reset(&args),
        "fetch" => fetch(&args),
        "pull" => pull(&args),
        "push" => push(&args),
        "tag" => tag(&args),
        "remote" => remote_cmd(&args),
        "config" => config(&args),
        "rev-parse" => rev_parse(&args),
        "cat-file" => cat_file(&args),
        "--version" | "version" => {
            println!("git version 2.0 (huldra {})", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "help" | "--help" | "-h" => return usage(),
        other => Err(format!("'{}' is not a git command", other)),
    };
    match r {
        Ok(()) => 0,
        Err(e) if e.is_empty() => 1,
        Err(e) => {
            eprintln!("{}: {}", paint("31", "error"), e);
            1
        }
    }
}
