//! A repository on disk: the object database (loose objects and packs),
//! references, the config file and the index.

use huldra_git::index::{self, Entry};
use huldra_git::object::{self, Commit, Id, Kind, TreeEntry};
use huldra_git::pack;
use huldra_user::abi::fs::{O_CREAT, O_TRUNC, O_WRONLY};
use huldra_user::{format, fs, String, ToString, Vec};

pub type Result<T> = core::result::Result<T, String>;

pub fn ctx<T, E: core::fmt::Display>(r: core::result::Result<T, E>, what: &str) -> Result<T> {
    r.map_err(|e| format!("{}: {}", what, e))
}

/// Writes through a temporary file and a rename.
pub fn write_atomic(path: &str, data: &[u8]) -> Result<()> {
    let tmp = format!("{}.lock", path);
    let f = ctx(fs::File::open_with(&tmp, O_WRONLY | O_CREAT | O_TRUNC, 0o644), &tmp)?;
    ctx(f.write_all(data), &tmp)?;
    drop(f);
    ctx(fs::rename(&tmp, path), path)
}

struct Pack {
    data: Vec<u8>,
    index: pack::Index,
}

pub struct Repo {
    pub git: String,
    pub work: String,
    packs: core::cell::RefCell<Option<Vec<Pack>>>,
}

impl Repo {
    /// The repository containing the current directory.
    pub fn find() -> Result<Repo> {
        let mut dir = fs::current_dir().map_err(|e| format!("cwd: {}", e))?;
        loop {
            let git = fs::join(&dir, ".git");
            if fs::metadata(&git).is_ok_and(|s| fs::is_dir(&s)) {
                return Ok(Repo::open(&dir));
            }
            match dir.rfind('/') {
                Some(0) if dir.len() > 1 => dir = "/".into(),
                Some(i) if i > 0 => dir.truncate(i),
                _ => return Err("not a git repository (or any parent up to /)".into()),
            }
        }
    }

    pub fn open(work: &str) -> Repo {
        Repo { git: fs::join(work, ".git"), work: work.to_string(), packs: core::cell::RefCell::new(None) }
    }

    /// Creates an empty repository in `work`.
    pub fn init(work: &str, branch: &str) -> Result<Repo> {
        let r = Repo::open(work);
        for d in ["objects/pack", "objects/info", "refs/heads", "refs/tags", "info"] {
            ctx(fs::create_dir_all(&r.path(d)), d)?;
        }
        if !fs::exists(&r.path("HEAD")) {
            ctx(fs::write(&r.path("HEAD"), format!("ref: refs/heads/{}\n", branch).as_bytes()), "HEAD")?;
        }
        if !fs::exists(&r.path("config")) {
            ctx(fs::write(&r.path("config"), b"[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n"), "config")?;
        }
        let _ = fs::write(&r.path("description"), b"Unnamed repository; edit this file 'description' to name the repository.\n");
        Ok(r)
    }

    pub fn path(&self, rel: &str) -> String {
        fs::join(&self.git, rel)
    }

    pub fn work_path(&self, rel: &str) -> String {
        fs::join(&self.work, rel)
    }

    // ------------------------------------------------------------ objects

    fn load_packs(&self) {
        let mut slot = self.packs.borrow_mut();
        if slot.is_some() {
            return;
        }
        let dir = self.path("objects/pack");
        let mut packs = Vec::new();
        for e in fs::read_dir(&dir).unwrap_or_default() {
            if let Some(base) = e.name.strip_suffix(".idx") {
                let idx = fs::read(&fs::join(&dir, &e.name)).ok().and_then(|d| pack::Index::parse(d).ok());
                let data = fs::read(&fs::join(&dir, &format!("{}.pack", base))).ok();
                if let (Some(index), Some(data)) = (idx, data) {
                    packs.push(Pack { data, index });
                }
            }
        }
        *slot = Some(packs);
    }

    /// Forgets loaded packs (after fetching a new one).
    pub fn reload_packs(&self) {
        *self.packs.borrow_mut() = None;
    }

    fn loose_path(&self, id: &Id) -> String {
        let h = id.hex();
        self.path(&format!("objects/{}/{}", &h[..2], &h[2..]))
    }

    pub fn read_object(&self, id: &Id) -> Option<(Kind, Vec<u8>)> {
        if let Ok(data) = fs::read(&self.loose_path(id)) {
            return object::decode_loose(&data).ok();
        }
        self.load_packs();
        let packs = self.packs.borrow();
        for p in packs.as_ref().unwrap() {
            if let Some(off) = p.index.find(id) {
                return pack::read_at(&p.data, off, &|base| self.read_object(base)).ok();
            }
        }
        None
    }

    pub fn has_object(&self, id: &Id) -> bool {
        if fs::exists(&self.loose_path(id)) {
            return true;
        }
        self.load_packs();
        self.packs.borrow().as_ref().unwrap().iter().any(|p| p.index.find(id).is_some())
    }

    pub fn object(&self, id: &Id, want: Kind) -> Result<Vec<u8>> {
        match self.read_object(id) {
            Some((k, data)) if k == want => Ok(data),
            Some((k, _)) => Err(format!("{} is a {}, not a {}", id.short(), k.name(), want.name())),
            None => Err(format!("object {} is missing", id)),
        }
    }

    pub fn write_object(&self, kind: Kind, data: &[u8]) -> Result<Id> {
        let id = object::hash(kind, data);
        if self.has_object(&id) {
            return Ok(id);
        }
        let path = self.loose_path(&id);
        let dir = &path[..path.rfind('/').unwrap()];
        ctx(fs::create_dir_all(dir), dir)?;
        write_atomic(&path, &object::encode_loose(kind, data))?;
        Ok(id)
    }

    pub fn commit(&self, id: &Id) -> Result<Commit> {
        Commit::parse(&self.object(id, Kind::Commit)?).map_err(|e| e.0)
    }

    pub fn tree(&self, id: &Id) -> Result<Vec<TreeEntry>> {
        object::parse_tree(&self.object(id, Kind::Tree)?).map_err(|e| e.0)
    }

    /// Every file of a tree, recursively: (path, mode, id).
    pub fn flatten_tree(&self, id: &Id, prefix: &str, out: &mut Vec<(String, u32, Id)>) -> Result<()> {
        for e in self.tree(id)? {
            let path = if prefix.is_empty() { e.name.clone() } else { format!("{}/{}", prefix, e.name) };
            if e.is_tree() {
                self.flatten_tree(&e.id, &path, out)?;
            } else {
                out.push((path, e.mode, e.id));
            }
        }
        Ok(())
    }

    /// Stores a received pack and its index.
    pub fn store_pack(&self, data: &[u8], objects: &[pack::Object]) -> Result<()> {
        let sum = &data[data.len() - 20..];
        let name = format!("pack-{}", huldra_crypto::hex(sum));
        let dir = self.path("objects/pack");
        ctx(fs::create_dir_all(&dir), &dir)?;
        write_atomic(&fs::join(&dir, &format!("{}.pack", name)), data)?;
        write_atomic(&fs::join(&dir, &format!("{}.idx", name)), &pack::write_index(objects, sum))?;
        self.reload_packs();
        Ok(())
    }

    // ------------------------------------------------------------- refs

    fn packed_refs(&self) -> Vec<(String, Id)> {
        fs::read_to_string(&self.path("packed-refs"))
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with('^'))
            .filter_map(|l| {
                let (id, name) = l.split_once(' ')?;
                Some((name.to_string(), Id::from_hex(id)?))
            })
            .collect()
    }

    /// The id a reference points to, following symbolic references.
    pub fn read_ref(&self, name: &str) -> Option<Id> {
        let mut name = name.to_string();
        for _ in 0..5 {
            match fs::read_to_string(&self.path(&name)) {
                Ok(text) => {
                    let t = text.trim();
                    match t.strip_prefix("ref: ") {
                        Some(target) => name = target.to_string(),
                        None => return Id::from_hex(t),
                    }
                }
                Err(_) => return self.packed_refs().into_iter().find(|(n, _)| *n == name).map(|(_, id)| id),
            }
        }
        None
    }

    /// The branch HEAD points to (`refs/heads/main`), if it is symbolic.
    pub fn head_ref(&self) -> Option<String> {
        let t = fs::read_to_string(&self.path("HEAD")).ok()?;
        t.trim().strip_prefix("ref: ").map(String::from)
    }

    pub fn head(&self) -> Option<Id> {
        self.read_ref("HEAD")
    }

    pub fn write_ref(&self, name: &str, id: &Id) -> Result<()> {
        let path = self.path(name);
        let dir = &path[..path.rfind('/').unwrap()];
        ctx(fs::create_dir_all(dir), dir)?;
        write_atomic(&path, format!("{}\n", id).as_bytes())
    }

    pub fn delete_ref(&self, name: &str) -> Result<()> {
        let _ = fs::remove_file(&self.path(name));
        let packed = self.packed_refs();
        if packed.iter().any(|(n, _)| n == name) {
            let text: String = packed.iter().filter(|(n, _)| n != name).map(|(n, id)| format!("{} {}\n", id, n)).collect();
            write_atomic(&self.path("packed-refs"), text.as_bytes())?;
        }
        Ok(())
    }

    pub fn set_head_branch(&self, branch: &str) -> Result<()> {
        write_atomic(&self.path("HEAD"), format!("ref: refs/heads/{}\n", branch).as_bytes())
    }

    /// Detached HEAD at `id`.
    pub fn set_head_detached(&self, id: &Id) -> Result<()> {
        write_atomic(&self.path("HEAD"), format!("{}\n", id).as_bytes())
    }

    /// Updates what HEAD points to (the branch, or HEAD itself if detached).
    pub fn update_head(&self, id: &Id) -> Result<()> {
        match self.head_ref() {
            Some(r) => self.write_ref(&r, id),
            None => self.set_head_detached(id),
        }
    }

    /// All references under `prefix` (loose and packed), sorted.
    pub fn refs(&self, prefix: &str) -> Vec<(String, Id)> {
        fn walk(repo: &Repo, rel: &str, out: &mut Vec<(String, Id)>) {
            for e in fs::read_dir(&repo.path(rel)).unwrap_or_default() {
                let name = format!("{}/{}", rel, e.name);
                if e.is_dir() {
                    walk(repo, &name, out);
                } else if let Some(id) = fs::read_to_string(&repo.path(&name)).ok().and_then(|t| Id::from_hex(t.trim())) {
                    out.push((name, id));
                }
            }
        }
        let mut out = Vec::new();
        walk(self, prefix.trim_end_matches('/'), &mut out);
        for (n, id) in self.packed_refs() {
            if n.starts_with(prefix) && !out.iter().any(|(m, _)| *m == n) {
                out.push((n, id));
            }
        }
        out.sort();
        out
    }

    /// Resolves a revision: HEAD, a branch, a tag, a remote branch, a full
    /// or abbreviated id, with `~N` and `^` suffixes.
    pub fn rev_parse(&self, rev: &str) -> Result<Id> {
        if let Some(i) = rev.rfind(['~', '^']) {
            let (base, op) = rev.split_at(i);
            let n: usize = if op.len() > 1 { op[1..].parse().map_err(|_| format!("bad revision '{}'", rev))? } else { 1 };
            let mut id = self.rev_parse(if base.is_empty() { "HEAD" } else { base })?;
            if op.starts_with('^') {
                let c = self.commit(&id)?;
                return c.parents.get(n.saturating_sub(1)).copied().ok_or_else(|| format!("{} has no parent", rev));
            }
            for _ in 0..n {
                id = *self.commit(&id)?.parents.first().ok_or_else(|| format!("{}: not that many ancestors", rev))?;
            }
            return Ok(id);
        }
        for candidate in [rev.to_string(), format!("refs/{}", rev), format!("refs/heads/{}", rev), format!("refs/tags/{}", rev), format!("refs/remotes/{}", rev)] {
            if let Some(id) = self.read_ref(&candidate) {
                return Ok(self.peel(id));
            }
        }
        if rev.len() == 40 {
            if let Some(id) = Id::from_hex(rev) {
                return Ok(id);
            }
        }
        if rev.len() >= 4 && rev.chars().all(|c| c.is_ascii_hexdigit()) {
            let rev = rev.to_ascii_lowercase();
            let mut found: Vec<Id> = Vec::new();
            for e in fs::read_dir(&self.path(&format!("objects/{}", &rev[..2]))).unwrap_or_default() {
                let full = format!("{}{}", &rev[..2], e.name);
                if full.starts_with(&rev) {
                    found.extend(Id::from_hex(&full));
                }
            }
            self.load_packs();
            for p in self.packs.borrow().as_ref().unwrap() {
                found.extend(p.index.ids().filter(|id| id.hex().starts_with(&rev)));
            }
            found.sort();
            found.dedup();
            return match found.len() {
                1 => Ok(found[0]),
                0 => Err(format!("unknown revision '{}'", rev)),
                _ => Err(format!("ambiguous revision '{}'", rev)),
            };
        }
        Err(format!("unknown revision '{}'", rev))
    }

    /// An annotated tag's target commit.
    pub fn peel(&self, id: Id) -> Id {
        match self.read_object(&id) {
            Some((Kind::Tag, data)) => object::tag_target(&data).map(|t| self.peel(t)).unwrap_or(id),
            _ => id,
        }
    }

    pub fn shallow(&self) -> Vec<Id> {
        fs::read_to_string(&self.path("shallow")).unwrap_or_default().lines().filter_map(Id::from_hex).collect()
    }

    // ------------------------------------------------------------ config

    /// `section.key` or `section.sub.key` from .git/config, then ~/.gitconfig.
    pub fn config(&self, key: &str) -> Option<String> {
        let local = fs::read_to_string(&self.path("config")).unwrap_or_default();
        config_get(&local, key).or_else(|| config_get(&global_config(), key))
    }

    pub fn set_config(&self, key: &str, value: &str) -> Result<()> {
        let text = fs::read_to_string(&self.path("config")).unwrap_or_default();
        write_atomic(&self.path("config"), config_set(&text, key, value).as_bytes())
    }

    // ------------------------------------------------------------- index

    pub fn read_index(&self) -> Result<Vec<Entry>> {
        match fs::read(&self.path("index")) {
            Ok(data) => index::parse(&data).map_err(|e| e.0),
            Err(_) => Ok(Vec::new()),
        }
    }

    pub fn write_index(&self, entries: &[Entry]) -> Result<()> {
        write_atomic(&self.path("index"), &index::write(entries))
    }
}

pub fn global_config() -> String {
    let home = huldra_user::env::var("HOME").unwrap_or("/root");
    fs::read_to_string(&fs::join(home, ".gitconfig")).unwrap_or_default()
}

/// Splits `a.b.c` into (section header match, key).
fn split_key(key: &str) -> (String, String, String) {
    let (section, rest) = key.split_once('.').unwrap_or((key, ""));
    match rest.rsplit_once('.') {
        Some((sub, k)) => (section.to_ascii_lowercase(), sub.to_string(), k.to_ascii_lowercase()),
        None => (section.to_ascii_lowercase(), String::new(), rest.to_ascii_lowercase()),
    }
}

fn header_of(line: &str) -> Option<(String, String)> {
    let inner = line.trim().strip_prefix('[')?.strip_suffix(']')?;
    match inner.split_once(' ') {
        Some((s, sub)) => Some((s.to_ascii_lowercase(), sub.trim().trim_matches('"').to_string())),
        None => Some((inner.to_ascii_lowercase(), String::new())),
    }
}

pub fn config_get(text: &str, key: &str) -> Option<String> {
    let (section, sub, k) = split_key(key);
    let mut current = (String::new(), String::new());
    let mut found = None;
    for line in text.lines() {
        if let Some(h) = header_of(line) {
            current = h;
            continue;
        }
        if current.0 == section && current.1 == sub {
            if let Some((name, value)) = line.split_once('=') {
                if name.trim().eq_ignore_ascii_case(&k) {
                    found = Some(value.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    found
}

pub fn config_set(text: &str, key: &str, value: &str) -> String {
    let (section, sub, k) = split_key(key);
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let mut current = (String::new(), String::new());
    let mut section_end = None;
    for i in 0..lines.len() {
        if let Some(h) = header_of(&lines[i]) {
            current = h;
            continue;
        }
        if current.0 == section && current.1 == sub {
            section_end = Some(i + 1);
            if lines[i].split_once('=').is_some_and(|(n, _)| n.trim().eq_ignore_ascii_case(&k)) {
                lines[i] = format!("\t{} = {}", k, value);
                return lines.join("\n") + "\n";
            }
        }
    }
    let entry = format!("\t{} = {}", k, value);
    match section_end.or_else(|| {
        let idx = lines.iter().position(|l| header_of(l).is_some_and(|h| h.0 == section && h.1 == sub))?;
        Some(idx + 1)
    }) {
        Some(at) => lines.insert(at, entry),
        None => {
            lines.push(if sub.is_empty() { format!("[{}]", section) } else { format!("[{} \"{}\"]", section, sub) });
            lines.push(entry);
        }
    }
    lines.join("\n") + "\n"
}

/// Simple .gitignore matching: `name`, `*.ext`, `dir/`, `/anchored`, `?`.
pub struct Ignore {
    patterns: Vec<(String, bool, bool)>, // (glob, anchored, dir only)
}

impl Ignore {
    pub fn load(repo: &Repo) -> Ignore {
        let mut patterns = Vec::new();
        let text = fs::read_to_string(&repo.work_path(".gitignore")).unwrap_or_default() + "\n" + &fs::read_to_string(&repo.path("info/exclude")).unwrap_or_default();
        for l in text.lines().map(str::trim) {
            if l.is_empty() || l.starts_with('#') || l.starts_with('!') {
                continue;
            }
            let dir = l.ends_with('/');
            let p = l.trim_end_matches('/');
            let anchored = p.starts_with('/') || p.contains('/');
            patterns.push((p.trim_start_matches('/').to_string(), anchored, dir));
        }
        Ignore { patterns }
    }

    pub fn ignored(&self, path: &str, is_dir: bool) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        self.patterns.iter().any(|(p, anchored, dir_only)| (!dir_only || is_dir) && if *anchored { glob(p, path) } else { glob(p, name) })
    }
}

pub fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    fn m(p: &[char], t: &[char]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some('*') => (0..=t.len()).any(|i| !t[..i].contains(&'/') && m(&p[1..], &t[i..])),
            Some('?') => !t.is_empty() && t[0] != '/' && m(&p[1..], &t[1..]),
            Some(c) => t.first() == Some(c) && m(&p[1..], &t[1..]),
        }
    }
    m(&p, &t)
}
