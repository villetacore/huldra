//! The ext2 system disk image (`target/disk.img`): creating and updating it
//! with the host-side ext2 library, and checking it with e2fsck.

use crate::Result;
use huldra_ext2::{Disk, Ext2, ROOT_INO, S_IFDIR, S_IFREG};
use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use crate::image::ImageFile;
use std::path::Path;
use std::process::Command;

pub const DISK_SIZE: u64 = 32 << 20;

struct FileDisk(RefCell<File>);

impl Disk for FileDisk {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> huldra_ext2::Result<()> {
        let mut f = self.0.borrow_mut();
        f.seek(SeekFrom::Start(offset))
            .map_err(|_| huldra_ext2::Error::Io)?;
        f.read_exact(buf).map_err(|_| huldra_ext2::Error::Io)
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> huldra_ext2::Result<()> {
        let mut f = self.0.borrow_mut();
        f.seek(SeekFrom::Start(offset))
            .map_err(|_| huldra_ext2::Error::Io)?;
        f.write_all(buf).map_err(|_| huldra_ext2::Error::Io)
    }
}

fn now() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as u32)
}

fn is_elf(data: &[u8]) -> bool {
    data.starts_with(b"\x7fELF")
}

fn open_fs(image: &Path) -> Result<Ext2<FileDisk>> {
    let file = OpenOptions::new().read(true).write(true).open(image).map_err(|e| e.to_string())?;
    Ext2::open(FileDisk(RefCell::new(file)), now).map_err(|e| format!("{}: {e:?}", image.display()))
}

/// Looks up or creates the directory `path` (relative, `/` separated).
fn ensure_dir(fs: &mut Ext2<FileDisk>, path: &str) -> Result<u32> {
    let mut ino = ROOT_INO;
    for comp in path.split('/').filter(|c| !c.is_empty()) {
        ino = match fs.lookup(ino, comp) {
            Ok(i) => i,
            Err(huldra_ext2::Error::NotFound) => fs.create(ino, comp, S_IFDIR | 0o755).map_err(|e| format!("{path}: {e:?}"))?,
            Err(e) => return Err(format!("{path}: {e:?}")),
        };
    }
    Ok(ino)
}

/// Writes one file (or directory, for a `dest` ending in `/`). Existing
/// files are replaced only when `overwrite` is set. Returns true if written.
fn put(fs: &mut Ext2<FileDisk>, f: &ImageFile, overwrite: bool) -> Result<bool> {
    if f.dest.ends_with('/') {
        ensure_dir(fs, &f.dest)?;
        return Ok(false);
    }
    let (dir, name) = match f.dest.rfind('/') {
        Some(i) => (&f.dest[..i], &f.dest[i + 1..]),
        None => ("", f.dest.as_str()),
    };
    let parent = ensure_dir(fs, dir)?;
    match fs.lookup(parent, name) {
        Ok(_) if !overwrite => return Ok(false),
        Ok(_) => fs.unlink(parent, name).map_err(|e| format!("{}: {e:?}", f.dest))?,
        Err(huldra_ext2::Error::NotFound) => {}
        Err(e) => return Err(format!("{}: {e:?}", f.dest)),
    }
    let data = fs::read(&f.source).map_err(|e| format!("{}: {e}", f.source.display()))?;
    let exec = f.mode & 0o111 != 0 || is_elf(&data) || name.ends_with(".sh");
    let mode = if exec { 0o755 } else { f.mode & 0o777 };
    let ino = fs.create(parent, name, S_IFREG | mode as u16).map_err(|e| format!("{}: {e:?}", f.dest))?;
    fs.write(ino, 0, &data).map_err(|e| format!("{}: {e:?}", f.dest))?;
    Ok(true)
}

/// Files of a host directory, placed below `prefix` in the image.
pub fn tree(prefix: &str, dir: &Path) -> Result<Vec<ImageFile>> {
    let mut files = crate::image::collect_tree(dir)?;
    for f in files.iter_mut() {
        f.dest = format!("{}/{}", prefix.trim_matches('/'), f.dest);
    }
    Ok(files)
}

/// Formats `image` and installs `files`.
pub fn create_image(image: &Path, files: &[ImageFile]) -> Result {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(image)
        .map_err(|e| e.to_string())?;
    file.set_len(DISK_SIZE).map_err(|e| e.to_string())?;
    let disk = FileDisk(RefCell::new(file));
    huldra_ext2::format(&disk, DISK_SIZE, "huldra", now()).map_err(|e| format!("mkfs: {e:?}"))?;
    drop(disk);
    let mut fs = open_fs(image)?;
    for f in files {
        put(&mut fs, f, true)?;
    }
    fs.flush().map_err(|e| format!("flush: {e:?}"))?;
    Ok(())
}

/// Updates an existing image: replaces files for which `replace` is true,
/// adds missing ones and leaves everything else (user data, /etc) alone.
pub fn update_image(image: &Path, files: &[ImageFile], replace: impl Fn(&str) -> bool) -> Result<usize> {
    let mut fs = open_fs(image)?;
    let mut changed = 0;
    for f in files {
        if put(&mut fs, f, replace(&f.dest))? {
            changed += 1;
        }
    }
    fs.flush().map_err(|e| format!("flush: {e:?}"))?;
    Ok(changed)
}

/// Runs `e2fsck -fn` on the image (natively or through WSL). Returns
/// Ok(false) when e2fsck is not available.
pub fn fsck(image: &Path) -> Result<bool> {
    let (program, args): (&str, Vec<String>) = if cfg!(windows) {
        let s = image.display().to_string().replace('\\', "/");
        let (drive, rest) = s.split_at(1);
        let wsl = format!("/mnt/{}{}", drive.to_lowercase(), &rest[1..]);
        (
            "wsl",
            vec!["-e".into(), "/usr/sbin/e2fsck".into(), "-fn".into(), wsl],
        )
    } else {
        ("e2fsck", vec!["-fn".into(), image.display().to_string()])
    };
    let output = match Command::new(program).args(&args).output() {
        Ok(o) => o,
        Err(_) => return Ok(false),
    };
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    if text.contains("not found") && !output.status.success() && output.status.code() == Some(127) {
        return Ok(false);
    }
    if output.status.success() {
        print!("{text}");
        Ok(true)
    } else {
        Err(format!(
            "e2fsck reports problems ({:?}):\n{text}",
            output.status.code()
        ))
    }
}

/// Reads `path` from the image (used to check what the guest wrote).
pub fn read_file(image: &Path, path: &str) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(image)
        .map_err(|e| e.to_string())?;
    let mut fs =
        Ext2::open(FileDisk(RefCell::new(file)), now).map_err(|e| format!("mount: {e:?}"))?;
    let mut ino = ROOT_INO;
    for comp in path.split('/').filter(|c| !c.is_empty()) {
        ino = fs.lookup(ino, comp).map_err(|e| format!("{path}: {e:?}"))?;
    }
    let size = fs.read_inode(ino).map_err(|e| format!("{e:?}"))?.size as usize;
    let mut buf = vec![0u8; size];
    fs.read(ino, 0, &mut buf).map_err(|e| format!("{e:?}"))?;
    Ok(buf)
}
