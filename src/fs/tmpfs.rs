//! In-memory file system.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    NotADirectory,
    IsADirectory,
    AlreadyExists,
    NotEmpty,
    InvalidPath,
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            FsError::NotFound => "No such file or directory",
            FsError::NotADirectory => "Not a directory",
            FsError::IsADirectory => "Is a directory",
            FsError::AlreadyExists => "File exists",
            FsError::NotEmpty => "Directory not empty",
            FsError::InvalidPath => "Invalid argument",
        })
    }
}

pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: usize,
}

enum Node {
    File(Vec<u8>),
    Dir(BTreeMap<String, Node>),
}

impl Node {
    fn size(&self) -> usize {
        match self {
            Node::File(data) => data.len(),
            Node::Dir(children) => children.len(),
        }
    }
}

pub struct Tmpfs {
    root: Node,
    cwd: Vec<String>,
}

impl Tmpfs {
    pub fn new() -> Self {
        Tmpfs { root: Node::Dir(BTreeMap::new()), cwd: Vec::new() }
    }

    /// Resolves `path` (absolute or relative to cwd) into components,
    /// normalizing `.` and `..`.
    fn components(&self, path: &str) -> Vec<String> {
        let mut parts = if path.starts_with('/') { Vec::new() } else { self.cwd.clone() };
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                name => parts.push(name.to_string()),
            }
        }
        parts
    }

    fn lookup(&self, parts: &[String]) -> Result<&Node, FsError> {
        let mut node = &self.root;
        for part in parts {
            match node {
                Node::Dir(children) => node = children.get(part).ok_or(FsError::NotFound)?,
                Node::File(_) => return Err(FsError::NotADirectory),
            }
        }
        Ok(node)
    }

    fn lookup_mut(&mut self, parts: &[String]) -> Result<&mut Node, FsError> {
        let mut node = &mut self.root;
        for part in parts {
            match node {
                Node::Dir(children) => node = children.get_mut(part).ok_or(FsError::NotFound)?,
                Node::File(_) => return Err(FsError::NotADirectory),
            }
        }
        Ok(node)
    }

    /// Returns the parent directory's entries and the final name in `path`.
    fn parent_mut(&mut self, path: &str) -> Result<(&mut BTreeMap<String, Node>, String), FsError> {
        let mut parts = self.components(path);
        let name = parts.pop().ok_or(FsError::InvalidPath)?;
        match self.lookup_mut(&parts)? {
            Node::Dir(children) => Ok((children, name)),
            Node::File(_) => Err(FsError::NotADirectory),
        }
    }

    pub fn mkdir(&mut self, path: &str) -> Result<(), FsError> {
        let (dir, name) = self.parent_mut(path)?;
        if dir.contains_key(&name) {
            return Err(FsError::AlreadyExists);
        }
        dir.insert(name, Node::Dir(BTreeMap::new()));
        Ok(())
    }

    pub fn touch(&mut self, path: &str) -> Result<(), FsError> {
        let (dir, name) = self.parent_mut(path)?;
        dir.entry(name).or_insert_with(|| Node::File(Vec::new()));
        Ok(())
    }

    pub fn write(&mut self, path: &str, data: &[u8], append: bool) -> Result<(), FsError> {
        let (dir, name) = self.parent_mut(path)?;
        match dir.entry(name).or_insert_with(|| Node::File(Vec::new())) {
            Node::Dir(_) => Err(FsError::IsADirectory),
            Node::File(contents) => {
                if !append {
                    contents.clear();
                }
                contents.extend_from_slice(data);
                Ok(())
            }
        }
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, FsError> {
        match self.lookup(&self.components(path))? {
            Node::File(data) => Ok(data.clone()),
            Node::Dir(_) => Err(FsError::IsADirectory),
        }
    }

    pub fn list(&self, path: &str) -> Result<Vec<DirEntry>, FsError> {
        let parts = self.components(path);
        Ok(match self.lookup(&parts)? {
            Node::Dir(children) => children
                .iter()
                .map(|(name, node)| DirEntry {
                    name: name.clone(),
                    is_dir: matches!(node, Node::Dir(_)),
                    size: node.size(),
                })
                .collect(),
            file => alloc::vec![DirEntry {
                name: parts.last().cloned().unwrap_or_default(),
                is_dir: false,
                size: file.size(),
            }],
        })
    }

    pub fn remove(&mut self, path: &str, recursive: bool) -> Result<(), FsError> {
        let (dir, name) = self.parent_mut(path)?;
        match dir.get(&name) {
            None => return Err(FsError::NotFound),
            Some(Node::Dir(children)) if !children.is_empty() && !recursive => {
                return Err(FsError::NotEmpty)
            }
            _ => {}
        }
        dir.remove(&name);
        Ok(())
    }

    pub fn chdir(&mut self, path: &str) -> Result<(), FsError> {
        let parts = self.components(path);
        match self.lookup(&parts)? {
            Node::Dir(_) => {
                self.cwd = parts;
                Ok(())
            }
            Node::File(_) => Err(FsError::NotADirectory),
        }
    }

    pub fn cwd(&self) -> String {
        if self.cwd.is_empty() {
            return "/".to_string();
        }
        let mut s = String::new();
        for part in &self.cwd {
            s.push('/');
            s.push_str(part);
        }
        s
    }
}
