extern crate std;

use super::*;
use core::cell::RefCell;
use std::vec::Vec;

struct MemDisk(RefCell<Vec<u8>>);

impl Disk for MemDisk {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let d = self.0.borrow();
        let o = offset as usize;
        buf.copy_from_slice(d.get(o..o + buf.len()).ok_or(Error::Io)?);
        Ok(())
    }
    fn write_at(&self, offset: u64, buf: &[u8]) -> Result<()> {
        let mut d = self.0.borrow_mut();
        let o = offset as usize;
        d.get_mut(o..o + buf.len()).ok_or(Error::Io)?.copy_from_slice(buf);
        Ok(())
    }
}

fn clock() -> u32 {
    1_700_000_000
}

fn fresh(size: usize) -> Ext2<MemDisk> {
    let disk = MemDisk(RefCell::new(std::vec![0u8; size]));
    format(&disk, size as u64, "test", clock()).unwrap();
    Ext2::open(disk, clock).unwrap()
}

/// Recounts bitmaps and compares them with the cached counters.
fn check_consistency(fs: &Ext2<MemDisk>) {
    let mut free_blocks = 0;
    let mut free_inodes = 0;
    for (g, grp) in fs.groups.iter().enumerate() {
        let bitmap = fs.read_block(grp.block_bitmap).unwrap();
        let n = fs.blocks_in_group(g) as usize;
        let free = (0..n).filter(|&i| bitmap[i / 8] & (1 << (i % 8)) == 0).count() as u32;
        assert_eq!(free, grp.free_blocks as u32, "group {g} free blocks");
        free_blocks += free;
        let ibitmap = fs.read_block(grp.inode_bitmap).unwrap();
        let ifree = (0..fs.inodes_per_group as usize).filter(|&i| ibitmap[i / 8] & (1 << (i % 8)) == 0).count() as u32;
        assert_eq!(ifree, grp.free_inodes as u32, "group {g} free inodes");
        free_inodes += ifree;
    }
    assert_eq!(free_blocks, fs.free_blocks);
    assert_eq!(free_inodes, fs.free_inodes);
}

#[test]
fn format_and_mount() {
    let mut fs = fresh(4 << 20);
    assert_eq!(fs.volume_name(), "test");
    let root = fs.read_inode(ROOT_INO).unwrap();
    assert!(root.is_dir());
    assert_eq!(root.links, 3);
    let names: Vec<String> = fs.read_dir(ROOT_INO).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["lost+found"]);
    check_consistency(&fs);
}

#[test]
fn files_roundtrip() {
    let mut fs = fresh(4 << 20);
    let before = fs.statfs();
    let f = fs.create(ROOT_INO, "hello.txt", S_IFREG | 0o644).unwrap();
    assert_eq!(fs.write(f, 0, b"hello, world").unwrap(), 12);
    let mut buf = [0u8; 64];
    assert_eq!(fs.read(f, 0, &mut buf).unwrap(), 12);
    assert_eq!(&buf[..12], b"hello, world");
    assert_eq!(fs.read(f, 7, &mut buf).unwrap(), 5);
    assert_eq!(&buf[..5], b"world");
    assert_eq!(fs.lookup(ROOT_INO, "hello.txt").unwrap(), f);
    assert_eq!(fs.create(ROOT_INO, "hello.txt", S_IFREG | 0o644), Err(Error::Exists));
    fs.unlink(ROOT_INO, "hello.txt").unwrap();
    assert_eq!(fs.lookup(ROOT_INO, "hello.txt"), Err(Error::NotFound));
    let after = fs.statfs();
    assert_eq!((before.free_blocks, before.free_inodes), (after.free_blocks, after.free_inodes));
    check_consistency(&fs);
}

#[test]
fn large_file_with_indirect_blocks() {
    let mut fs = fresh(8 << 20);
    let before = fs.statfs().free_blocks;
    let f = fs.create(ROOT_INO, "big", S_IFREG | 0o644).unwrap();
    // 12 direct + 256 single-indirect blocks = 268 KiB; go well past that.
    let data: Vec<u8> = (0..700_000u32).map(|i| (i * 7 % 251) as u8).collect();
    assert_eq!(fs.write(f, 0, &data).unwrap(), data.len());
    let mut back = std::vec![0u8; data.len()];
    assert_eq!(fs.read(f, 0, &mut back).unwrap(), data.len());
    assert!(back == data);
    let inode = fs.read_inode(f).unwrap();
    // data blocks + 1 single indirect + 1 double indirect + 2 second-level tables
    let data_blocks = data.len().div_ceil(1024) as u32;
    assert_eq!(inode.sectors, (data_blocks + 4) * 2);
    check_consistency(&fs);

    fs.truncate(f, 5000).unwrap();
    let inode = fs.read_inode(f).unwrap();
    assert_eq!(inode.sectors, 5 * 2);
    assert_eq!(inode.block[12], 0);
    assert_eq!(inode.block[13], 0);
    assert_eq!(fs.statfs().free_blocks, before - 5);
    let mut tail = [0u8; 100];
    fs.truncate(f, 6000).unwrap();
    assert_eq!(fs.read(f, 5000, &mut tail).unwrap(), 100);
    assert!(tail.iter().all(|&b| b == 0), "old data visible after truncate+extend");
    check_consistency(&fs);
}

#[test]
fn sparse_files() {
    let mut fs = fresh(4 << 20);
    let f = fs.create(ROOT_INO, "sparse", S_IFREG | 0o644).unwrap();
    fs.write(f, 100_000, b"end").unwrap();
    let inode = fs.read_inode(f).unwrap();
    assert_eq!(inode.size, 100_003);
    let mut buf = [1u8; 16];
    fs.read(f, 50_000, &mut buf).unwrap();
    assert!(buf.iter().all(|&b| b == 0));
    check_consistency(&fs);
}

#[test]
fn directories() {
    let mut fs = fresh(4 << 20);
    let d = fs.create(ROOT_INO, "dir", S_IFDIR | 0o755).unwrap();
    assert_eq!(fs.read_inode(ROOT_INO).unwrap().links, 4);
    // Enough entries to need several directory blocks.
    for i in 0..200 {
        fs.create(d, &std::format!("file-with-a-long-name-{i:04}"), S_IFREG | 0o644).unwrap();
    }
    assert!(fs.read_inode(d).unwrap().size > 1024);
    assert_eq!(fs.read_dir(d).unwrap().len(), 200);
    assert_eq!(fs.unlink(ROOT_INO, "dir"), Err(Error::NotEmpty));
    for i in (0..200).rev() {
        fs.unlink(d, &std::format!("file-with-a-long-name-{i:04}")).unwrap();
    }
    assert!(fs.read_dir(d).unwrap().is_empty());
    // Freed slots are reused.
    fs.create(d, "again", S_IFREG | 0o644).unwrap();
    fs.unlink(d, "again").unwrap();
    fs.unlink(ROOT_INO, "dir").unwrap();
    assert_eq!(fs.read_inode(ROOT_INO).unwrap().links, 3);
    check_consistency(&fs);
}

#[test]
fn rename_files_and_directories() {
    let mut fs = fresh(4 << 20);
    let a = fs.create(ROOT_INO, "a", S_IFDIR | 0o755).unwrap();
    let b = fs.create(ROOT_INO, "b", S_IFDIR | 0o755).unwrap();
    let f = fs.create(a, "f", S_IFREG | 0o644).unwrap();
    fs.write(f, 0, b"payload").unwrap();
    fs.rename(a, "f", b, "g").unwrap();
    assert_eq!(fs.lookup(b, "g").unwrap(), f);
    assert_eq!(fs.lookup(a, "f"), Err(Error::NotFound));

    let sub = fs.create(a, "sub", S_IFDIR | 0o755).unwrap();
    fs.rename(a, "sub", b, "sub").unwrap();
    assert_eq!(fs.read_inode(a).unwrap().links, 2);
    assert_eq!(fs.read_inode(b).unwrap().links, 3);
    assert_eq!(fs.lookup(sub, ".."), Ok(b));

    // Replacing an existing file frees it.
    let h = fs.create(b, "h", S_IFREG | 0o644).unwrap();
    fs.write(h, 0, &[1u8; 4096]).unwrap();
    fs.rename(b, "g", b, "h").unwrap();
    assert_eq!(fs.lookup(b, "h").unwrap(), f);
    check_consistency(&fs);
}

#[test]
fn multiple_groups_and_persistence() {
    let size = 40 << 20;
    let disk = MemDisk(RefCell::new(std::vec![0u8; size]));
    format(&disk, size as u64, "multi", clock()).unwrap();
    let mut fs = Ext2::open(disk, clock).unwrap();
    assert!(fs.groups.len() >= 5);
    let blob: Vec<u8> = (0..3_000_000u32).map(|i| i as u8).collect();
    let f = fs.create(ROOT_INO, "blob", S_IFREG | 0o644).unwrap();
    fs.write(f, 0, &blob).unwrap();
    fs.flush().unwrap();
    check_consistency(&fs);

    // Remount from the same bytes.
    let disk = MemDisk(RefCell::new(fs.disk().0.borrow().clone()));
    let mut fs2 = Ext2::open(disk, clock).unwrap();
    assert_eq!(fs2.statfs(), fs.statfs());
    let f2 = fs2.lookup(ROOT_INO, "blob").unwrap();
    let mut back = std::vec![0u8; blob.len()];
    fs2.read(f2, 0, &mut back).unwrap();
    assert!(back == blob);
}

#[test]
fn out_of_space() {
    let mut fs = fresh(1 << 20);
    let f = fs.create(ROOT_INO, "fill", S_IFREG | 0o644).unwrap();
    let chunk = std::vec![0xAAu8; 64 * 1024];
    let mut total = 0;
    loop {
        match fs.write(f, total as u64, &chunk) {
            Ok(n) if n == chunk.len() => total += n,
            Ok(n) => {
                total += n;
                break;
            }
            Err(Error::NoSpace) => break,
            Err(e) => panic!("{e:?}"),
        }
    }
    assert_eq!(fs.statfs().free_blocks, 0);
    assert!(total > 800 * 1024);
    check_consistency(&fs);
    fs.unlink(ROOT_INO, "fill").unwrap();
    check_consistency(&fs);
}

#[test]
fn rejects_bad_names_and_images() {
    let mut fs = fresh(1 << 20);
    assert_eq!(fs.create(ROOT_INO, "", S_IFREG), Err(Error::Invalid));
    assert_eq!(fs.create(ROOT_INO, "a/b", S_IFREG), Err(Error::Invalid));
    assert_eq!(fs.create(ROOT_INO, &"x".repeat(256), S_IFREG), Err(Error::NameTooLong));
    let disk = MemDisk(RefCell::new(std::vec![0u8; 1 << 20]));
    assert!(matches!(Ext2::open(disk, clock), Err(Error::Corrupt(_))));
}

#[test]
fn sparse_super_backups() {
    assert!(format::has_superblock(0) && format::has_superblock(1));
    assert!(format::has_superblock(3) && format::has_superblock(9) && format::has_superblock(25) && format::has_superblock(49));
    assert!(!format::has_superblock(2) && !format::has_superblock(4) && !format::has_superblock(6));
}
