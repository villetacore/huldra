//! Block device layer: the `BlockDevice` interface, a write-back sector
//! cache in front of each disk, and `/dev` nodes for disks.

use crate::fs::vfs::*;
use crate::sync::Mutex;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::any::Any;
use huldra_abi::errno::Errno;

pub const SECTOR_SIZE: usize = 512;
const CACHE_SECTORS: usize = 8192; // 4 MiB per disk

pub trait BlockDevice: Send + Sync {
    fn name(&self) -> &str;
    fn sector_count(&self) -> u64;
    fn read_sectors(&self, lba: u64, buf: &mut [u8]) -> KResult<()>;
    fn write_sectors(&self, lba: u64, buf: &[u8]) -> KResult<()>;
    fn flush(&self) -> KResult<()> {
        Ok(())
    }
}

struct Cache {
    sectors: BTreeMap<u64, (Vec<u8>, bool)>,
    order: VecDeque<u64>,
}

/// A disk with a write-back sector cache, addressed in bytes.
pub struct Disk {
    device: Arc<dyn BlockDevice>,
    cache: Mutex<Cache>,
}

impl Disk {
    pub fn new(device: Arc<dyn BlockDevice>) -> Arc<Disk> {
        Arc::new(Disk { device, cache: Mutex::new(Cache { sectors: BTreeMap::new(), order: VecDeque::new() }) })
    }

    pub fn name(&self) -> &str {
        self.device.name()
    }

    pub fn size(&self) -> u64 {
        self.device.sector_count() * SECTOR_SIZE as u64
    }

    fn with_sector<R>(&self, cache: &mut Cache, lba: u64, f: impl FnOnce(&mut Vec<u8>, &mut bool) -> R) -> KResult<R> {
        if !cache.sectors.contains_key(&lba) {
            if cache.sectors.len() >= CACHE_SECTORS {
                if let Some(victim) = cache.order.pop_front() {
                    if let Some((data, dirty)) = cache.sectors.remove(&victim) {
                        if dirty {
                            self.device.write_sectors(victim, &data)?;
                        }
                    }
                }
            }
            let mut data = vec![0u8; SECTOR_SIZE];
            self.device.read_sectors(lba, &mut data)?;
            cache.sectors.insert(lba, (data, false));
            cache.order.push_back(lba);
        }
        let (data, dirty) = cache.sectors.get_mut(&lba).unwrap();
        Ok(f(data, dirty))
    }

    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> KResult<()> {
        if offset + buf.len() as u64 > self.size() {
            return Err(Errno::EIO);
        }
        let mut cache = self.cache.lock();
        let mut done = 0;
        while done < buf.len() {
            let pos = offset + done as u64;
            let (lba, within) = (pos / SECTOR_SIZE as u64, (pos % SECTOR_SIZE as u64) as usize);
            let n = (SECTOR_SIZE - within).min(buf.len() - done);
            self.with_sector(&mut cache, lba, |data, _| buf[done..done + n].copy_from_slice(&data[within..within + n]))?;
            done += n;
        }
        Ok(())
    }

    pub fn write_at(&self, offset: u64, buf: &[u8]) -> KResult<()> {
        if offset + buf.len() as u64 > self.size() {
            return Err(Errno::EIO);
        }
        let mut cache = self.cache.lock();
        let mut done = 0;
        while done < buf.len() {
            let pos = offset + done as u64;
            let (lba, within) = (pos / SECTOR_SIZE as u64, (pos % SECTOR_SIZE as u64) as usize);
            let n = (SECTOR_SIZE - within).min(buf.len() - done);
            self.with_sector(&mut cache, lba, |data, dirty| {
                data[within..within + n].copy_from_slice(&buf[done..done + n]);
                *dirty = true;
            })?;
            done += n;
        }
        Ok(())
    }

    /// Writes all dirty sectors back to the device.
    pub fn sync(&self) -> KResult<()> {
        let mut cache = self.cache.lock();
        for (&lba, (data, dirty)) in cache.sectors.iter_mut() {
            if *dirty {
                self.device.write_sectors(lba, data)?;
                *dirty = false;
            }
        }
        self.device.flush()
    }
}

/// `/dev/hdX`: raw access to a disk.
pub struct DiskNode {
    pub disk: Arc<Disk>,
    ino: u64,
    rdev: u64,
}

impl Inode for DiskNode {
    fn metadata(&self) -> Metadata {
        let mut m = Metadata::new(0, self.ino, FileType::BlockDevice, 0o660);
        m.rdev = self.rdev;
        m.size = self.disk.size();
        m
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> KResult<usize> {
        let n = (buf.len() as u64).min(self.disk.size().saturating_sub(offset)) as usize;
        self.disk.read_at(offset, &mut buf[..n])?;
        Ok(n)
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> KResult<usize> {
        let n = (buf.len() as u64).min(self.disk.size().saturating_sub(offset)) as usize;
        if n == 0 && !buf.is_empty() {
            return Err(Errno::ENOSPC);
        }
        self.disk.write_at(offset, &buf[..n])?;
        Ok(n)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

static DISKS: Mutex<Vec<Arc<Disk>>> = Mutex::new(Vec::new());

/// Makes a disk available as `/dev/<name>`.
pub fn register(device: Arc<dyn BlockDevice>, major: u32, minor: u32) {
    let name = String::from(device.name());
    let disk = Disk::new(device);
    kinfo!("block: {} ({} MiB)", name, disk.size() >> 20);
    crate::fs::devfs::register(
        &name,
        Arc::new(DiskNode { disk: disk.clone(), ino: crate::fs::devfs::dev_ino(), rdev: makedev(major, minor) }),
    );
    DISKS.lock().push(disk);
}

/// Finds the disk behind a device node path such as `/dev/hda`.
pub fn disk_for_path(path: &str) -> KResult<Arc<Disk>> {
    let node = lookup(path)?;
    let node = node.as_any().downcast_ref::<DiskNode>().ok_or(Errno::ENOTTY)?;
    Ok(node.disk.clone())
}

pub fn sync_all() {
    let disks = DISKS.lock().clone();
    for d in disks {
        if let Err(e) = d.sync() {
            kerror!("{}: sync failed: {}", d.name(), e);
        }
    }
}
