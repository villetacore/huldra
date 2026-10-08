//! Creating the ext2 disk image (`target/disk.img`) from `diskfs/`, and
//! checking images with e2fsck when it is available.

use crate::Result;
use huldra_ext2::{Disk, Ext2, ROOT_INO, S_IFDIR, S_IFREG};
use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
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

fn copy_tree(fs: &mut Ext2<FileDisk>, dir_ino: u32, src: &Path) -> Result {
    let mut entries: Vec<_> = fs::read_dir(src)
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        let err = |x: huldra_ext2::Error| format!("{}: {:?}", path.display(), x);
        if path.is_dir() {
            let ino = fs.create(dir_ino, &name, S_IFDIR | 0o755).map_err(err)?;
            copy_tree(fs, ino, &path)?;
        } else {
            let data = fs::read(&path).map_err(|x| x.to_string())?;
            let mode = if name.ends_with(".sh") { 0o755 } else { 0o644 };
            let ino = fs.create(dir_ino, &name, S_IFREG | mode).map_err(err)?;
            fs.write(ino, 0, &data).map_err(err)?;
        }
    }
    Ok(())
}

/// Formats `image` and fills it with the contents of `source`.
pub fn create_image(image: &Path, source: &Path) -> Result {
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
    let mut fs = Ext2::open(disk, now).map_err(|e| format!("mount: {e:?}"))?;
    if source.is_dir() {
        copy_tree(&mut fs, ROOT_INO, source)?;
    }
    fs.flush().map_err(|e| format!("flush: {e:?}"))?;
    Ok(())
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
