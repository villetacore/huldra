//! File system ABI: open flags, file modes, `struct stat`, directory entries.

pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_ACCMODE: u32 = 3;
pub const O_CREAT: u32 = 0o100;
pub const O_EXCL: u32 = 0o200;
pub const O_NOCTTY: u32 = 0o400;
pub const O_TRUNC: u32 = 0o1000;
pub const O_APPEND: u32 = 0o2000;
pub const O_NONBLOCK: u32 = 0o4000;
pub const O_DIRECTORY: u32 = 0o200000;
pub const O_CLOEXEC: u32 = 0o2000000;

pub const AT_FDCWD: i32 = -100;
pub const AT_REMOVEDIR: u32 = 0x200;

pub const SEEK_SET: u32 = 0;
pub const SEEK_CUR: u32 = 1;
pub const SEEK_END: u32 = 2;

pub const F_DUPFD: u32 = 0;
pub const F_GETFD: u32 = 1;
pub const F_SETFD: u32 = 2;
pub const F_GETFL: u32 = 3;
pub const F_SETFL: u32 = 4;
pub const F_DUPFD_CLOEXEC: u32 = 1030;
pub const FD_CLOEXEC: u32 = 1;

pub const S_IFMT: u32 = 0o170000;
pub const S_IFSOCK: u32 = 0o140000;
pub const S_IFLNK: u32 = 0o120000;
pub const S_IFREG: u32 = 0o100000;
pub const S_IFBLK: u32 = 0o060000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFCHR: u32 = 0o020000;
pub const S_IFIFO: u32 = 0o010000;

pub const DT_UNKNOWN: u8 = 0;
pub const DT_FIFO: u8 = 1;
pub const DT_CHR: u8 = 2;
pub const DT_DIR: u8 = 4;
pub const DT_BLK: u8 = 6;
pub const DT_REG: u8 = 8;
pub const DT_LNK: u8 = 10;

/// `struct stat` (x86_64).
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_nlink: u64,
    pub st_mode: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub _pad0: i32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    pub st_atime: i64,
    pub st_atime_nsec: i64,
    pub st_mtime: i64,
    pub st_mtime_nsec: i64,
    pub st_ctime: i64,
    pub st_ctime_nsec: i64,
    pub _unused: [i64; 3],
}

/// Size of the fixed part of `struct linux_dirent64`
/// (`d_ino: u64, d_off: i64, d_reclen: u16, d_type: u8`); the name follows.
pub const DIRENT64_HEADER_SIZE: usize = 19;

/// Record length of a dirent64 with a name of `name_len` bytes.
pub const fn dirent64_reclen(name_len: usize) -> usize {
    (DIRENT64_HEADER_SIZE + name_len + 1 + 7) & !7
}

/// Encodes one dirent64 record into `out`; returns the record length.
pub fn encode_dirent64(
    out: &mut [u8],
    ino: u64,
    off: i64,
    d_type: u8,
    name: &[u8],
) -> Option<usize> {
    let reclen = dirent64_reclen(name.len());
    if out.len() < reclen {
        return None;
    }
    out[..reclen].fill(0);
    out[0..8].copy_from_slice(&ino.to_le_bytes());
    out[8..16].copy_from_slice(&off.to_le_bytes());
    out[16..18].copy_from_slice(&(reclen as u16).to_le_bytes());
    out[18] = d_type;
    out[19..19 + name.len()].copy_from_slice(name);
    Some(reclen)
}

/// A decoded dirent64 record.
pub struct Dirent<'a> {
    pub ino: u64,
    pub d_type: u8,
    pub name: &'a [u8],
}

/// Iterates over the records in a `getdents64` buffer.
pub fn decode_dirents(buf: &[u8]) -> impl Iterator<Item = Dirent<'_>> {
    let mut pos = 0;
    core::iter::from_fn(move || {
        if pos + DIRENT64_HEADER_SIZE > buf.len() {
            return None;
        }
        let rec = &buf[pos..];
        let reclen = u16::from_le_bytes([rec[16], rec[17]]) as usize;
        if reclen < DIRENT64_HEADER_SIZE || pos + reclen > buf.len() {
            return None;
        }
        let name_area = &rec[DIRENT64_HEADER_SIZE..reclen];
        let len = name_area
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_area.len());
        pos += reclen;
        Some(Dirent {
            ino: u64::from_le_bytes(rec[0..8].try_into().unwrap()),
            d_type: rec[18],
            name: &name_area[..len],
        })
    })
}

/// Mount flags.
pub const MS_RDONLY: u64 = 1;
