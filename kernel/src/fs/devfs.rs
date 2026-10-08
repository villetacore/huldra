//! devfs: device nodes under `/dev`.

use super::vfs::*;
use crate::sync::SpinLock;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::Any;
use core::sync::atomic::{AtomicU64, Ordering};
use huldra_abi::errno::Errno;

static DEVICES: SpinLock<BTreeMap<String, Arc<dyn Inode>>> = SpinLock::new(BTreeMap::new());
static NEXT_INO: AtomicU64 = AtomicU64::new(2);

/// Device numbers (major, minor) follow Linux where it has an equivalent.
pub fn dev_ino() -> u64 {
    NEXT_INO.fetch_add(1, Ordering::Relaxed)
}

pub fn register(name: &str, node: Arc<dyn Inode>) {
    DEVICES.lock().insert(name.to_string(), node);
}

pub fn get(name: &str) -> Option<Arc<dyn Inode>> {
    DEVICES.lock().get(name).cloned()
}

struct DevRoot {
    dev: u64,
}

impl Inode for DevRoot {
    fn metadata(&self) -> Metadata {
        Metadata::new(self.dev, 1, FileType::Directory, 0o755)
    }

    fn lookup(&self, name: &str) -> KResult<Arc<dyn Inode>> {
        get(name).ok_or(Errno::ENOENT)
    }

    fn readdir(&self) -> KResult<Vec<DirEntry>> {
        Ok(DEVICES
            .lock()
            .iter()
            .map(|(n, d)| {
                let m = d.metadata();
                DirEntry {
                    name: n.clone(),
                    ino: m.ino,
                    kind: m.kind,
                }
            })
            .collect())
    }

    fn create(&self, _name: &str, _kind: FileType, _perm: u32) -> KResult<Arc<dyn Inode>> {
        Err(Errno::EPERM)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct DevFs {
    root: Arc<DevRoot>,
}

impl FileSystem for DevFs {
    fn name(&self) -> &'static str {
        "devfs"
    }

    fn root(&self) -> Arc<dyn Inode> {
        self.root.clone()
    }
}

/// A simple character device defined by read/write functions.
pub struct CharDevice {
    ino: u64,
    rdev: u64,
    perm: u32,
    read: fn(&mut [u8]) -> KResult<usize>,
    write: fn(&[u8]) -> KResult<usize>,
}

impl Inode for CharDevice {
    fn metadata(&self) -> Metadata {
        let mut m = Metadata::new(0, self.ino, FileType::CharDevice, self.perm);
        m.rdev = self.rdev;
        m
    }

    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> KResult<usize> {
        (self.read)(buf)
    }

    fn write_at(&self, _offset: u64, buf: &[u8]) -> KResult<usize> {
        (self.write)(buf)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn char_device(
    rdev: u64,
    read: fn(&mut [u8]) -> KResult<usize>,
    write: fn(&[u8]) -> KResult<usize>,
) -> Arc<dyn Inode> {
    Arc::new(CharDevice {
        ino: dev_ino(),
        rdev,
        perm: 0o666,
        read,
        write,
    })
}

fn discard(buf: &[u8]) -> KResult<usize> {
    Ok(buf.len())
}

fn random_bytes(buf: &mut [u8]) -> KResult<usize> {
    static STATE: SpinLock<u64> = SpinLock::new(0);
    let mut s = STATE.lock();
    if *s == 0 {
        *s = crate::arch::cpu::rdtsc() | 1;
    }
    for b in buf.iter_mut() {
        // xorshift64*: fine for /dev/urandom in a hobby kernel, not for crypto.
        *s ^= *s >> 12;
        *s ^= *s << 25;
        *s ^= *s >> 27;
        *b = (s.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8;
    }
    Ok(buf.len())
}

pub fn new() -> Arc<DevFs> {
    register("null", char_device(makedev(1, 3), |_| Ok(0), discard));
    register(
        "zero",
        char_device(
            makedev(1, 5),
            |b| {
                b.fill(0);
                Ok(b.len())
            },
            discard,
        ),
    );
    register("random", char_device(makedev(1, 8), random_bytes, discard));
    register("urandom", char_device(makedev(1, 9), random_bytes, discard));
    register(
        "kmsg",
        char_device(
            makedev(1, 11),
            |_| Ok(0),
            |b| {
                kinfo!(
                    "{}",
                    core::str::from_utf8(b).unwrap_or("<binary>").trim_end()
                );
                Ok(b.len())
            },
        ),
    );
    Arc::new(DevFs {
        root: Arc::new(DevRoot { dev: alloc_dev() }),
    })
}
