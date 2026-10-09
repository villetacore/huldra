//! The working tree: hashing files, building trees from the index,
//! checking trees out, and comparing HEAD, the index and the files.

use crate::repo::{ctx, Ignore, Repo, Result};
use huldra_git::index::Entry;
use huldra_git::object::{self, Id, Kind, TreeEntry, MODE_EXEC, MODE_FILE, MODE_LINK, MODE_TREE};
use huldra_user::abi::fs::{Stat, O_CREAT, O_TRUNC, O_WRONLY};
use huldra_user::{format, fs, String, ToString, Vec};
use alloc::collections::BTreeMap;

pub fn mode_of(st: &Stat) -> u32 {
    if fs::is_symlink(st) {
        MODE_LINK
    } else if st.st_mode & 0o111 != 0 {
        MODE_EXEC
    } else {
        MODE_FILE
    }
}

/// The content git stores for a path: file data, or a link's target.
pub fn read_content(path: &str, st: &Stat) -> Result<Vec<u8>> {
    if fs::is_symlink(st) {
        Ok(ctx(fs::read_link(path), path)?.into_bytes())
    } else {
        ctx(fs::read(path), path)
    }
}

pub fn entry_for(path: &str, st: &Stat, id: Id) -> Entry {
    Entry {
        ctime: (st.st_ctime as u32, st.st_ctime_nsec as u32),
        mtime: (st.st_mtime as u32, st.st_mtime_nsec as u32),
        dev: st.st_dev as u32,
        ino: st.st_ino as u32,
        mode: mode_of(st),
        uid: st.st_uid,
        gid: st.st_gid,
        size: st.st_size as u32,
        id,
        flags: 0,
        path: path.to_string(),
    }
}

/// Did the file change since the index entry was made (by stat data,
/// then by content when the stat data differs)?
pub fn changed(repo: &Repo, e: &Entry) -> Result<Option<Id>> {
    let path = repo.work_path(&e.path);
    let Ok(st) = fs::symlink_metadata(&path) else { return Ok(None) };
    if st.st_mtime as u32 == e.mtime.0 && st.st_size as u32 == e.size && st.st_ino as u32 == e.ino && mode_of(&st) == e.mode {
        return Ok(Some(e.id));
    }
    let id = object::hash(Kind::Blob, &read_content(&path, &st)?);
    Ok(Some(id))
}

/// Every file of the working tree (relative paths), skipping .git and
/// ignored files.
pub fn walk(repo: &Repo, ignore: &Ignore) -> Vec<String> {
    fn rec(repo: &Repo, ignore: &Ignore, rel: &str, out: &mut Vec<String>) {
        let dir = if rel.is_empty() { repo.work.clone() } else { repo.work_path(rel) };
        for e in fs::read_dir(&dir).unwrap_or_default() {
            if rel.is_empty() && e.name == ".git" {
                continue;
            }
            let path = if rel.is_empty() { e.name.clone() } else { format!("{}/{}", rel, e.name) };
            if ignore.ignored(&path, e.is_dir()) {
                continue;
            }
            if e.is_dir() {
                rec(repo, ignore, &path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    rec(repo, ignore, "", &mut out);
    out.sort();
    out
}

/// Writes trees for the index; returns the root tree id.
pub fn write_tree(repo: &Repo, entries: &[Entry]) -> Result<Id> {
    fn build(repo: &Repo, items: &[(Vec<&str>, u32, Id)]) -> Result<Id> {
        let mut tree: Vec<TreeEntry> = Vec::new();
        let mut subdirs: BTreeMap<&str, Vec<(Vec<&str>, u32, Id)>> = BTreeMap::new();
        for (parts, mode, id) in items {
            if parts.len() == 1 {
                tree.push(TreeEntry { mode: *mode, name: parts[0].to_string(), id: *id });
            } else {
                subdirs.entry(parts[0]).or_default().push((parts[1..].to_vec(), *mode, *id));
            }
        }
        for (name, sub) in subdirs {
            let id = build(repo, &sub)?;
            tree.push(TreeEntry { mode: MODE_TREE, name: name.to_string(), id });
        }
        repo.write_object(Kind::Tree, &object::write_tree(&tree))
    }
    let items: Vec<(Vec<&str>, u32, Id)> = entries.iter().map(|e| (e.path.split('/').collect(), e.mode, e.id)).collect();
    build(repo, &items)
}

/// path -> (mode, id) for a commit's tree (empty for none).
pub fn tree_files(repo: &Repo, commit: Option<Id>) -> Result<BTreeMap<String, (u32, Id)>> {
    let mut out = BTreeMap::new();
    if let Some(c) = commit {
        let tree = repo.commit(&c)?.tree;
        let mut files = Vec::new();
        repo.flatten_tree(&tree, "", &mut files)?;
        for (p, m, id) in files {
            out.insert(p, (m, id));
        }
    }
    Ok(out)
}

fn write_file(repo: &Repo, rel: &str, mode: u32, data: &[u8]) -> Result<()> {
    let path = repo.work_path(rel);
    if let Some(i) = path.rfind('/') {
        ctx(fs::create_dir_all(&path[..i]), &path[..i])?;
    }
    let _ = fs::remove_file(&path);
    if mode == MODE_LINK {
        return ctx(fs::symlink(&String::from_utf8_lossy(data), &path), &path);
    }
    let f = ctx(fs::File::open_with(&path, O_WRONLY | O_CREAT | O_TRUNC, if mode == MODE_EXEC { 0o755 } else { 0o644 }), &path)?;
    ctx(f.write_all(data), &path)
}

/// Removes now-empty parent directories of `rel`.
fn prune_dirs(repo: &Repo, rel: &str) {
    let mut p = rel.to_string();
    while let Some(i) = p.rfind('/') {
        p.truncate(i);
        if fs::remove_dir(&repo.work_path(&p)).is_err() {
            break;
        }
    }
}

/// Makes the working tree and the index match `target`'s tree, starting
/// from `current` (files of the old commit that are not in the new one
/// are removed). Returns how many files changed.
pub fn checkout(repo: &Repo, current: Option<Id>, target: Id) -> Result<usize> {
    let old = tree_files(repo, current)?;
    let new = tree_files(repo, Some(target))?;
    let mut changed = 0;
    for path in old.keys() {
        if !new.contains_key(path) {
            let _ = fs::remove_file(&repo.work_path(path));
            prune_dirs(repo, path);
            changed += 1;
        }
    }
    let mut entries = Vec::new();
    for (path, (mode, id)) in &new {
        let full = repo.work_path(path);
        let same = old.get(path) == Some(&(*mode, *id)) && fs::symlink_metadata(&full).is_ok();
        if !same {
            if *mode == object::MODE_GITLINK {
                ctx(fs::create_dir_all(&full), &full)?;
                continue;
            }
            let data = repo.object(id, Kind::Blob)?;
            write_file(repo, path, *mode, &data)?;
            changed += 1;
        }
        let st = ctx(fs::symlink_metadata(&full), &full)?;
        let mut e = entry_for(path, &st, *id);
        e.mode = *mode;
        entries.push(e);
    }
    repo.write_index(&entries)?;
    Ok(changed)
}

/// Restores one path from the index.
pub fn restore(repo: &Repo, e: &Entry) -> Result<()> {
    let data = repo.object(&e.id, Kind::Blob)?;
    write_file(repo, &e.path, e.mode, &data)
}

#[derive(Default)]
pub struct Status {
    /// HEAD vs index: (path, "new file" | "modified" | "deleted")
    pub staged: Vec<(String, &'static str)>,
    /// index vs working tree
    pub unstaged: Vec<(String, &'static str)>,
    pub untracked: Vec<String>,
}

impl Status {
    pub fn clean(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty()
    }
}

pub fn status(repo: &Repo) -> Result<Status> {
    let head = tree_files(repo, repo.head())?;
    let index = repo.read_index()?;
    let mut s = Status::default();
    for e in &index {
        match head.get(&e.path) {
            None => s.staged.push((e.path.clone(), "new file")),
            Some((m, id)) if *id != e.id || *m != e.mode => s.staged.push((e.path.clone(), "modified")),
            _ => {}
        }
    }
    for path in head.keys() {
        if !index.iter().any(|e| e.path == *path) {
            s.staged.push((path.clone(), "deleted"));
        }
    }
    s.staged.sort();
    for e in &index {
        match changed(repo, e)? {
            None => s.unstaged.push((e.path.clone(), "deleted")),
            Some(id) if id != e.id => s.unstaged.push((e.path.clone(), "modified")),
            _ => {}
        }
    }
    let ignore = Ignore::load(repo);
    let tracked: alloc::collections::BTreeSet<&str> = index.iter().map(|e| e.path.as_str()).collect();
    s.untracked = walk(repo, &ignore).into_iter().filter(|p| !tracked.contains(p.as_str())).collect();
    Ok(s)
}

/// Text of a blob or file for diffs ("Binary" if it is not text).
pub fn text_of(data: &[u8]) -> Option<String> {
    if data.iter().take(8000).any(|&b| b == 0) {
        return None;
    }
    Some(String::from_utf8_lossy(data).into_owned())
}
