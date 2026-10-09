//! tmpfs: an in-memory file system (used for `/` and `/tmp`).

use super::vfs::*;
use crate::sync::Mutex;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::Any;
use core::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};
use huldra_abi::errno::Errno;

static NEXT_INO: AtomicU64 = AtomicU64::new(2);

enum Data {
    File(Vec<u8>),
    Dir(BTreeMap<String, Arc<TmpInode>>),
}

pub struct TmpInode {
    ino: u64,
    dev: u64,
    kind: FileType,
    perm: AtomicU32,
    mtime: AtomicI64,
    data: Mutex<Data>,
}

impl TmpInode {
    fn new(dev: u64, ino: u64, kind: FileType, perm: u32) -> Arc<TmpInode> {
        let data = match kind {
            FileType::Directory => Data::Dir(BTreeMap::new()),
            _ => Data::File(Vec::new()),
        };
        Arc::new(TmpInode {
            ino,
            dev,
            kind,
            perm: AtomicU32::new(perm),
            mtime: AtomicI64::new(crate::time::now()),
            data: Mutex::new(data),
        })
    }

    fn touch(&self) {
        self.mtime.store(crate::time::now(), Ordering::Relaxed);
    }
}

impl Inode for TmpInode {
    fn metadata(&self) -> Metadata {
        let mut m = Metadata::new(
            self.dev,
            self.ino,
            self.kind,
            self.perm.load(Ordering::Relaxed),
        );
        m.mtime = self.mtime.load(Ordering::Relaxed);
        match &*self.data.lock() {
            Data::File(v) => m.size = v.len() as u64,
            Data::Dir(d) => {
                m.size = 4096;
                m.nlink = 2 + d.values().filter(|c| c.kind == FileType::Directory).count() as u32;
            }
        }
        m
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> KResult<usize> {
        match &*self.data.lock() {
            Data::Dir(_) => Err(Errno::EISDIR),
            Data::File(v) => {
                let start = (offset as usize).min(v.len());
                let n = buf.len().min(v.len() - start);
                buf[..n].copy_from_slice(&v[start..start + n]);
                Ok(n)
            }
        }
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> KResult<usize> {
        let mut data = self.data.lock();
        match &mut *data {
            Data::Dir(_) => Err(Errno::EISDIR),
            Data::File(v) => {
                let end = offset as usize + buf.len();
                if end > v.len() {
                    v.resize(end, 0);
                }
                v[offset as usize..end].copy_from_slice(buf);
                drop(data);
                self.touch();
                Ok(buf.len())
            }
        }
    }

    fn truncate(&self, size: u64) -> KResult<()> {
        match &mut *self.data.lock() {
            Data::Dir(_) => Err(Errno::EISDIR),
            Data::File(v) => {
                v.resize(size as usize, 0);
                Ok(())
            }
        }
    }

    fn lookup(&self, name: &str) -> KResult<Arc<dyn Inode>> {
        match &*self.data.lock() {
            Data::File(_) => Err(Errno::ENOTDIR),
            Data::Dir(d) => d
                .get(name)
                .map(|n| n.clone() as Arc<dyn Inode>)
                .ok_or(Errno::ENOENT),
        }
    }

    fn create(&self, name: &str, kind: FileType, perm: u32) -> KResult<Arc<dyn Inode>> {
        if !matches!(kind, FileType::Regular | FileType::Directory) {
            return Err(Errno::EPERM);
        }
        let mut data = self.data.lock();
        let Data::Dir(d) = &mut *data else {
            return Err(Errno::ENOTDIR);
        };
        if d.contains_key(name) {
            return Err(Errno::EEXIST);
        }
        let node = TmpInode::new(
            self.dev,
            NEXT_INO.fetch_add(1, Ordering::Relaxed),
            kind,
            perm,
        );
        d.insert(name.to_string(), node.clone());
        drop(data);
        self.touch();
        Ok(node)
    }

    fn symlink(&self, name: &str, target: &str) -> KResult<()> {
        let mut data = self.data.lock();
        let Data::Dir(d) = &mut *data else {
            return Err(Errno::ENOTDIR);
        };
        if d.contains_key(name) {
            return Err(Errno::EEXIST);
        }
        let node = TmpInode::new(self.dev, NEXT_INO.fetch_add(1, Ordering::Relaxed), FileType::Symlink, 0o777);
        *node.data.lock() = Data::File(target.as_bytes().to_vec());
        d.insert(name.to_string(), node);
        drop(data);
        self.touch();
        Ok(())
    }

    fn readlink(&self) -> KResult<String> {
        match (&*self.data.lock(), self.kind) {
            (Data::File(v), FileType::Symlink) => Ok(String::from_utf8_lossy(v).into_owned()),
            _ => Err(Errno::EINVAL),
        }
    }

    fn unlink(&self, name: &str) -> KResult<()> {
        let mut data = self.data.lock();
        let Data::Dir(d) = &mut *data else {
            return Err(Errno::ENOTDIR);
        };
        let node = d.get(name).ok_or(Errno::ENOENT)?;
        if let Data::Dir(children) = &*node.data.lock() {
            if !children.is_empty() {
                return Err(Errno::ENOTEMPTY);
            }
        }
        d.remove(name);
        drop(data);
        self.touch();
        Ok(())
    }

    fn rename(&self, old: &str, target: &Arc<dyn Inode>, new: &str) -> KResult<()> {
        let target = target
            .as_any()
            .downcast_ref::<TmpInode>()
            .ok_or(Errno::EXDEV)?;
        if target.dev != self.dev {
            return Err(Errno::EXDEV);
        }
        fn check_target(d: &BTreeMap<String, Arc<TmpInode>>, new: &str) -> KResult<()> {
            match d.get(new) {
                Some(existing) if existing.kind == FileType::Directory => Err(Errno::EISDIR),
                _ => Ok(()),
            }
        }

        if core::ptr::eq(self, target) {
            let mut data = self.data.lock();
            let Data::Dir(d) = &mut *data else {
                return Err(Errno::ENOTDIR);
            };
            if old == new {
                return d.contains_key(old).then_some(()).ok_or(Errno::ENOENT);
            }
            check_target(d, new)?;
            let node = d.remove(old).ok_or(Errno::ENOENT)?;
            d.insert(new.to_string(), node);
            return Ok(());
        }

        // Lock both directories in address order so concurrent renames
        // in opposite directions cannot deadlock.
        let self_first = (self as *const TmpInode) < (target as *const TmpInode);
        let (mut a, mut b) = if self_first {
            let a = self.data.lock();
            (a, target.data.lock())
        } else {
            let b = target.data.lock();
            (self.data.lock(), b)
        };
        let (Data::Dir(src), Data::Dir(dst)) = (&mut *a, &mut *b) else {
            return Err(Errno::ENOTDIR);
        };
        check_target(dst, new)?;
        let node = src.remove(old).ok_or(Errno::ENOENT)?;
        dst.insert(new.to_string(), node);
        Ok(())
    }

    fn readdir(&self) -> KResult<Vec<DirEntry>> {
        match &*self.data.lock() {
            Data::File(_) => Err(Errno::ENOTDIR),
            Data::Dir(d) => Ok(d
                .iter()
                .map(|(n, c)| DirEntry {
                    name: n.clone(),
                    ino: c.ino,
                    kind: c.kind,
                })
                .collect()),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct TmpFs {
    root: Arc<TmpInode>,
}

impl TmpFs {
    pub fn new() -> Arc<TmpFs> {
        Arc::new(TmpFs {
            root: TmpInode::new(alloc_dev(), 1, FileType::Directory, 0o755),
        })
    }
}

impl FileSystem for TmpFs {
    fn name(&self) -> &'static str {
        "tmpfs"
    }

    fn statfs(&self) -> FsStats {
        // tmpfs lives in RAM: report memory.
        let (free, total) = crate::mm::frame::stats();
        FsStats {
            magic: 0x0102_1994,
            block_size: 4096,
            blocks: total as u64,
            free_blocks: free as u64,
            files: 0,
            free_files: 0,
        }
    }

    fn root(&self) -> Arc<dyn Inode> {
        self.root.clone()
    }
}
