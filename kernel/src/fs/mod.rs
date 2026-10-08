//! File systems.
//!
//! [`vfs`] defines the `Inode`/`FileSystem` interfaces, mounts and path
//! lookup; [`file`] the open file descriptions and descriptor tables. The
//! functions here implement the path-based operations used by system calls
//! (all paths absolute and normalized, see [`vfs::normalize`]).

pub mod devfs;
pub mod ext2;
pub mod file;
pub mod initrd;
pub mod pipe;
pub mod procfs;
pub mod tmpfs;
pub mod vfs;

pub use file::{FdTable, OpenFile};
pub use vfs::{FileType, Inode, KResult, Metadata};

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use huldra_abi::errno::Errno;
use huldra_abi::fs::*;

/// Mounts the root file system: ext2 on `root_device` (e.g. `/dev/hda`)
/// when given and usable, otherwise a tmpfs filled from the initrd.
pub fn init(root_device: Option<&str>) {
    crate::drivers::tty::init();
    let devfs = devfs::new();
    let disk_root = root_device.and_then(|dev| match mount_disk_root(dev) {
        Ok(()) => Some(dev),
        Err(e) => {
            kerror!("cannot mount root file system {}: {}; falling back to the initrd", dev, e);
            None
        }
    });
    if let Some(dev) = disk_root {
        kinfo!("root file system: {} (ext2)", dev);
        initrd::release_all();
    } else {
        vfs::mount("/", tmpfs::TmpFs::new(), "rootfs").expect("mount /");
        let unpacked = initrd::unpack_all();
        if !unpacked {
            let _ = mkdir("/etc", 0o755);
            let _ = write_file("/etc/hostname", b"huldra\n");
            let _ = write_file("/etc/motd", b"Welcome to Huldra (no initrd loaded).\n");
        }
    }
    for dir in ["/dev", "/proc", "/tmp", "/mnt", "/root"] {
        let _ = mkdir(dir, 0o755);
    }
    vfs::mount("/dev", devfs, "devfs").expect("mount /dev");
    vfs::mount("/proc", procfs::new(), "proc").expect("mount /proc");
    vfs::mount("/tmp", tmpfs::TmpFs::new(), "tmpfs").expect("mount /tmp");
}

fn mount_disk_root(dev: &str) -> KResult<()> {
    let name = dev.strip_prefix("/dev/").ok_or(Errno::EINVAL)?;
    let disk = crate::drivers::block::disk_by_name(name).ok_or(Errno::ENODEV)?;
    vfs::mount("/", ext2::mount(disk)?, dev)
}

/// Flushes every mounted file system.
pub fn sync_all() {
    for m in vfs::mount_list() {
        if let Err(e) = m.fs.sync() {
            kwarn!("sync of {} failed: {}", m.source, e);
        }
    }
    crate::drivers::block::sync_all();
}

/// Mounts a file system of type `fstype` (the `mount(2)` back end).
pub fn mount_by_type(fstype: &str, source: &str, target: &str, _flags: u64) -> KResult<()> {
    let fs: Arc<dyn vfs::FileSystem> = match fstype {
        "tmpfs" => tmpfs::TmpFs::new(),
        "proc" => procfs::new(),
        "devfs" => return Err(Errno::EBUSY),
        other => return crate::fs::mount_block_fs(other, source, target),
    };
    vfs::mount(target, fs, if source.is_empty() { fstype } else { source })
}

/// Mounts a disk-based file system (provided by block device drivers).
pub fn mount_block_fs(fstype: &str, source: &str, target: &str) -> KResult<()> {
    if fstype != "ext2" {
        return Err(Errno::ENODEV);
    }
    let disk = crate::drivers::block::disk_for_path(source)?;
    vfs::mount(target, ext2::mount(disk)?, source)
}

/// Opens (and possibly creates) the file at `path`.
pub fn open(path: &str, flags: u32, perm: u32) -> KResult<Arc<OpenFile>> {
    let inode = match vfs::lookup(path) {
        Ok(inode) => {
            if flags & O_CREAT != 0 && flags & O_EXCL != 0 {
                return Err(Errno::EEXIST);
            }
            inode
        }
        Err(Errno::ENOENT) if flags & O_CREAT != 0 => {
            let (parent, name) = vfs::lookup_parent(path)?;
            parent.create(&name, FileType::Regular, perm & 0o7777)?
        }
        Err(e) => return Err(e),
    };
    let meta = inode.metadata();
    if flags & O_DIRECTORY != 0 && !meta.is_dir() {
        return Err(Errno::ENOTDIR);
    }
    if meta.is_dir() && flags & O_ACCMODE != O_RDONLY {
        return Err(Errno::EISDIR);
    }
    if flags & O_TRUNC != 0 && meta.kind == FileType::Regular && flags & O_ACCMODE != O_RDONLY {
        inode.truncate(0)?;
    }
    OpenFile::new(
        inode,
        flags & !(O_CREAT | O_EXCL | O_TRUNC | O_CLOEXEC),
        String::from(path),
    )
}

pub fn stat(path: &str) -> KResult<Metadata> {
    Ok(vfs::lookup(path)?.metadata())
}

pub fn mkdir(path: &str, perm: u32) -> KResult<()> {
    let (parent, name) = vfs::lookup_parent(path)?;
    parent
        .create(&name, FileType::Directory, perm & 0o7777)
        .map(|_| ())
}

/// Creates `path` and any missing parent directories.
pub fn mkdir_all(path: &str) -> KResult<()> {
    let mut cur = String::new();
    for comp in path.split('/').filter(|c| !c.is_empty()) {
        cur.push('/');
        cur.push_str(comp);
        match mkdir(&cur, 0o755) {
            Ok(()) | Err(Errno::EEXIST) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Removes a non-directory.
pub fn unlink(path: &str) -> KResult<()> {
    let (parent, name) = vfs::lookup_parent(path)?;
    if parent.lookup(&name)?.metadata().is_dir() {
        return Err(Errno::EISDIR);
    }
    parent.unlink(&name)
}

/// Removes an empty directory.
pub fn rmdir(path: &str) -> KResult<()> {
    if vfs::is_mount_point(path) {
        return Err(Errno::EBUSY);
    }
    let (parent, name) = vfs::lookup_parent(path)?;
    let node = parent.lookup(&name)?;
    if !node.metadata().is_dir() {
        return Err(Errno::ENOTDIR);
    }
    if !node.readdir()?.is_empty() {
        return Err(Errno::ENOTEMPTY);
    }
    parent.unlink(&name)
}

pub fn rename(old: &str, new: &str) -> KResult<()> {
    if new.starts_with(old) && new.as_bytes().get(old.len()) == Some(&b'/') {
        return Err(Errno::EINVAL);
    }
    let (old_parent, old_name) = vfs::lookup_parent(old)?;
    let (new_parent, new_name) = vfs::lookup_parent(new)?;
    old_parent.rename(&old_name, &new_parent, &new_name)
}

/// Reads a whole file (kernel convenience).
pub fn read_file(path: &str) -> KResult<Vec<u8>> {
    let inode = vfs::lookup(path)?;
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = inode.read_at(out.len() as u64, &mut buf)?;
        if n == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&buf[..n]);
    }
}

/// Creates or replaces a file (kernel convenience).
pub fn write_file(path: &str, data: &[u8]) -> KResult<()> {
    let f = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0o644)?;
    f.write(data).map(|_| ())
}

pub const TESTS: &[crate::ktest::Test] = ktests![
    tests::files,
    tests::directories,
    tests::rename,
    tests::paths,
    tests::pipes,
    tests::devices,
    tests::procfs,
    tests::getdents,
];

mod tests {
    use super::*;

    pub fn files() {
        write_file("/tmp/t", b"hello").unwrap();
        let f = open("/tmp/t", O_WRONLY | O_APPEND, 0).unwrap();
        f.write(b" world").unwrap();
        assert_eq!(read_file("/tmp/t").unwrap(), b"hello world");
        let f = open("/tmp/t", O_RDONLY, 0).unwrap();
        assert_eq!(f.seek(6, SEEK_SET).unwrap(), 6);
        let mut buf = [0u8; 16];
        assert_eq!(f.read(&mut buf).unwrap(), 5);
        assert_eq!(&buf[..5], b"world");
        assert_eq!(f.write(b"x"), Err(Errno::EBADF));
        assert_eq!(
            open("/tmp/t", O_CREAT | O_EXCL | O_WRONLY, 0o644).err(),
            Some(Errno::EEXIST)
        );
        unlink("/tmp/t").unwrap();
        assert_eq!(stat("/tmp/t").err(), Some(Errno::ENOENT));
    }

    pub fn directories() {
        mkdir("/tmp/d", 0o755).unwrap();
        write_file("/tmp/d/f", b"").unwrap();
        assert_eq!(rmdir("/tmp/d"), Err(Errno::ENOTEMPTY));
        assert_eq!(unlink("/tmp/d"), Err(Errno::EISDIR));
        assert_eq!(open("/tmp/d/f/x", O_RDONLY, 0).err(), Some(Errno::ENOTDIR));
        unlink("/tmp/d/f").unwrap();
        rmdir("/tmp/d").unwrap();
        assert_eq!(rmdir("/proc"), Err(Errno::EBUSY));
    }

    pub fn rename() {
        mkdir_all("/tmp/a/b").unwrap();
        write_file("/tmp/a/b/f", b"data").unwrap();
        super::rename("/tmp/a/b/f", "/tmp/a/g").unwrap();
        assert_eq!(read_file("/tmp/a/g").unwrap(), b"data");
        assert_eq!(super::rename("/tmp/a", "/tmp/a/b/c"), Err(Errno::EINVAL));
        assert_eq!(super::rename("/tmp/a/g", "/etc/g"), Err(Errno::EXDEV));
        unlink("/tmp/a/g").unwrap();
        rmdir("/tmp/a/b").unwrap();
        rmdir("/tmp/a").unwrap();
    }

    pub fn paths() {
        assert_eq!(
            vfs::normalize("/usr/bin", "../lib/./x").unwrap(),
            "/usr/lib/x"
        );
        assert_eq!(vfs::normalize("/", "../../..").unwrap(), "/");
        assert_eq!(vfs::normalize("/a", "/b//c/").unwrap(), "/b/c");
        assert_eq!(vfs::normalize("/", ""), Err(Errno::ENOENT));
    }

    pub fn pipes() {
        let (r, w) = pipe::create().unwrap();
        let writer = crate::task::spawn_kernel("writer", move || {
            for i in 0..100u8 {
                w.write(&[i; 1000]).unwrap();
            }
            0
        });
        let mut total = 0usize;
        let mut buf = [0u8; 777];
        loop {
            let n = r.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            total += n;
        }
        crate::task::join(writer);
        assert_eq!(total, 100_000);
    }

    pub fn devices() {
        let f = open("/dev/zero", O_RDONLY, 0).unwrap();
        let mut buf = [1u8; 64];
        assert_eq!(f.read(&mut buf).unwrap(), 64);
        assert!(buf.iter().all(|&b| b == 0));
        let n = open("/dev/null", O_RDWR, 0).unwrap();
        assert_eq!(n.write(b"gone").unwrap(), 4);
        assert_eq!(n.read(&mut buf).unwrap(), 0);
        assert_eq!(stat("/dev/console").unwrap().kind, FileType::CharDevice);
    }

    pub fn procfs() {
        let up = read_file("/proc/uptime").unwrap();
        assert!(core::str::from_utf8(&up).unwrap().contains('.'));
        let mounts = String::from_utf8(read_file("/proc/mounts").unwrap()).unwrap();
        assert!(mounts.contains("devfs /dev devfs"));
        assert!(stat("/proc/1").unwrap().is_dir());
    }

    pub fn getdents() {
        mkdir_all("/tmp/ls").unwrap();
        for name in ["a", "bb", "ccc"] {
            write_file(&alloc::format!("/tmp/ls/{}", name), b"").unwrap();
        }
        let dir = open("/tmp/ls", O_RDONLY | O_DIRECTORY, 0).unwrap();
        let mut names = Vec::new();
        let mut buf = [0u8; 48]; // small buffer: forces several calls
        loop {
            let n = dir.getdents(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            for d in decode_dirents(&buf[..n]) {
                names.push(String::from_utf8(d.name.to_vec()).unwrap());
            }
        }
        assert_eq!(names, [".", "..", "a", "bb", "ccc"]);
        for name in ["a", "bb", "ccc"] {
            unlink(&alloc::format!("/tmp/ls/{}", name)).unwrap();
        }
        rmdir("/tmp/ls").unwrap();
    }
}
