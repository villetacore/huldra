//! Git objects: ids, the loose object format, trees, commits and tags.

use crate::{Error, Result};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use huldra_crypto::sha::{Hash, Sha1};

/// A SHA-1 object name.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Id(pub [u8; 20]);

impl Id {
    pub const ZERO: Id = Id([0; 20]);

    pub fn from_hex(s: &str) -> Option<Id> {
        let b = huldra_crypto::from_hex(s.get(..40)?)?;
        Some(Id(b.try_into().ok()?))
    }

    pub fn hex(&self) -> String {
        huldra_crypto::hex(&self.0)
    }

    pub fn short(&self) -> String {
        self.hex()[..7].to_string()
    }

    pub fn is_zero(&self) -> bool {
        self.0 == [0; 20]
    }
}

impl core::fmt::Debug for Id {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.write_str(&self.hex())
    }
}

impl core::fmt::Display for Id {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.write_str(&self.hex())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Commit,
    Tree,
    Blob,
    Tag,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Commit => "commit",
            Kind::Tree => "tree",
            Kind::Blob => "blob",
            Kind::Tag => "tag",
        }
    }

    pub fn from_name(s: &str) -> Option<Kind> {
        Some(match s {
            "commit" => Kind::Commit,
            "tree" => Kind::Tree,
            "blob" => Kind::Blob,
            "tag" => Kind::Tag,
            _ => return None,
        })
    }

    /// Object type in a pack file.
    pub fn pack_type(self) -> u8 {
        match self {
            Kind::Commit => 1,
            Kind::Tree => 2,
            Kind::Blob => 3,
            Kind::Tag => 4,
        }
    }

    pub fn from_pack_type(t: u8) -> Option<Kind> {
        Some(match t {
            1 => Kind::Commit,
            2 => Kind::Tree,
            3 => Kind::Blob,
            4 => Kind::Tag,
            _ => return None,
        })
    }
}

/// The id of an object: SHA-1 of "kind size\0" and the content.
pub fn hash(kind: Kind, data: &[u8]) -> Id {
    let mut h = Sha1::new();
    h.update(format!("{} {}\0", kind.name(), data.len()).as_bytes());
    h.update(data);
    Id(h.finish())
}

/// A loose object file: zlib of the header and content.
pub fn encode_loose(kind: Kind, data: &[u8]) -> Vec<u8> {
    let mut raw = format!("{} {}\0", kind.name(), data.len()).into_bytes();
    raw.extend_from_slice(data);
    huldra_flate::zlib_compress(&raw)
}

pub fn decode_loose(file: &[u8]) -> Result<(Kind, Vec<u8>)> {
    let (raw, _) = huldra_flate::zlib_decompress(file).map_err(|e| Error(format!("loose object: {:?}", e)))?;
    let nul = raw.iter().position(|&b| b == 0).ok_or_else(|| Error("loose object: no header".into()))?;
    let header = core::str::from_utf8(&raw[..nul]).map_err(|_| Error("loose object: bad header".into()))?;
    let (kind, size) = header.split_once(' ').ok_or_else(|| Error("loose object: bad header".into()))?;
    let kind = Kind::from_name(kind).ok_or_else(|| Error(format!("unknown object type '{}'", kind)))?;
    let data = raw[nul + 1..].to_vec();
    if size.parse::<usize>().ok() != Some(data.len()) {
        return Err(Error("loose object: size mismatch".into()));
    }
    Ok((kind, data))
}

// ---------------------------------------------------------------- trees

pub const MODE_FILE: u32 = 0o100644;
pub const MODE_EXEC: u32 = 0o100755;
pub const MODE_LINK: u32 = 0o120000;
pub const MODE_TREE: u32 = 0o040000;
pub const MODE_GITLINK: u32 = 0o160000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEntry {
    pub mode: u32,
    pub name: String,
    pub id: Id,
}

impl TreeEntry {
    pub fn is_tree(&self) -> bool {
        self.mode == MODE_TREE
    }
}

pub fn parse_tree(data: &[u8]) -> Result<Vec<TreeEntry>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < data.len() {
        let sp = data[p..].iter().position(|&b| b == b' ').ok_or_else(|| Error("tree: no mode".into()))? + p;
        let mode = u32::from_str_radix(core::str::from_utf8(&data[p..sp]).unwrap_or("x"), 8).map_err(|_| Error("tree: bad mode".into()))?;
        let nul = data[sp..].iter().position(|&b| b == 0).ok_or_else(|| Error("tree: no name".into()))? + sp;
        let name = String::from_utf8_lossy(&data[sp + 1..nul]).into_owned();
        let id = data.get(nul + 1..nul + 21).ok_or_else(|| Error("tree: truncated".into()))?;
        out.push(TreeEntry { mode, name, id: Id(id.try_into().unwrap()) });
        p = nul + 21;
    }
    Ok(out)
}

/// Git's tree order: names compare as if directories ended with '/'.
fn tree_key(e: &TreeEntry) -> Vec<u8> {
    let mut k = e.name.as_bytes().to_vec();
    if e.is_tree() {
        k.push(b'/');
    }
    k
}

pub fn write_tree(entries: &[TreeEntry]) -> Vec<u8> {
    let mut sorted: Vec<&TreeEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| tree_key(e));
    let mut out = Vec::new();
    for e in sorted {
        out.extend_from_slice(format!("{:o} {}\0", e.mode, e.name).as_bytes());
        out.extend_from_slice(&e.id.0);
    }
    out
}

// -------------------------------------------------------------- commits

/// "Name <email> 1700000000 +0100"
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub name: String,
    pub email: String,
    pub time: i64,
    /// Offset from UTC in minutes.
    pub tz: i32,
}

impl Signature {
    pub fn parse(s: &str) -> Option<Signature> {
        let lt = s.find('<')?;
        let gt = s[lt..].find('>')? + lt;
        let mut rest = s[gt + 1..].split_whitespace();
        let time = rest.next()?.parse().ok()?;
        let tz = rest.next().unwrap_or("+0000");
        let sign = if tz.starts_with('-') { -1 } else { 1 };
        let digits: i32 = tz.trim_start_matches(['+', '-']).parse().unwrap_or(0);
        Some(Signature {
            name: s[..lt].trim().to_string(),
            email: s[lt + 1..gt].to_string(),
            time,
            tz: sign * (digits / 100 * 60 + digits % 100),
        })
    }

    pub fn to_line(&self) -> String {
        let a = self.tz.abs();
        format!("{} <{}> {} {}{:02}{:02}", self.name, self.email, self.time, if self.tz < 0 { '-' } else { '+' }, a / 60, a % 60)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub tree: Id,
    pub parents: Vec<Id>,
    pub author: Signature,
    pub committer: Signature,
    /// Other header lines (gpgsig, encoding, ...), kept as they were.
    pub extra: Vec<(String, String)>,
    pub message: String,
}

impl Commit {
    pub fn parse(data: &[u8]) -> Result<Commit> {
        let text = String::from_utf8_lossy(data);
        let (head, message) = text.split_once("\n\n").unwrap_or((&text, ""));
        let mut tree = None;
        let mut parents = Vec::new();
        let (mut author, mut committer) = (None, None);
        let mut extra: Vec<(String, String)> = Vec::new();
        for line in head.lines() {
            if let Some(cont) = line.strip_prefix(' ') {
                if let Some(last) = extra.last_mut() {
                    last.1.push('\n');
                    last.1.push_str(cont);
                }
                continue;
            }
            let (k, v) = line.split_once(' ').unwrap_or((line, ""));
            match k {
                "tree" => tree = Id::from_hex(v),
                "parent" => parents.push(Id::from_hex(v).ok_or_else(|| Error("commit: bad parent".into()))?),
                "author" => author = Signature::parse(v),
                "committer" => committer = Signature::parse(v),
                _ => extra.push((k.to_string(), v.to_string())),
            }
        }
        let author = author.ok_or_else(|| Error("commit: no author".into()))?;
        Ok(Commit {
            tree: tree.ok_or_else(|| Error("commit: no tree".into()))?,
            parents,
            committer: committer.unwrap_or_else(|| author.clone()),
            author,
            extra,
            message: message.to_string(),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut s = format!("tree {}\n", self.tree);
        for p in &self.parents {
            s.push_str(&format!("parent {}\n", p));
        }
        s.push_str(&format!("author {}\ncommitter {}\n", self.author.to_line(), self.committer.to_line()));
        for (k, v) in &self.extra {
            s.push_str(&format!("{} {}\n", k, v.replace('\n', "\n ")));
        }
        s.push('\n');
        s.push_str(&self.message);
        s.into_bytes()
    }

    /// The first line of the message.
    pub fn summary(&self) -> &str {
        self.message.lines().next().unwrap_or("")
    }
}

/// The object an annotated tag points to.
pub fn tag_target(data: &[u8]) -> Option<Id> {
    let text = core::str::from_utf8(data).ok()?;
    Id::from_hex(text.lines().find_map(|l| l.strip_prefix("object "))?)
}
