//! The ext2 file system: mounting, reading, writing and formatting.
//!
//! Supports revision 0 and 1 file systems with any block size, direct and
//! single/double/triple indirect blocks, the `filetype` directory feature,
//! `sparse_super` and `large_file`. Journals (ext3) and extents (ext4) are
//! not supported. All access goes through the [`Disk`] trait, so the same
//! code runs in the kernel and on the build host (for `mkfs`).

#![no_std]

extern crate alloc;

mod format;

pub use format::format;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Byte-addressed storage holding the file system.
pub trait Disk {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()>;
    fn write_at(&self, offset: u64, buf: &[u8]) -> Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Io,
    Corrupt(&'static str),
    Unsupported(&'static str),
    NotFound,
    Exists,
    NotDir,
    IsDir,
    NotEmpty,
    NoSpace,
    NameTooLong,
    Invalid,
    FileTooLarge,
}

pub type Result<T> = core::result::Result<T, Error>;

pub const ROOT_INO: u32 = 2;
pub const MAGIC: u16 = 0xEF53;

pub const S_IFMT: u16 = 0o170000;
pub const S_IFREG: u16 = 0o100000;
pub const S_IFDIR: u16 = 0o040000;
pub const S_IFCHR: u16 = 0o020000;
pub const S_IFBLK: u16 = 0o060000;
pub const S_IFIFO: u16 = 0o010000;
pub const S_IFLNK: u16 = 0o120000;

pub const FT_UNKNOWN: u8 = 0;
pub const FT_REG: u8 = 1;
pub const FT_DIR: u8 = 2;
pub const FT_SYMLINK: u8 = 7;

/// Symbolic link targets shorter than this live in the inode's block
/// pointers ("fast" symlinks); longer ones get a data block.
const FAST_SYMLINK_MAX: usize = 60;

pub(crate) const INCOMPAT_FILETYPE: u32 = 0x2;
pub(crate) const RO_COMPAT_SPARSE_SUPER: u32 = 0x1;
pub(crate) const RO_COMPAT_LARGE_FILE: u32 = 0x2;
const SUPPORTED_INCOMPAT: u32 = INCOMPAT_FILETYPE;
const SUPPORTED_RO_COMPAT: u32 = RO_COMPAT_SPARSE_SUPER | RO_COMPAT_LARGE_FILE;

pub(crate) fn get16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
pub(crate) fn get32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
pub(crate) fn put16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
pub(crate) fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

/// The fields of an on-disk inode this implementation uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inode {
    pub mode: u16,
    pub uid: u16,
    pub size: u64,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub gid: u16,
    pub links: u16,
    /// Allocated space in 512-byte units (data and indirect blocks).
    pub sectors: u32,
    pub flags: u32,
    pub block: [u32; 15],
}

impl Inode {
    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }

    pub fn is_regular(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }

    pub fn is_symlink(&self) -> bool {
        self.mode & S_IFMT == S_IFLNK
    }

    /// A symlink whose target is stored in `block` instead of a data block.
    fn is_fast_symlink(&self) -> bool {
        self.is_symlink() && self.sectors == 0
    }

    fn decode(b: &[u8]) -> Inode {
        let mut block = [0u32; 15];
        for (i, slot) in block.iter_mut().enumerate() {
            *slot = get32(b, 40 + i * 4);
        }
        let mode = get16(b, 0);
        let high = if mode & S_IFMT == S_IFREG {
            get32(b, 108) as u64
        } else {
            0
        };
        Inode {
            mode,
            uid: get16(b, 2),
            size: get32(b, 4) as u64 | high << 32,
            atime: get32(b, 8),
            ctime: get32(b, 12),
            mtime: get32(b, 16),
            dtime: get32(b, 20),
            gid: get16(b, 24),
            links: get16(b, 26),
            sectors: get32(b, 28),
            flags: get32(b, 32),
            block,
        }
    }

    fn encode(&self, b: &mut [u8]) {
        put16(b, 0, self.mode);
        put16(b, 2, self.uid);
        put32(b, 4, self.size as u32);
        put32(b, 8, self.atime);
        put32(b, 12, self.ctime);
        put32(b, 16, self.mtime);
        put32(b, 20, self.dtime);
        put16(b, 24, self.gid);
        put16(b, 26, self.links);
        put32(b, 28, self.sectors);
        put32(b, 32, self.flags);
        for (i, v) in self.block.iter().enumerate() {
            put32(b, 40 + i * 4, *v);
        }
        if self.is_regular() {
            put32(b, 108, (self.size >> 32) as u32);
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Group {
    block_bitmap: u32,
    inode_bitmap: u32,
    inode_table: u32,
    free_blocks: u16,
    free_inodes: u16,
    used_dirs: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub ino: u32,
    pub file_type: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatFs {
    pub block_size: u32,
    pub blocks: u32,
    pub free_blocks: u32,
    pub inodes: u32,
    pub free_inodes: u32,
}

pub struct Ext2<D: Disk> {
    disk: D,
    clock: fn() -> u32,
    superblock: Vec<u8>,
    gdt: Vec<u8>,
    groups: Vec<Group>,
    block_size: u32,
    blocks_count: u32,
    inodes_count: u32,
    first_data_block: u32,
    blocks_per_group: u32,
    inodes_per_group: u32,
    inode_size: u32,
    first_ino: u32,
    free_blocks: u32,
    free_inodes: u32,
    dirty: bool,
}

/// Minimum record length for a directory entry with an `n`-byte name.
fn rec_len_for(n: usize) -> usize {
    (8 + n + 3) & !3
}

fn file_type_of(mode: u16) -> u8 {
    match mode & S_IFMT {
        S_IFREG => 1,
        S_IFDIR => 2,
        S_IFCHR => 3,
        S_IFBLK => 4,
        S_IFIFO => 5,
        0o140000 => 6,
        S_IFLNK => 7,
        _ => FT_UNKNOWN,
    }
}

fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err(Error::Invalid);
    }
    if name.len() > 255 {
        return Err(Error::NameTooLong);
    }
    Ok(())
}

impl<D: Disk> Ext2<D> {
    /// Mounts the file system on `disk`. `clock` returns the current time.
    pub fn open(disk: D, clock: fn() -> u32) -> Result<Ext2<D>> {
        let mut sb = vec![0u8; 1024];
        disk.read_at(1024, &mut sb)?;
        if get16(&sb, 56) != MAGIC {
            return Err(Error::Corrupt("bad superblock magic"));
        }
        let rev = get32(&sb, 76);
        let (inode_size, first_ino, incompat, ro_compat) = if rev >= 1 {
            (
                get16(&sb, 88) as u32,
                get32(&sb, 84),
                get32(&sb, 96),
                get32(&sb, 100),
            )
        } else {
            (128, 11, 0, 0)
        };
        if incompat & !SUPPORTED_INCOMPAT != 0 {
            return Err(Error::Unsupported(
                "incompatible features (ext3 journal recovery, ext4 extents, ...)",
            ));
        }
        if ro_compat & !SUPPORTED_RO_COMPAT != 0 {
            return Err(Error::Unsupported("read-only compatible features"));
        }
        let log = get32(&sb, 24);
        if log > 6 {
            return Err(Error::Corrupt("block size"));
        }
        let block_size = 1024u32 << log;
        let blocks_count = get32(&sb, 4);
        let first_data_block = get32(&sb, 20);
        let blocks_per_group = get32(&sb, 32);
        let inodes_per_group = get32(&sb, 40);
        if blocks_per_group == 0
            || inodes_per_group == 0
            || inode_size < 128
            || inode_size > block_size
        {
            return Err(Error::Corrupt("geometry"));
        }
        let ngroups = (blocks_count - first_data_block).div_ceil(blocks_per_group) as usize;
        let gdt_offset = (first_data_block as u64 + 1) * block_size as u64;
        let mut gdt = vec![0u8; ngroups * 32];
        disk.read_at(gdt_offset, &mut gdt)?;
        let groups = (0..ngroups)
            .map(|g| {
                let d = &gdt[g * 32..];
                Group {
                    block_bitmap: get32(d, 0),
                    inode_bitmap: get32(d, 4),
                    inode_table: get32(d, 8),
                    free_blocks: get16(d, 12),
                    free_inodes: get16(d, 14),
                    used_dirs: get16(d, 16),
                }
            })
            .collect();
        Ok(Ext2 {
            disk,
            clock,
            free_blocks: get32(&sb, 12),
            free_inodes: get32(&sb, 16),
            inodes_count: get32(&sb, 0),
            superblock: sb,
            gdt,
            groups,
            block_size,
            blocks_count,
            first_data_block,
            blocks_per_group,
            inodes_per_group,
            inode_size,
            first_ino,
            dirty: false,
        })
    }

    pub fn disk(&self) -> &D {
        &self.disk
    }

    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    pub fn statfs(&self) -> StatFs {
        StatFs {
            block_size: self.block_size,
            blocks: self.blocks_count,
            free_blocks: self.free_blocks,
            inodes: self.inodes_count,
            free_inodes: self.free_inodes,
        }
    }

    pub fn volume_name(&self) -> String {
        let raw = &self.superblock[120..136];
        let n = raw.iter().position(|&b| b == 0).unwrap_or(16);
        String::from_utf8_lossy(&raw[..n]).into_owned()
    }

    fn now(&self) -> u32 {
        (self.clock)()
    }

    fn block_offset(&self, block: u32) -> u64 {
        block as u64 * self.block_size as u64
    }

    fn read_block(&self, block: u32) -> Result<Vec<u8>> {
        if block == 0 || block >= self.blocks_count {
            return Err(Error::Corrupt("block number out of range"));
        }
        let mut buf = vec![0u8; self.block_size as usize];
        self.disk.read_at(self.block_offset(block), &mut buf)?;
        Ok(buf)
    }

    fn write_block(&self, block: u32, data: &[u8]) -> Result<()> {
        if block == 0 || block >= self.blocks_count {
            return Err(Error::Corrupt("block number out of range"));
        }
        self.disk.write_at(self.block_offset(block), data)
    }

    // ---------------------------------------------------------------- allocation

    fn blocks_in_group(&self, g: usize) -> u32 {
        let start = self.first_data_block + g as u32 * self.blocks_per_group;
        (self.blocks_count - start).min(self.blocks_per_group)
    }

    fn alloc_block(&mut self, goal_group: usize) -> Result<u32> {
        let n = self.groups.len();
        for g in (goal_group..n).chain(0..goal_group) {
            if self.groups[g].free_blocks == 0 {
                continue;
            }
            let bitmap_block = self.groups[g].block_bitmap;
            let mut bitmap = self.read_block(bitmap_block)?;
            let limit = self.blocks_in_group(g) as usize;
            if let Some(i) = (0..limit).find(|&i| bitmap[i / 8] & (1 << (i % 8)) == 0) {
                bitmap[i / 8] |= 1 << (i % 8);
                self.write_block(bitmap_block, &bitmap)?;
                self.groups[g].free_blocks -= 1;
                self.free_blocks -= 1;
                self.dirty = true;
                let block = self.first_data_block + g as u32 * self.blocks_per_group + i as u32;
                self.write_block(block, &vec![0u8; self.block_size as usize])?;
                return Ok(block);
            }
            return Err(Error::Corrupt("group free count does not match its bitmap"));
        }
        Err(Error::NoSpace)
    }

    fn free_block(&mut self, block: u32) -> Result<()> {
        let rel = block
            .checked_sub(self.first_data_block)
            .ok_or(Error::Corrupt("freeing a metadata block"))?;
        let (g, i) = (
            (rel / self.blocks_per_group) as usize,
            (rel % self.blocks_per_group) as usize,
        );
        let bitmap_block = self
            .groups
            .get(g)
            .ok_or(Error::Corrupt("block group"))?
            .block_bitmap;
        let mut bitmap = self.read_block(bitmap_block)?;
        if bitmap[i / 8] & (1 << (i % 8)) == 0 {
            return Err(Error::Corrupt("double free of a block"));
        }
        bitmap[i / 8] &= !(1 << (i % 8));
        self.write_block(bitmap_block, &bitmap)?;
        self.groups[g].free_blocks += 1;
        self.free_blocks += 1;
        self.dirty = true;
        Ok(())
    }

    fn alloc_inode(&mut self, goal_group: usize, dir: bool) -> Result<u32> {
        let n = self.groups.len();
        for g in (goal_group..n).chain(0..goal_group) {
            if self.groups[g].free_inodes == 0 {
                continue;
            }
            let bitmap_block = self.groups[g].inode_bitmap;
            let mut bitmap = self.read_block(bitmap_block)?;
            let first = if g == 0 {
                self.first_ino as usize - 1
            } else {
                0
            };
            if let Some(i) = (first..self.inodes_per_group as usize)
                .find(|&i| bitmap[i / 8] & (1 << (i % 8)) == 0)
            {
                bitmap[i / 8] |= 1 << (i % 8);
                self.write_block(bitmap_block, &bitmap)?;
                self.groups[g].free_inodes -= 1;
                if dir {
                    self.groups[g].used_dirs += 1;
                }
                self.free_inodes -= 1;
                self.dirty = true;
                return Ok(g as u32 * self.inodes_per_group + i as u32 + 1);
            }
        }
        Err(Error::NoSpace)
    }

    fn free_inode(&mut self, ino: u32, dir: bool) -> Result<()> {
        let (g, i) = (
            ((ino - 1) / self.inodes_per_group) as usize,
            ((ino - 1) % self.inodes_per_group) as usize,
        );
        let bitmap_block = self.groups[g].inode_bitmap;
        let mut bitmap = self.read_block(bitmap_block)?;
        bitmap[i / 8] &= !(1 << (i % 8));
        self.write_block(bitmap_block, &bitmap)?;
        self.groups[g].free_inodes += 1;
        if dir {
            self.groups[g].used_dirs -= 1;
        }
        self.free_inodes += 1;
        self.dirty = true;
        Ok(())
    }

    // ---------------------------------------------------------------- inodes

    fn inode_location(&self, ino: u32) -> Result<u64> {
        if ino == 0 || ino > self.inodes_count {
            return Err(Error::Corrupt("inode number out of range"));
        }
        let g = ((ino - 1) / self.inodes_per_group) as usize;
        let index = ((ino - 1) % self.inodes_per_group) as u64;
        Ok(self.block_offset(self.groups[g].inode_table) + index * self.inode_size as u64)
    }

    pub fn read_inode(&self, ino: u32) -> Result<Inode> {
        let mut buf = [0u8; 128];
        self.disk.read_at(self.inode_location(ino)?, &mut buf)?;
        Ok(Inode::decode(&buf))
    }

    pub fn write_inode(&self, ino: u32, inode: &Inode) -> Result<()> {
        let at = self.inode_location(ino)?;
        let mut buf = [0u8; 128];
        self.disk.read_at(at, &mut buf)?;
        inode.encode(&mut buf);
        self.disk.write_at(at, &buf)
    }

    fn group_of(&self, ino: u32) -> usize {
        ((ino - 1) / self.inodes_per_group) as usize
    }

    // ---------------------------------------------------------------- block map

    fn ptrs_per_block(&self) -> u64 {
        self.block_size as u64 / 4
    }

    /// Physical block holding file block `index`; allocates when `alloc`.
    fn bmap(
        &mut self,
        inode: &mut Inode,
        group: usize,
        index: u64,
        alloc: bool,
    ) -> Result<Option<u32>> {
        let p = self.ptrs_per_block();
        let sectors_per_block = self.block_size / 512;
        if index < 12 {
            let slot = &mut inode.block[index as usize];
            if *slot == 0 && alloc {
                *slot = self.alloc_block(group)?;
                inode.sectors += sectors_per_block;
            }
            return Ok((*slot != 0).then_some(*slot));
        }
        let (depth, mut rel) = {
            let i = index - 12;
            if i < p {
                (1, i)
            } else if i - p < p * p {
                (2, i - p)
            } else if i - p - p * p < p * p * p {
                (3, i - p - p * p)
            } else {
                return Err(Error::FileTooLarge);
            }
        };
        let root = 11 + depth;
        if inode.block[root] == 0 {
            if !alloc {
                return Ok(None);
            }
            inode.block[root] = self.alloc_block(group)?;
            inode.sectors += sectors_per_block;
        }
        let mut table = inode.block[root];
        for level in (0..depth).rev() {
            let span = p.pow(level as u32);
            let slot = (rel / span) as usize;
            rel %= span;
            let mut entries = self.read_block(table)?;
            let mut next = get32(&entries, slot * 4);
            if next == 0 {
                if !alloc {
                    return Ok(None);
                }
                next = self.alloc_block(group)?;
                inode.sectors += sectors_per_block;
                put32(&mut entries, slot * 4, next);
                self.write_block(table, &entries)?;
            }
            table = next;
        }
        Ok(Some(table))
    }

    /// Frees the subtree at `ptr` (depth 0 = data block) for file blocks >= `start`.
    /// Returns true if `ptr` itself was freed.
    fn free_tree(
        &mut self,
        inode: &mut Inode,
        ptr: u32,
        depth: u32,
        base: u64,
        start: u64,
    ) -> Result<bool> {
        let p = self.ptrs_per_block();
        let covered = p.pow(depth);
        if base + covered <= start {
            return Ok(false);
        }
        let sectors_per_block = self.block_size / 512;
        if depth > 0 {
            let mut entries = self.read_block(ptr)?;
            let mut changed = false;
            let child_span = p.pow(depth - 1);
            for i in 0..p as usize {
                let child = get32(&entries, i * 4);
                if child != 0
                    && self.free_tree(
                        inode,
                        child,
                        depth - 1,
                        base + i as u64 * child_span,
                        start,
                    )?
                {
                    put32(&mut entries, i * 4, 0);
                    changed = true;
                }
            }
            if base < start {
                if changed {
                    self.write_block(ptr, &entries)?;
                }
                return Ok(false);
            }
        }
        self.free_block(ptr)?;
        inode.sectors -= sectors_per_block;
        Ok(true)
    }

    /// Releases all blocks holding file data at or after block `start`.
    fn free_blocks_from(&mut self, inode: &mut Inode, start: u64) -> Result<()> {
        let sectors_per_block = self.block_size / 512;
        for i in (start.min(12) as usize)..12 {
            if inode.block[i] != 0 {
                self.free_block(inode.block[i])?;
                inode.block[i] = 0;
                inode.sectors -= sectors_per_block;
            }
        }
        let p = self.ptrs_per_block();
        let bases = [12, 12 + p, 12 + p + p * p];
        for depth in 1..=3u32 {
            let ptr = inode.block[11 + depth as usize];
            if ptr != 0 && self.free_tree(inode, ptr, depth, bases[depth as usize - 1], start)? {
                inode.block[11 + depth as usize] = 0;
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------- file data

    pub fn read(&mut self, ino: u32, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let mut inode = self.read_inode(ino)?;
        if offset >= inode.size {
            return Ok(0);
        }
        let len = buf.len().min((inode.size - offset) as usize);
        let bs = self.block_size as u64;
        let mut done = 0;
        while done < len {
            let pos = offset + done as u64;
            let within = (pos % bs) as usize;
            let n = (bs as usize - within).min(len - done);
            match self.bmap(&mut inode, 0, pos / bs, false)? {
                Some(b) => self.disk.read_at(
                    self.block_offset(b) + within as u64,
                    &mut buf[done..done + n],
                )?,
                None => buf[done..done + n].fill(0), // hole
            }
            done += n;
        }
        Ok(len)
    }

    pub fn write(&mut self, ino: u32, offset: u64, data: &[u8]) -> Result<usize> {
        let mut inode = self.read_inode(ino)?;
        if inode.is_dir() {
            return Err(Error::IsDir);
        }
        let group = self.group_of(ino);
        let bs = self.block_size as u64;
        let mut done = 0;
        let result = loop {
            if done == data.len() {
                break Ok(());
            }
            let pos = offset + done as u64;
            let within = (pos % bs) as usize;
            let n = (bs as usize - within).min(data.len() - done);
            match self.bmap(&mut inode, group, pos / bs, true) {
                Ok(Some(b)) => {
                    if let Err(e) = self
                        .disk
                        .write_at(self.block_offset(b) + within as u64, &data[done..done + n])
                    {
                        break Err(e);
                    }
                }
                Ok(None) => break Err(Error::Corrupt("bmap")),
                Err(e) => break Err(e),
            }
            done += n;
        };
        inode.size = inode.size.max(offset + done as u64);
        let now = self.now();
        inode.mtime = now;
        inode.ctime = now;
        self.write_inode(ino, &inode)?;
        match result {
            Ok(()) => Ok(done),
            Err(e) if done == 0 => Err(e),
            Err(_) => Ok(done),
        }
    }

    pub fn truncate(&mut self, ino: u32, size: u64) -> Result<()> {
        let mut inode = self.read_inode(ino)?;
        if inode.is_dir() {
            return Err(Error::IsDir);
        }
        let bs = self.block_size as u64;
        if size < inode.size {
            self.free_blocks_from(&mut inode, size.div_ceil(bs))?;
            // Zero the tail of the last block so growing again reads zeros.
            if size % bs != 0 {
                if let Some(b) = self.bmap(&mut inode, 0, size / bs, false)? {
                    let within = size % bs;
                    self.disk.write_at(
                        self.block_offset(b) + within,
                        &vec![0u8; (bs - within) as usize],
                    )?;
                }
            }
        }
        inode.size = size;
        let now = self.now();
        inode.mtime = now;
        inode.ctime = now;
        self.write_inode(ino, &inode)
    }

    // ---------------------------------------------------------------- directories

    /// Calls `f(block_index, block_data)` for each block of a directory.
    fn dir_blocks(&mut self, ino: u32) -> Result<Vec<(u32, Vec<u8>)>> {
        let mut inode = self.read_inode(ino)?;
        if !inode.is_dir() {
            return Err(Error::NotDir);
        }
        let count = inode.size.div_ceil(self.block_size as u64);
        let mut out = Vec::new();
        for i in 0..count {
            if let Some(b) = self.bmap(&mut inode, 0, i, false)? {
                out.push((b, self.read_block(b)?));
            }
        }
        Ok(out)
    }

    fn parse_entries(&self, block: &[u8]) -> Result<Vec<(usize, u32, usize, String, u8)>> {
        let mut out = Vec::new();
        let mut pos = 0;
        while pos + 8 <= block.len() {
            let ino = get32(block, pos);
            let rec_len = get16(block, pos + 4) as usize;
            let name_len = block[pos + 6] as usize;
            if rec_len < 8 || pos + rec_len > block.len() || name_len + 8 > rec_len {
                return Err(Error::Corrupt("directory entry"));
            }
            let name = String::from_utf8_lossy(&block[pos + 8..pos + 8 + name_len]).into_owned();
            out.push((pos, ino, rec_len, name, block[pos + 7]));
            pos += rec_len;
        }
        Ok(out)
    }

    /// Entries of a directory, without `.` and `..`.
    pub fn read_dir(&mut self, ino: u32) -> Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for (_, data) in self.dir_blocks(ino)? {
            for (_, e_ino, _, name, ft) in self.parse_entries(&data)? {
                if e_ino != 0 && name != "." && name != ".." {
                    entries.push(DirEntry {
                        name,
                        ino: e_ino,
                        file_type: ft,
                    });
                }
            }
        }
        Ok(entries)
    }

    pub fn lookup(&mut self, dir: u32, name: &str) -> Result<u32> {
        for (_, data) in self.dir_blocks(dir)? {
            for (_, ino, _, n, _) in self.parse_entries(&data)? {
                if ino != 0 && n == name {
                    return Ok(ino);
                }
            }
        }
        Err(Error::NotFound)
    }

    fn write_entry(block: &mut [u8], pos: usize, ino: u32, rec_len: usize, name: &str, ft: u8) {
        put32(block, pos, ino);
        put16(block, pos + 4, rec_len as u16);
        block[pos + 6] = name.len() as u8;
        block[pos + 7] = ft;
        block[pos + 8..pos + 8 + name.len()].copy_from_slice(name.as_bytes());
    }

    fn add_entry(&mut self, dir: u32, name: &str, ino: u32, ft: u8) -> Result<()> {
        let need = rec_len_for(name.len());
        for (b, mut data) in self.dir_blocks(dir)? {
            for (pos, e_ino, rec_len, e_name, _) in self.parse_entries(&data)? {
                if e_ino == 0 && rec_len >= need {
                    Self::write_entry(&mut data, pos, ino, rec_len, name, ft);
                    return self.write_block(b, &data);
                }
                let used = rec_len_for(e_name.len());
                if e_ino != 0 && rec_len >= used + need {
                    put16(&mut data, pos + 4, used as u16);
                    Self::write_entry(&mut data, pos + used, ino, rec_len - used, name, ft);
                    return self.write_block(b, &data);
                }
            }
        }
        // No room: append a block to the directory.
        let mut inode = self.read_inode(dir)?;
        let group = self.group_of(dir);
        let index = inode.size / self.block_size as u64;
        let b = self
            .bmap(&mut inode, group, index, true)?
            .ok_or(Error::NoSpace)?;
        let mut data = vec![0u8; self.block_size as usize];
        Self::write_entry(&mut data, 0, ino, self.block_size as usize, name, ft);
        self.write_block(b, &data)?;
        inode.size += self.block_size as u64;
        self.write_inode(dir, &inode)
    }

    fn remove_entry(&mut self, dir: u32, name: &str) -> Result<u32> {
        for (b, mut data) in self.dir_blocks(dir)? {
            let entries = self.parse_entries(&data)?;
            for (i, &(pos, ino, rec_len, ref n, _)) in entries.iter().enumerate() {
                if ino == 0 || n != name {
                    continue;
                }
                if i == 0 {
                    put32(&mut data, pos, 0);
                } else {
                    let (prev_pos, _, prev_len, _, _) = entries[i - 1];
                    put16(&mut data, prev_pos + 4, (prev_len + rec_len) as u16);
                }
                self.write_block(b, &data)?;
                return Ok(ino);
            }
        }
        Err(Error::NotFound)
    }

    /// Creates a file or directory (`mode` includes the type bits).
    pub fn create(&mut self, parent: u32, name: &str, mode: u16) -> Result<u32> {
        check_name(name)?;
        match self.lookup(parent, name) {
            Ok(_) => return Err(Error::Exists),
            Err(Error::NotFound) => {}
            Err(e) => return Err(e),
        }
        let dir = mode & S_IFMT == S_IFDIR;
        let group = self.group_of(parent);
        let ino = self.alloc_inode(group, dir)?;
        let now = self.now();
        let mut inode = Inode {
            mode,
            links: if dir { 2 } else { 1 },
            atime: now,
            ctime: now,
            mtime: now,
            ..Inode::default()
        };
        if dir {
            let b = self.alloc_block(group)?;
            let mut data = vec![0u8; self.block_size as usize];
            Self::write_entry(&mut data, 0, ino, 12, ".", FT_DIR);
            Self::write_entry(
                &mut data,
                12,
                parent,
                self.block_size as usize - 12,
                "..",
                FT_DIR,
            );
            self.write_block(b, &data)?;
            inode.block[0] = b;
            inode.size = self.block_size as u64;
            inode.sectors = self.block_size / 512;
        }
        self.write_inode(ino, &inode)?;
        self.add_entry(parent, name, ino, file_type_of(mode))?;
        let mut p = self.read_inode(parent)?;
        if dir {
            p.links += 1;
        }
        p.mtime = now;
        p.ctime = now;
        self.write_inode(parent, &p)?;
        Ok(ino)
    }

    fn release_inode(&mut self, ino: u32, mut inode: Inode) -> Result<()> {
        if inode.is_fast_symlink() {
            inode.block = [0; 15]; // the target text, not block numbers
        }
        self.free_blocks_from(&mut inode, 0)?;
        inode.links = 0;
        inode.size = 0;
        inode.dtime = self.now();
        self.write_inode(ino, &inode)?;
        self.free_inode(ino, inode.is_dir())
    }

    /// Creates the symbolic link `parent/name` pointing to `target`.
    pub fn symlink(&mut self, parent: u32, name: &str, target: &str) -> Result<u32> {
        if target.is_empty() {
            return Err(Error::Invalid);
        }
        if target.len() >= self.block_size as usize {
            return Err(Error::NameTooLong);
        }
        let ino = self.create(parent, name, S_IFLNK | 0o777)?;
        if target.len() < FAST_SYMLINK_MAX {
            let mut inode = self.read_inode(ino)?;
            let mut bytes = [0u8; FAST_SYMLINK_MAX];
            bytes[..target.len()].copy_from_slice(target.as_bytes());
            for (i, slot) in inode.block.iter_mut().enumerate() {
                *slot = get32(&bytes, i * 4);
            }
            inode.size = target.len() as u64;
            self.write_inode(ino, &inode)?;
        } else if let Err(e) = self.write(ino, 0, target.as_bytes()) {
            let _ = self.unlink(parent, name);
            return Err(e);
        }
        Ok(ino)
    }

    /// The target of a symbolic link.
    pub fn read_link(&mut self, ino: u32) -> Result<String> {
        let inode = self.read_inode(ino)?;
        if !inode.is_symlink() {
            return Err(Error::Invalid);
        }
        let len = inode.size as usize;
        let bytes = if inode.is_fast_symlink() {
            if len >= FAST_SYMLINK_MAX {
                return Err(Error::Corrupt("fast symlink too long"));
            }
            let mut b = [0u8; FAST_SYMLINK_MAX];
            for (i, v) in inode.block.iter().enumerate() {
                put32(&mut b, i * 4, *v);
            }
            b[..len].to_vec()
        } else {
            if len >= self.block_size as usize {
                return Err(Error::Corrupt("symlink too long"));
            }
            let mut b = vec![0u8; len];
            self.read(ino, 0, &mut b)?;
            b
        };
        String::from_utf8(bytes).map_err(|_| Error::Corrupt("symlink target is not UTF-8"))
    }

    /// Removes `name` from `parent` (directories must be empty).
    pub fn unlink(&mut self, parent: u32, name: &str) -> Result<()> {
        let ino = self.lookup(parent, name)?;
        let mut inode = self.read_inode(ino)?;
        if inode.is_dir() && !self.read_dir(ino)?.is_empty() {
            return Err(Error::NotEmpty);
        }
        self.remove_entry(parent, name)?;
        let now = self.now();
        let mut p = self.read_inode(parent)?;
        if inode.is_dir() {
            p.links -= 1;
        }
        p.mtime = now;
        p.ctime = now;
        self.write_inode(parent, &p)?;
        if inode.is_dir() || inode.links <= 1 {
            self.release_inode(ino, inode)
        } else {
            inode.links -= 1;
            inode.ctime = now;
            self.write_inode(ino, &inode)
        }
    }

    /// Moves `old_parent/old_name` to `new_parent/new_name`.
    pub fn rename(
        &mut self,
        old_parent: u32,
        old_name: &str,
        new_parent: u32,
        new_name: &str,
    ) -> Result<()> {
        check_name(new_name)?;
        let ino = self.lookup(old_parent, old_name)?;
        let inode = self.read_inode(ino)?;
        if old_parent == new_parent && old_name == new_name {
            return Ok(());
        }
        match self.lookup(new_parent, new_name) {
            Ok(existing) => {
                if self.read_inode(existing)?.is_dir() {
                    return Err(Error::IsDir);
                }
                self.unlink(new_parent, new_name)?;
            }
            Err(Error::NotFound) => {}
            Err(e) => return Err(e),
        }
        self.add_entry(new_parent, new_name, ino, file_type_of(inode.mode))?;
        self.remove_entry(old_parent, old_name)?;
        if inode.is_dir() && old_parent != new_parent {
            // Point ".." at the new parent and move the link count.
            let mut inode = inode;
            let b = self
                .bmap(&mut inode, 0, 0, false)?
                .ok_or(Error::Corrupt("empty directory"))?;
            let mut data = self.read_block(b)?;
            let entries = self.parse_entries(&data)?;
            let pos = entries
                .iter()
                .find(|e| e.3 == "..")
                .ok_or(Error::Corrupt("no .. entry"))?
                .0;
            put32(&mut data, pos, new_parent);
            self.write_block(b, &data)?;
            let mut op = self.read_inode(old_parent)?;
            op.links -= 1;
            self.write_inode(old_parent, &op)?;
            let mut np = self.read_inode(new_parent)?;
            np.links += 1;
            self.write_inode(new_parent, &np)?;
        }
        Ok(())
    }

    /// Writes the superblock and group descriptors back.
    pub fn flush(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        for (g, grp) in self.groups.iter().enumerate() {
            let d = &mut self.gdt[g * 32..g * 32 + 32];
            put16(d, 12, grp.free_blocks);
            put16(d, 14, grp.free_inodes);
            put16(d, 16, grp.used_dirs);
        }
        let gdt_offset = (self.first_data_block as u64 + 1) * self.block_size as u64;
        self.disk.write_at(gdt_offset, &self.gdt)?;
        let now = self.now();
        put32(&mut self.superblock, 12, self.free_blocks);
        put32(&mut self.superblock, 16, self.free_inodes);
        put32(&mut self.superblock, 48, now);
        self.disk.write_at(1024, &self.superblock)?;
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
