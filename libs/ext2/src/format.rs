//! `mkfs.ext2`: creates an empty revision-1 file system with 1 KiB blocks,
//! `sparse_super`, `large_file` and `filetype`, a root directory and
//! `lost+found`.

use crate::*;

const BLOCK_SIZE: u32 = 1024;
const INODE_SIZE: u32 = 128;
const FIRST_INO: u32 = 11;
const LOST_FOUND_INO: u32 = 11;

fn is_power_of(mut n: u32, base: u32) -> bool {
    while n > 1 && n % base == 0 {
        n /= base;
    }
    n == 1
}

/// Groups that carry a superblock backup under `sparse_super`.
pub(crate) fn has_superblock(group: u32) -> bool {
    group <= 1 || is_power_of(group, 3) || is_power_of(group, 5) || is_power_of(group, 7)
}

fn set_bit(bitmap: &mut [u8], i: usize) {
    bitmap[i / 8] |= 1 << (i % 8);
}

/// Formats `size` bytes of `disk`.
pub fn format<D: Disk>(disk: &D, size: u64, volume_name: &str, now: u32) -> Result<()> {
    let bs = BLOCK_SIZE as u64;
    let first_data_block = 1u32;
    let blocks_per_group = 8 * BLOCK_SIZE;
    let mut blocks_count = (size / bs).min(u32::MAX as u64) as u32;
    if blocks_count < 64 {
        return Err(Error::NoSpace);
    }
    let mut ngroups = (blocks_count - first_data_block).div_ceil(blocks_per_group);
    let total_inodes = (blocks_count / 4).max(64);
    let inodes_per_block = BLOCK_SIZE / INODE_SIZE;
    let inodes_per_group = total_inodes
        .div_ceil(ngroups)
        .next_multiple_of(inodes_per_block)
        .clamp(16, 8 * BLOCK_SIZE);
    let inode_table_blocks = inodes_per_group / inodes_per_block;
    let gdt_blocks = (ngroups * 32).div_ceil(BLOCK_SIZE);

    let overhead =
        |g: u32| (if has_superblock(g) { 1 + gdt_blocks } else { 0 }) + 2 + inode_table_blocks;
    // Drop a last group too small to hold its own metadata.
    let last_size = blocks_count - first_data_block - (ngroups - 1) * blocks_per_group;
    if ngroups > 1 && last_size < overhead(ngroups - 1) + 16 {
        ngroups -= 1;
        blocks_count = first_data_block + ngroups * blocks_per_group;
    }

    let group_start = |g: u32| first_data_block + g * blocks_per_group;
    let blocks_in_group = |g: u32| (blocks_count - group_start(g)).min(blocks_per_group);

    struct Layout {
        block_bitmap: u32,
        inode_bitmap: u32,
        inode_table: u32,
        used: u32,
    }
    let layouts: Vec<Layout> = (0..ngroups)
        .map(|g| {
            let meta = group_start(g) + if has_superblock(g) { 1 + gdt_blocks } else { 0 };
            Layout {
                block_bitmap: meta,
                inode_bitmap: meta + 1,
                inode_table: meta + 2,
                used: overhead(g),
            }
        })
        .collect();

    // Root and lost+found get the first data blocks of group 0.
    let root_block = layouts[0].inode_table + inode_table_blocks;
    let lost_found_block = root_block + 1;

    let zero = vec![0u8; BLOCK_SIZE as usize];
    let mut free_blocks_total = 0u32;
    let mut gdt = vec![0u8; (gdt_blocks * BLOCK_SIZE) as usize];

    for g in 0..ngroups {
        let l = &layouts[g as usize];
        let extra = if g == 0 { 2 } else { 0 };
        let used = l.used + extra;
        let in_group = blocks_in_group(g);

        let mut bitmap = vec![0u8; BLOCK_SIZE as usize];
        for i in 0..used as usize {
            set_bit(&mut bitmap, i);
        }
        for i in in_group as usize..(8 * BLOCK_SIZE) as usize {
            set_bit(&mut bitmap, i); // padding past the end of the group
        }
        disk.write_at(l.block_bitmap as u64 * bs, &bitmap)?;

        let mut ibitmap = vec![0u8; BLOCK_SIZE as usize];
        let used_inodes = if g == 0 { FIRST_INO } else { 0 };
        for i in 0..used_inodes as usize {
            set_bit(&mut ibitmap, i);
        }
        for i in inodes_per_group as usize..(8 * BLOCK_SIZE) as usize {
            set_bit(&mut ibitmap, i);
        }
        disk.write_at(l.inode_bitmap as u64 * bs, &ibitmap)?;

        for b in 0..inode_table_blocks {
            disk.write_at((l.inode_table + b) as u64 * bs, &zero)?;
        }

        let free = in_group - used;
        free_blocks_total += free;
        let d = &mut gdt[g as usize * 32..g as usize * 32 + 32];
        put32(d, 0, l.block_bitmap);
        put32(d, 4, l.inode_bitmap);
        put32(d, 8, l.inode_table);
        put16(d, 12, free as u16);
        put16(d, 14, (inodes_per_group - used_inodes) as u16);
        put16(d, 16, if g == 0 { 2 } else { 0 });
    }

    // Root directory and lost+found.
    let mut block = vec![0u8; BLOCK_SIZE as usize];
    let entry = |b: &mut [u8], pos: usize, ino: u32, rec: usize, name: &str| {
        put32(b, pos, ino);
        put16(b, pos + 4, rec as u16);
        b[pos + 6] = name.len() as u8;
        b[pos + 7] = FT_DIR;
        b[pos + 8..pos + 8 + name.len()].copy_from_slice(name.as_bytes());
    };
    entry(&mut block, 0, ROOT_INO, 12, ".");
    entry(&mut block, 12, ROOT_INO, 12, "..");
    entry(
        &mut block,
        24,
        LOST_FOUND_INO,
        BLOCK_SIZE as usize - 24,
        "lost+found",
    );
    disk.write_at(root_block as u64 * bs, &block)?;
    let mut block = vec![0u8; BLOCK_SIZE as usize];
    entry(&mut block, 0, LOST_FOUND_INO, 12, ".");
    entry(&mut block, 12, ROOT_INO, BLOCK_SIZE as usize - 12, "..");
    disk.write_at(lost_found_block as u64 * bs, &block)?;

    let write_inode = |ino: u32, inode: &Inode| -> Result<()> {
        let mut raw = [0u8; 128];
        inode.encode(&mut raw);
        let at = layouts[0].inode_table as u64 * bs + (ino as u64 - 1) * INODE_SIZE as u64;
        disk.write_at(at, &raw)
    };
    let mut root = Inode {
        mode: S_IFDIR | 0o755,
        links: 3,
        size: bs,
        sectors: (bs / 512) as u32,
        atime: now,
        ctime: now,
        mtime: now,
        ..Inode::default()
    };
    root.block[0] = root_block;
    write_inode(ROOT_INO, &root)?;
    let mut lf = Inode {
        mode: S_IFDIR | 0o700,
        links: 2,
        ..root
    };
    lf.block[0] = lost_found_block;
    write_inode(LOST_FOUND_INO, &lf)?;

    // Superblock (primary and backups) and group descriptor table copies.
    let inodes_count = inodes_per_group * ngroups;
    let mut sb = vec![0u8; 1024];
    put32(&mut sb, 0, inodes_count);
    put32(&mut sb, 4, blocks_count);
    put32(&mut sb, 8, 0); // reserved blocks
    put32(&mut sb, 12, free_blocks_total);
    put32(&mut sb, 16, inodes_count - FIRST_INO);
    put32(&mut sb, 20, first_data_block);
    put32(&mut sb, 24, 0); // log2(block size) - 10
    put32(&mut sb, 28, 0);
    put32(&mut sb, 32, blocks_per_group);
    put32(&mut sb, 36, blocks_per_group);
    put32(&mut sb, 40, inodes_per_group);
    put32(&mut sb, 44, 0); // mount time
    put32(&mut sb, 48, now); // write time
    put16(&mut sb, 52, 0); // mount count
    put16(&mut sb, 54, 0xFFFF); // max mount count: -1
    put16(&mut sb, 56, MAGIC);
    put16(&mut sb, 58, 1); // state: clean
    put16(&mut sb, 60, 1); // errors: continue
    put32(&mut sb, 64, now); // last check
    put32(&mut sb, 76, 1); // revision 1 (dynamic)
    put32(&mut sb, 84, FIRST_INO);
    put16(&mut sb, 88, INODE_SIZE as u16);
    put32(&mut sb, 96, INCOMPAT_FILETYPE);
    put32(&mut sb, 100, RO_COMPAT_SPARSE_SUPER | RO_COMPAT_LARGE_FILE);
    let mut uuid = [0u8; 16];
    let mut seed = now as u64 ^ 0x9E37_79B9_7F4A_7C15 ^ size;
    for b in uuid.iter_mut() {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *b = (seed >> 56) as u8;
    }
    uuid[6] = (uuid[6] & 0x0F) | 0x40; // version 4
    uuid[8] = (uuid[8] & 0x3F) | 0x80;
    sb[104..120].copy_from_slice(&uuid);
    let name = volume_name.as_bytes();
    sb[120..120 + name.len().min(16)].copy_from_slice(&name[..name.len().min(16)]);

    for g in 0..ngroups {
        if !has_superblock(g) {
            continue;
        }
        put16(&mut sb, 90, g as u16); // block group number of this copy
        let start = group_start(g) as u64;
        let at = if g == 0 { 1024 } else { start * bs };
        disk.write_at(at, &sb)?;
        disk.write_at((start + 1) * bs, &gdt)?;
    }
    Ok(())
}
