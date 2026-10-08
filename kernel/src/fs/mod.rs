//! Virtual file system. For now a single in-memory tmpfs mounted at `/`.

mod tmpfs;

pub use tmpfs::{DirEntry, FsError};

use crate::sync::SpinLock;
use alloc::string::String;
use alloc::vec::Vec;
use tmpfs::Tmpfs;

static ROOT: SpinLock<Option<Tmpfs>> = SpinLock::new(None);

fn with<R>(f: impl FnOnce(&mut Tmpfs) -> R) -> R {
    f(ROOT.lock().as_mut().expect("fs not initialized"))
}

pub fn init() {
    let mut fs = Tmpfs::new();
    for dir in ["/bin", "/dev", "/etc", "/home", "/proc", "/root", "/tmp", "/usr", "/var"] {
        fs.mkdir(dir).expect("mkdir");
    }
    fs.write("/etc/hostname", b"huldra\n", false).expect("write");
    fs.write("/etc/os-release", b"NAME=\"Huldra\"\nID=huldra\nVERSION=\"0.1.0\"\n", false)
        .expect("write");
    fs.write(
        "/etc/motd",
        b"Welcome to Huldra, a small Unix-like kernel written in Rust.\nType 'help' to see available commands.\n",
        false,
    )
    .expect("write");
    fs.chdir("/root").expect("chdir");
    *ROOT.lock() = Some(fs);
}

pub fn mkdir(path: &str) -> Result<(), FsError> {
    with(|fs| fs.mkdir(path))
}

pub fn touch(path: &str) -> Result<(), FsError> {
    with(|fs| fs.touch(path))
}

pub fn write(path: &str, data: &[u8], append: bool) -> Result<(), FsError> {
    with(|fs| fs.write(path, data, append))
}

pub fn read(path: &str) -> Result<Vec<u8>, FsError> {
    with(|fs| fs.read(path))
}

pub fn list(path: &str) -> Result<Vec<DirEntry>, FsError> {
    with(|fs| fs.list(path))
}

pub fn remove(path: &str, recursive: bool) -> Result<(), FsError> {
    with(|fs| fs.remove(path, recursive))
}

pub fn chdir(path: &str) -> Result<(), FsError> {
    with(|fs| fs.chdir(path))
}

pub fn cwd() -> String {
    with(|fs| fs.cwd())
}

pub const TESTS: &[crate::ktest::Test] = ktests![tests::write_read, tests::directories];

mod tests {
    use super::*;

    pub fn write_read() {
        write("/tmp/t", b"hello", false).unwrap();
        write("/tmp/t", b" world", true).unwrap();
        assert_eq!(read("/tmp/t").unwrap(), b"hello world");
        remove("/tmp/t", false).unwrap();
        assert_eq!(read("/tmp/t"), Err(FsError::NotFound));
    }

    pub fn directories() {
        mkdir("/tmp/d").unwrap();
        touch("/tmp/d/f").unwrap();
        assert_eq!(remove("/tmp/d", false), Err(FsError::NotEmpty));
        remove("/tmp/d", true).unwrap();
    }
}
