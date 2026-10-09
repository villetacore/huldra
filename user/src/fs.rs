//! Files and directories.

use crate::{sys, Errno, Result};
use alloc::string::String;
use alloc::vec::Vec;
use huldra_abi::fs::*;

pub use huldra_abi::fs::Stat;

/// An open file descriptor, closed on drop.
pub struct File {
    fd: i32,
}

impl File {
    pub fn open(path: &str) -> Result<File> {
        Self::open_with(path, O_RDONLY, 0)
    }

    pub fn create(path: &str) -> Result<File> {
        Self::open_with(path, O_WRONLY | O_CREAT | O_TRUNC, 0o644)
    }

    pub fn open_with(path: &str, flags: u32, mode: u32) -> Result<File> {
        Ok(File {
            fd: sys::open(path, flags | O_CLOEXEC, mode)?,
        })
    }

    pub fn fd(&self) -> i32 {
        self.fd
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        sys::read(self.fd, buf)
    }

    pub fn write_all(&self, data: &[u8]) -> Result<()> {
        crate::io::write_all(self.fd, data)
    }

    pub fn read_to_end(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match self.read(&mut buf) {
                Ok(0) => return Ok(out),
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(Errno::EINTR) => continue,
                Err(e) => return Err(e),
            }
        }
    }

    pub fn stat(&self) -> Result<Stat> {
        sys::fstat(self.fd)
    }

    /// Keeps the descriptor open past this value (e.g. before exec).
    pub fn into_raw(self) -> i32 {
        let fd = self.fd;
        core::mem::forget(self);
        fd
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let _ = sys::close(self.fd);
    }
}

pub fn read(path: &str) -> Result<Vec<u8>> {
    File::open(path)?.read_to_end()
}

pub fn read_to_string(path: &str) -> Result<String> {
    Ok(String::from_utf8_lossy(&read(path)?).into_owned())
}

pub fn write(path: &str, data: &[u8]) -> Result<()> {
    File::create(path)?.write_all(data)
}

pub fn metadata(path: &str) -> Result<Stat> {
    sys::stat(path)
}

pub fn exists(path: &str) -> bool {
    sys::stat(path).is_ok()
}

pub fn is_dir(st: &Stat) -> bool {
    st.st_mode & S_IFMT == S_IFDIR
}

pub fn is_symlink(st: &Stat) -> bool {
    st.st_mode & S_IFMT == S_IFLNK
}

/// Metadata of `path` itself, even if it is a symbolic link.
pub fn symlink_metadata(path: &str) -> Result<Stat> {
    sys::lstat(path)
}

/// Creates the symbolic link `path` pointing to `target`.
pub fn symlink(target: &str, path: &str) -> Result<()> {
    sys::symlink(target, path)
}

pub fn read_link(path: &str) -> Result<String> {
    sys::readlink(path)
}

#[derive(Clone)]
pub struct DirEntry {
    pub name: String,
    pub ino: u64,
    pub d_type: u8,
}

impl DirEntry {
    pub fn is_dir(&self) -> bool {
        self.d_type == DT_DIR
    }

    pub fn is_symlink(&self) -> bool {
        self.d_type == DT_LNK
    }
}

/// Lists a directory (without `.` and `..`), sorted by name.
pub fn read_dir(path: &str) -> Result<Vec<DirEntry>> {
    let dir = File::open_with(path, O_RDONLY | O_DIRECTORY, 0)?;
    let mut entries = Vec::new();
    let mut buf = alloc::vec![0u8; 4096];
    loop {
        let n = sys::getdents64(dir.fd, &mut buf)?;
        if n == 0 {
            break;
        }
        for d in decode_dirents(&buf[..n]) {
            if d.name == b"." || d.name == b".." {
                continue;
            }
            entries.push(DirEntry {
                name: String::from_utf8_lossy(d.name).into_owned(),
                ino: d.ino,
                d_type: d.d_type,
            });
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

pub fn create_dir(path: &str) -> Result<()> {
    sys::mkdir(path, 0o755)
}

/// Creates `path` and missing parents.
pub fn create_dir_all(path: &str) -> Result<()> {
    let mut cur = String::new();
    if path.starts_with('/') {
        cur.push('/');
    }
    for comp in path.split('/').filter(|c| !c.is_empty()) {
        if !cur.is_empty() && !cur.ends_with('/') {
            cur.push('/');
        }
        cur.push_str(comp);
        match sys::mkdir(&cur, 0o755) {
            Ok(()) | Err(Errno::EEXIST) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

pub fn remove_file(path: &str) -> Result<()> {
    sys::unlink(path)
}

pub fn remove_dir(path: &str) -> Result<()> {
    sys::rmdir(path)
}

/// Removes a file or a directory tree. Symbolic links are removed, never
/// followed.
pub fn remove_all(path: &str) -> Result<()> {
    let st = sys::lstat(path)?;
    if is_dir(&st) {
        for e in read_dir(path)? {
            remove_all(&join(path, &e.name))?;
        }
        sys::rmdir(path)
    } else {
        sys::unlink(path)
    }
}

pub fn rename(old: &str, new: &str) -> Result<()> {
    sys::rename(old, new)
}

pub fn current_dir() -> Result<String> {
    sys::getcwd()
}

pub fn set_current_dir(path: &str) -> Result<()> {
    sys::chdir(path)
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        alloc::format!("{}{}", dir, name)
    } else {
        alloc::format!("{}/{}", dir, name)
    }
}

/// `-rw-r--r--` style permission string for a mode.
pub fn mode_string(mode: u32) -> String {
    let kind = match mode & S_IFMT {
        S_IFDIR => 'd',
        S_IFCHR => 'c',
        S_IFBLK => 'b',
        S_IFIFO => 'p',
        S_IFLNK => 'l',
        _ => '-',
    };
    let mut s = String::new();
    s.push(kind);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}
