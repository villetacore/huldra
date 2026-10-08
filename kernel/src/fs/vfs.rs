//! Virtual file system core: the `Inode` and `FileSystem` traits, the mount
//! table and path resolution.
//!
//! Paths are resolved lexically: callers turn a path into a normalized
//! absolute path (resolving `.` and `..` against the working directory), then
//! [`lookup`] walks it from the root, crossing into mounted file systems at
//! mount points. There are no symbolic links yet, so this matches POSIX
//! semantics.

use crate::sync::SpinLock;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::Any;
use core::sync::atomic::{AtomicU64, Ordering};
use huldra_abi::errno::Errno;
use huldra_abi::fs as abi;

pub type KResult<T> = Result<T, Errno>;

pub const PATH_MAX: usize = 4096;
pub const NAME_MAX: usize = 255;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileType {
    Regular,
    Directory,
    CharDevice,
    BlockDevice,
    Fifo,
    Symlink,
    Socket,
}

impl FileType {
    pub fn mode_bits(self) -> u32 {
        match self {
            FileType::Regular => abi::S_IFREG,
            FileType::Directory => abi::S_IFDIR,
            FileType::CharDevice => abi::S_IFCHR,
            FileType::BlockDevice => abi::S_IFBLK,
            FileType::Fifo => abi::S_IFIFO,
            FileType::Symlink => abi::S_IFLNK,
            FileType::Socket => abi::S_IFSOCK,
        }
    }

    pub fn dirent_type(self) -> u8 {
        match self {
            FileType::Regular => abi::DT_REG,
            FileType::Directory => abi::DT_DIR,
            FileType::CharDevice => abi::DT_CHR,
            FileType::BlockDevice => abi::DT_BLK,
            FileType::Fifo => abi::DT_FIFO,
            FileType::Symlink => abi::DT_LNK,
            FileType::Socket => abi::DT_UNKNOWN,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Metadata {
    pub dev: u64,
    pub ino: u64,
    pub kind: FileType,
    /// Permission bits (`0o7777`).
    pub perm: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    /// Device number for device files.
    pub rdev: u64,
    pub blocks: u64,
    pub mtime: i64,
}

impl Metadata {
    pub fn new(dev: u64, ino: u64, kind: FileType, perm: u32) -> Metadata {
        Metadata {
            dev,
            ino,
            kind,
            perm,
            nlink: if kind == FileType::Directory { 2 } else { 1 },
            uid: 0,
            gid: 0,
            size: 0,
            rdev: 0,
            blocks: 0,
            mtime: crate::time::now(),
        }
    }

    pub fn is_dir(&self) -> bool {
        self.kind == FileType::Directory
    }

    pub fn to_stat(&self) -> abi::Stat {
        abi::Stat {
            st_dev: self.dev,
            st_ino: self.ino,
            st_nlink: self.nlink as u64,
            st_mode: self.kind.mode_bits() | (self.perm & 0o7777),
            st_uid: self.uid,
            st_gid: self.gid,
            st_rdev: self.rdev,
            st_size: self.size as i64,
            st_blksize: 4096,
            st_blocks: if self.blocks != 0 { self.blocks as i64 } else { self.size.div_ceil(512) as i64 },
            st_atime: self.mtime,
            st_mtime: self.mtime,
            st_ctime: self.mtime,
            ..abi::Stat::default()
        }
    }
}

/// Linux-style device number.
pub const fn makedev(major: u32, minor: u32) -> u64 {
    ((major as u64) << 8) | minor as u64
}

#[derive(Clone, Debug)]
pub struct DirEntry {
    pub name: String,
    pub ino: u64,
    pub kind: FileType,
}

/// A file system object. Methods a given kind of node does not support keep
/// their default implementation, which reports the POSIX error.
pub trait Inode: Send + Sync + Any {
    fn metadata(&self) -> Metadata;

    fn read_at(&self, _offset: u64, _buf: &mut [u8]) -> KResult<usize> {
        Err(if self.metadata().is_dir() { Errno::EISDIR } else { Errno::EINVAL })
    }

    fn write_at(&self, _offset: u64, _buf: &[u8]) -> KResult<usize> {
        Err(if self.metadata().is_dir() { Errno::EISDIR } else { Errno::EINVAL })
    }

    fn truncate(&self, _size: u64) -> KResult<()> {
        Err(Errno::EINVAL)
    }

    fn lookup(&self, _name: &str) -> KResult<Arc<dyn Inode>> {
        Err(Errno::ENOTDIR)
    }

    fn create(&self, _name: &str, _kind: FileType, _perm: u32) -> KResult<Arc<dyn Inode>> {
        Err(Errno::ENOTDIR)
    }

    /// Removes the entry `name`. Directories must be empty (checked by the VFS).
    fn unlink(&self, _name: &str) -> KResult<()> {
        Err(Errno::ENOTDIR)
    }

    /// Moves entry `old` of this directory to `new` in `target` (same file system).
    fn rename(&self, _old: &str, _target: &Arc<dyn Inode>, _new: &str) -> KResult<()> {
        Err(Errno::EXDEV)
    }

    fn readdir(&self) -> KResult<Vec<DirEntry>> {
        Err(Errno::ENOTDIR)
    }

    fn ioctl(&self, _cmd: u32, _arg: u64) -> KResult<u64> {
        Err(Errno::ENOTTY)
    }

    /// Called when a file description referring to this node is created.
    fn open(&self, _flags: u32) -> KResult<()> {
        Ok(())
    }

    /// Called when such a file description is closed for the last time.
    fn release(&self, _flags: u32) {}

    /// Bytes that can be read without blocking (FIONREAD), if meaningful.
    fn bytes_available(&self) -> Option<usize> {
        None
    }

    fn as_any(&self) -> &dyn Any;
}

pub trait FileSystem: Send + Sync {
    fn name(&self) -> &'static str;
    fn root(&self) -> Arc<dyn Inode>;
    fn sync(&self) -> KResult<()> {
        Ok(())
    }
}

static NEXT_DEV: AtomicU64 = AtomicU64::new(1);

/// Allocates a device number for an in-memory file system instance.
pub fn alloc_dev() -> u64 {
    makedev(0, NEXT_DEV.fetch_add(1, Ordering::Relaxed) as u32)
}

#[derive(Clone)]
pub struct Mount {
    pub fs: Arc<dyn FileSystem>,
    pub source: String,
}

static MOUNTS: SpinLock<BTreeMap<String, Mount>> = SpinLock::new(BTreeMap::new());

fn mount_table() -> BTreeMap<String, Mount> {
    MOUNTS.lock().clone()
}

/// Mounts `fs` at the absolute path `target`.
pub fn mount(target: &str, fs: Arc<dyn FileSystem>, source: &str) -> KResult<()> {
    let target = normalize("/", target)?;
    if target != "/" {
        if !lookup(&target)?.metadata().is_dir() {
            return Err(Errno::ENOTDIR);
        }
    }
    let mut mounts = MOUNTS.lock();
    if mounts.contains_key(&target) {
        return Err(Errno::EBUSY);
    }
    mounts.insert(target, Mount { fs, source: source.to_string() });
    Ok(())
}

pub fn umount(target: &str) -> KResult<()> {
    let target = normalize("/", target)?;
    if target == "/" {
        return Err(Errno::EBUSY);
    }
    let mut mounts = MOUNTS.lock();
    let prefix = alloc::format!("{}/", target);
    if mounts.keys().any(|k| k.starts_with(&prefix)) {
        return Err(Errno::EBUSY);
    }
    let m = mounts.remove(&target).ok_or(Errno::EINVAL)?;
    drop(mounts);
    m.fs.sync()
}

/// (mount point, source, file system type) for every mount.
pub fn mounts() -> Vec<(String, String, &'static str)> {
    mount_table().into_iter().map(|(p, m)| (p, m.source, m.fs.name())).collect()
}

pub fn is_mount_point(path: &str) -> bool {
    MOUNTS.lock().contains_key(path)
}

/// Turns `path` into a normalized absolute path (relative paths are
/// resolved against `cwd`, which must be absolute and normalized).
pub fn normalize(cwd: &str, path: &str) -> KResult<String> {
    if path.is_empty() {
        return Err(Errno::ENOENT);
    }
    if path.len() > PATH_MAX {
        return Err(Errno::ENAMETOOLONG);
    }
    let mut parts: Vec<&str> = Vec::new();
    if !path.starts_with('/') {
        parts.extend(cwd.split('/').filter(|c| !c.is_empty()));
    }
    for comp in path.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            name if name.len() > NAME_MAX => return Err(Errno::ENAMETOOLONG),
            name => parts.push(name),
        }
    }
    let mut out = String::with_capacity(path.len() + 1);
    for p in &parts {
        out.push('/');
        out.push_str(p);
    }
    if out.is_empty() {
        out.push('/');
    }
    Ok(out)
}

/// Looks up a normalized absolute path.
pub fn lookup(path: &str) -> KResult<Arc<dyn Inode>> {
    let mounts = mount_table();
    let mut node = mounts.get("/").ok_or(Errno::ENOENT)?.fs.root();
    let mut current = String::with_capacity(path.len());
    for comp in path.split('/').filter(|c| !c.is_empty()) {
        if !node.metadata().is_dir() {
            return Err(Errno::ENOTDIR);
        }
        node = node.lookup(comp)?;
        current.push('/');
        current.push_str(comp);
        if let Some(m) = mounts.get(&current) {
            node = m.fs.root();
        }
    }
    Ok(node)
}

/// Splits a normalized absolute path into its parent directory and last
/// component, and looks up the parent.
pub fn lookup_parent(path: &str) -> KResult<(Arc<dyn Inode>, String)> {
    if path == "/" {
        return Err(Errno::EBUSY);
    }
    let split = path.rfind('/').unwrap_or(0);
    let (dir, name) = (&path[..split], &path[split + 1..]);
    let parent = lookup(if dir.is_empty() { "/" } else { dir })?;
    if !parent.metadata().is_dir() {
        return Err(Errno::ENOTDIR);
    }
    Ok((parent, name.to_string()))
}
