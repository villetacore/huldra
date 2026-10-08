//! Open file descriptions and per-process file descriptor tables.

use super::vfs::{FileType, Inode, KResult};
use crate::sync::SpinLock;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use huldra_abi::errno::Errno;
use huldra_abi::fs::*;

/// An open file description (`struct file`): shared by duplicated
/// descriptors and across `fork`, so they share the file offset.
pub struct OpenFile {
    pub inode: Arc<dyn Inode>,
    offset: SpinLock<u64>,
    flags: AtomicU32,
    pub path: String,
}

impl OpenFile {
    pub fn new(inode: Arc<dyn Inode>, flags: u32, path: String) -> KResult<Arc<OpenFile>> {
        inode.open(flags)?;
        Ok(Arc::new(OpenFile { inode, offset: SpinLock::new(0), flags: AtomicU32::new(flags), path }))
    }

    pub fn flags(&self) -> u32 {
        self.flags.load(Ordering::Relaxed)
    }

    /// Updates the status flags F_SETFL may change (O_APPEND, O_NONBLOCK).
    pub fn set_status_flags(&self, flags: u32) {
        let keep = self.flags() & !(O_APPEND | O_NONBLOCK);
        self.flags.store(keep | (flags & (O_APPEND | O_NONBLOCK)), Ordering::Relaxed);
    }

    pub fn readable(&self) -> bool {
        self.flags() & O_ACCMODE != O_WRONLY
    }

    pub fn writable(&self) -> bool {
        self.flags() & O_ACCMODE != O_RDONLY
    }

    fn is_seekable(&self) -> bool {
        !matches!(self.inode.metadata().kind, FileType::Fifo | FileType::CharDevice | FileType::Socket)
    }

    pub fn read(&self, buf: &mut [u8]) -> KResult<usize> {
        if !self.readable() {
            return Err(Errno::EBADF);
        }
        let off = *self.offset.lock();
        let n = self.inode.read_at(off, buf)?;
        if self.is_seekable() {
            *self.offset.lock() = off + n as u64;
        }
        Ok(n)
    }

    pub fn write(&self, buf: &[u8]) -> KResult<usize> {
        if !self.writable() {
            return Err(Errno::EBADF);
        }
        let off = if self.flags() & O_APPEND != 0 { self.inode.metadata().size } else { *self.offset.lock() };
        let n = self.inode.write_at(off, buf)?;
        if self.is_seekable() {
            *self.offset.lock() = off + n as u64;
        }
        Ok(n)
    }

    pub fn pread(&self, buf: &mut [u8], off: u64) -> KResult<usize> {
        if !self.is_seekable() {
            return Err(Errno::ESPIPE);
        }
        self.inode.read_at(off, buf)
    }

    pub fn pwrite(&self, buf: &[u8], off: u64) -> KResult<usize> {
        if !self.is_seekable() {
            return Err(Errno::ESPIPE);
        }
        self.inode.write_at(off, buf)
    }

    pub fn seek(&self, off: i64, whence: u32) -> KResult<u64> {
        if !self.is_seekable() {
            return Err(Errno::ESPIPE);
        }
        let mut cur = self.offset.lock();
        let base = match whence {
            SEEK_SET => 0,
            SEEK_CUR => *cur as i64,
            SEEK_END => self.inode.metadata().size as i64,
            _ => return Err(Errno::EINVAL),
        };
        let new = base.checked_add(off).filter(|&n| n >= 0).ok_or(Errno::EINVAL)?;
        *cur = new as u64;
        Ok(new as u64)
    }

    /// Fills `buf` with `linux_dirent64` records; the offset counts entries.
    pub fn getdents(&self, buf: &mut [u8]) -> KResult<usize> {
        let meta = self.inode.metadata();
        if !meta.is_dir() {
            return Err(Errno::ENOTDIR);
        }
        let mut entries = self.inode.readdir()?;
        entries.insert(0, super::vfs::DirEntry { name: "..".into(), ino: meta.ino, kind: FileType::Directory });
        entries.insert(0, super::vfs::DirEntry { name: ".".into(), ino: meta.ino, kind: FileType::Directory });

        let mut cur = self.offset.lock();
        let mut written = 0;
        for (i, e) in entries.iter().enumerate().skip(*cur as usize) {
            match encode_dirent64(&mut buf[written..], e.ino, (i + 1) as i64, e.kind.dirent_type(), e.name.as_bytes()) {
                Some(n) => written += n,
                None if written == 0 => return Err(Errno::EINVAL),
                None => break,
            }
            *cur = (i + 1) as u64;
        }
        Ok(written)
    }
}

impl Drop for OpenFile {
    fn drop(&mut self) {
        self.inode.release(self.flags());
    }
}

pub const MAX_FDS: usize = 256;

#[derive(Clone)]
struct FdEntry {
    file: Arc<OpenFile>,
    cloexec: bool,
}

/// File descriptor table. Cloned on `fork`.
#[derive(Clone, Default)]
pub struct FdTable {
    fds: Vec<Option<FdEntry>>,
}

impl FdTable {
    pub fn new() -> FdTable {
        FdTable { fds: Vec::new() }
    }

    /// Installs `file` at the lowest free descriptor `>= min`.
    pub fn alloc_from(&mut self, min: usize, file: Arc<OpenFile>, cloexec: bool) -> KResult<i32> {
        let fd = (min..MAX_FDS).find(|&i| self.fds.get(i).is_none_or(|e| e.is_none())).ok_or(Errno::EMFILE)?;
        if fd >= self.fds.len() {
            self.fds.resize(fd + 1, None);
        }
        self.fds[fd] = Some(FdEntry { file, cloexec });
        Ok(fd as i32)
    }

    pub fn alloc(&mut self, file: Arc<OpenFile>, cloexec: bool) -> KResult<i32> {
        self.alloc_from(0, file, cloexec)
    }

    pub fn get(&self, fd: i32) -> KResult<Arc<OpenFile>> {
        self.fds.get(fd as usize).and_then(|e| e.as_ref()).map(|e| e.file.clone()).ok_or(Errno::EBADF)
    }

    pub fn close(&mut self, fd: i32) -> KResult<()> {
        let slot = self.fds.get_mut(fd as usize).ok_or(Errno::EBADF)?;
        slot.take().map(|_| ()).ok_or(Errno::EBADF)
    }

    /// Makes `new` refer to the same file as `old` (closing `new` first).
    pub fn dup2(&mut self, old: i32, new: i32, cloexec: bool) -> KResult<i32> {
        let file = self.get(old)?;
        let new_idx = new as usize;
        if new < 0 || new_idx >= MAX_FDS {
            return Err(Errno::EBADF);
        }
        if new_idx >= self.fds.len() {
            self.fds.resize(new_idx + 1, None);
        }
        self.fds[new_idx] = Some(FdEntry { file, cloexec });
        Ok(new)
    }

    pub fn cloexec(&self, fd: i32) -> KResult<bool> {
        self.fds.get(fd as usize).and_then(|e| e.as_ref()).map(|e| e.cloexec).ok_or(Errno::EBADF)
    }

    pub fn set_cloexec(&mut self, fd: i32, on: bool) -> KResult<()> {
        let e = self.fds.get_mut(fd as usize).and_then(|e| e.as_mut()).ok_or(Errno::EBADF)?;
        e.cloexec = on;
        Ok(())
    }

    pub fn close_on_exec(&mut self) {
        for slot in self.fds.iter_mut() {
            if slot.as_ref().is_some_and(|e| e.cloexec) {
                *slot = None;
            }
        }
    }

    pub fn close_all(&mut self) {
        self.fds.clear();
    }

    /// (fd, file) pairs for /proc/<pid>/fd.
    pub fn iter(&self) -> impl Iterator<Item = (i32, &Arc<OpenFile>)> {
        self.fds.iter().enumerate().filter_map(|(i, e)| e.as_ref().map(|e| (i as i32, &e.file)))
    }
}
