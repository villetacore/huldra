//! ext2 on a block device, adapted to the VFS (`huldra-ext2` does the work).

use super::vfs::*;
use crate::drivers::block::Disk;
use crate::sync::Mutex;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::Any;
use huldra_abi::errno::Errno;
use huldra_ext2::{self as ext2, Ext2};

struct BlockDisk(Arc<Disk>);

impl ext2::Disk for BlockDisk {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> ext2::Result<()> {
        self.0.read_at(offset, buf).map_err(|_| ext2::Error::Io)
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> ext2::Result<()> {
        self.0.write_at(offset, buf).map_err(|_| ext2::Error::Io)
    }
}

fn errno(e: ext2::Error) -> Errno {
    match e {
        ext2::Error::Io => Errno::EIO,
        ext2::Error::Corrupt(what) => {
            kerror!("ext2: file system corrupted: {}", what);
            Errno::EIO
        }
        ext2::Error::Unsupported(_) => Errno::EINVAL,
        ext2::Error::NotFound => Errno::ENOENT,
        ext2::Error::Exists => Errno::EEXIST,
        ext2::Error::NotDir => Errno::ENOTDIR,
        ext2::Error::IsDir => Errno::EISDIR,
        ext2::Error::NotEmpty => Errno::ENOTEMPTY,
        ext2::Error::NoSpace => Errno::ENOSPC,
        ext2::Error::NameTooLong => Errno::ENAMETOOLONG,
        ext2::Error::Invalid => Errno::EINVAL,
        ext2::Error::FileTooLarge => Errno::EFBIG,
    }
}

fn clock() -> u32 {
    crate::time::now() as u32
}

pub struct Ext2Fs {
    inner: Mutex<Ext2<BlockDisk>>,
    dev: u64,
    disk: Arc<Disk>,
}

struct Ext2Inode {
    fs: Arc<Ext2Fs>,
    ino: u32,
}

impl Ext2Fs {
    pub fn mount(disk: Arc<Disk>) -> KResult<Arc<Ext2Fs>> {
        let fs = Ext2::open(BlockDisk(disk.clone()), clock).map_err(|e| {
            kerror!("ext2: cannot mount {}: {:?}", disk.name(), e);
            errno(e)
        })?;
        let st = fs.statfs();
        kinfo!(
            "ext2: mounted {} '{}': {} KiB blocks, {}/{} blocks free",
            disk.name(),
            fs.volume_name(),
            st.block_size / 1024,
            st.free_blocks,
            st.blocks
        );
        Ok(Arc::new(Ext2Fs {
            inner: Mutex::new(fs),
            dev: alloc_dev(),
            disk,
        }))
    }

    fn node(self: &Arc<Self>, ino: u32) -> Arc<dyn Inode> {
        Arc::new(Ext2Inode {
            fs: self.clone(),
            ino,
        })
    }
}

fn kind_of(mode: u16) -> FileType {
    match mode & ext2::S_IFMT {
        ext2::S_IFDIR => FileType::Directory,
        ext2::S_IFCHR => FileType::CharDevice,
        ext2::S_IFBLK => FileType::BlockDevice,
        ext2::S_IFIFO => FileType::Fifo,
        ext2::S_IFLNK => FileType::Symlink,
        _ => FileType::Regular,
    }
}

impl Inode for Ext2Inode {
    fn metadata(&self) -> Metadata {
        let inode = self
            .fs
            .inner
            .lock()
            .read_inode(self.ino)
            .unwrap_or_default();
        let mut m = Metadata::new(
            self.fs.dev,
            self.ino as u64,
            kind_of(inode.mode),
            (inode.mode & 0o7777) as u32,
        );
        m.size = inode.size;
        m.nlink = inode.links as u32;
        m.uid = inode.uid as u32;
        m.gid = inode.gid as u32;
        m.blocks = inode.sectors as u64;
        m.mtime = inode.mtime as i64;
        m
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> KResult<usize> {
        let mut fs = self.fs.inner.lock();
        if fs.read_inode(self.ino).map_err(errno)?.is_dir() {
            return Err(Errno::EISDIR);
        }
        fs.read(self.ino, offset, buf).map_err(errno)
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> KResult<usize> {
        self.fs
            .inner
            .lock()
            .write(self.ino, offset, buf)
            .map_err(errno)
    }

    fn truncate(&self, size: u64) -> KResult<()> {
        self.fs.inner.lock().truncate(self.ino, size).map_err(errno)
    }

    fn lookup(&self, name: &str) -> KResult<Arc<dyn Inode>> {
        let ino = self.fs.inner.lock().lookup(self.ino, name).map_err(errno)?;
        Ok(self.fs.node(ino))
    }

    fn create(&self, name: &str, kind: FileType, perm: u32) -> KResult<Arc<dyn Inode>> {
        let type_bits = match kind {
            FileType::Regular => ext2::S_IFREG,
            FileType::Directory => ext2::S_IFDIR,
            _ => return Err(Errno::EPERM),
        };
        let ino = self
            .fs
            .inner
            .lock()
            .create(self.ino, name, type_bits | (perm & 0o7777) as u16)
            .map_err(errno)?;
        Ok(self.fs.node(ino))
    }

    fn unlink(&self, name: &str) -> KResult<()> {
        self.fs.inner.lock().unlink(self.ino, name).map_err(errno)
    }

    fn rename(&self, old: &str, target: &Arc<dyn Inode>, new: &str) -> KResult<()> {
        let target = target
            .as_any()
            .downcast_ref::<Ext2Inode>()
            .ok_or(Errno::EXDEV)?;
        if !Arc::ptr_eq(&self.fs, &target.fs) {
            return Err(Errno::EXDEV);
        }
        self.fs
            .inner
            .lock()
            .rename(self.ino, old, target.ino, new)
            .map_err(errno)
    }

    fn readdir(&self) -> KResult<Vec<DirEntry>> {
        let entries = self.fs.inner.lock().read_dir(self.ino).map_err(errno)?;
        Ok(entries
            .into_iter()
            .map(|e| DirEntry {
                name: e.name,
                ino: e.ino as u64,
                kind: if e.file_type == ext2::FT_DIR {
                    FileType::Directory
                } else {
                    FileType::Regular
                },
            })
            .collect())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct Ext2Mount(Arc<Ext2Fs>);

impl FileSystem for Ext2Mount {
    fn name(&self) -> &'static str {
        "ext2"
    }

    fn root(&self) -> Arc<dyn Inode> {
        self.0.node(ext2::ROOT_INO)
    }

    fn sync(&self) -> KResult<()> {
        self.0.inner.lock().flush().map_err(errno)?;
        self.0.disk.sync()
    }
}

pub fn mount(disk: Arc<Disk>) -> KResult<Arc<dyn FileSystem>> {
    Ok(Arc::new(Ext2Mount(Ext2Fs::mount(disk)?)))
}
