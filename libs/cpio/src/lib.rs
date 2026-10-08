//! The cpio "newc" archive format, as used by Linux initramfs.
//!
//! Each entry is a 110-byte ASCII header (magic `070701` and 13 eight-digit
//! hex fields), the NUL-terminated name padded to 4 bytes, then the data
//! padded to 4 bytes. The archive ends with an entry named `TRAILER!!!`.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

const MAGIC: &[u8; 6] = b"070701";
const HEADER_LEN: usize = 110;
const TRAILER: &str = "TRAILER!!!";

pub const S_IFMT: u32 = 0o170000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFREG: u32 = 0o100000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    BadMagic,
    BadNumber,
    Truncated,
    BadName,
}

#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    pub name: &'a str,
    pub mode: u32,
    pub data: &'a [u8],
}

impl Entry<'_> {
    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }

    pub fn is_file(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }

    pub fn permissions(&self) -> u32 {
        self.mode & 0o7777
    }
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn hex_field(header: &[u8], index: usize) -> Result<u32, Error> {
    let s = &header[6 + index * 8..6 + (index + 1) * 8];
    let s = core::str::from_utf8(s).map_err(|_| Error::BadNumber)?;
    u32::from_str_radix(s, 16).map_err(|_| Error::BadNumber)
}

/// Iterates over the entries of an archive (the trailer is not returned).
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    done: bool,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0, done: false }
    }

    fn next_entry(&mut self) -> Result<Option<Entry<'a>>, Error> {
        let header = self.data.get(self.pos..self.pos + HEADER_LEN).ok_or(Error::Truncated)?;
        if &header[..6] != MAGIC {
            return Err(Error::BadMagic);
        }
        let mode = hex_field(header, 1)?;
        let file_size = hex_field(header, 6)? as usize;
        let name_size = hex_field(header, 11)? as usize;
        if name_size == 0 {
            return Err(Error::BadName);
        }

        let name_start = self.pos + HEADER_LEN;
        let name_bytes = self.data.get(name_start..name_start + name_size - 1).ok_or(Error::Truncated)?;
        let name = core::str::from_utf8(name_bytes).map_err(|_| Error::BadName)?;
        let data_start = align4(name_start + name_size);
        let data = self.data.get(data_start..data_start + file_size).ok_or(Error::Truncated)?;
        self.pos = align4(data_start + file_size);

        if name == TRAILER {
            return Ok(None);
        }
        Ok(Some(Entry { name: name.trim_start_matches("./").trim_start_matches('/'), mode, data }))
    }
}

impl<'a> Iterator for Reader<'a> {
    type Item = Result<Entry<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.next_entry() {
            Ok(Some(e)) => Some(Ok(e)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

/// Builds an archive in memory.
pub struct Writer {
    out: Vec<u8>,
    next_ino: u32,
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer {
    pub fn new() -> Self {
        Writer { out: Vec::new(), next_ino: 1 }
    }

    fn push_hex(&mut self, v: u32) {
        const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
        for shift in (0..8).rev() {
            self.out.push(DIGITS[((v >> (shift * 4)) & 0xF) as usize]);
        }
    }

    fn pad4(&mut self) {
        while self.out.len() % 4 != 0 {
            self.out.push(0);
        }
    }

    fn entry(&mut self, name: &str, mode: u32, data: &[u8]) {
        let ino = self.next_ino;
        self.next_ino += 1;
        self.out.extend_from_slice(MAGIC);
        let nlink = if mode & S_IFMT == S_IFDIR { 2 } else { 1 };
        let fields = [ino, mode, 0, 0, nlink, 0, data.len() as u32, 0, 0, 0, 0, name.len() as u32 + 1, 0];
        for f in fields {
            self.push_hex(f);
        }
        self.out.extend_from_slice(name.as_bytes());
        self.out.push(0);
        self.pad4();
        self.out.extend_from_slice(data);
        self.pad4();
    }

    pub fn add_dir(&mut self, name: &str, permissions: u32) {
        self.entry(name, S_IFDIR | (permissions & 0o7777), &[]);
    }

    pub fn add_file(&mut self, name: &str, permissions: u32, data: &[u8]) {
        self.entry(name, S_IFREG | (permissions & 0o7777), data);
    }

    pub fn finish(mut self) -> Vec<u8> {
        self.entry(TRAILER, 0, &[]);
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn roundtrip() {
        let mut w = Writer::new();
        w.add_dir("bin", 0o755);
        w.add_file("bin/hello", 0o755, b"\x7fELF...");
        w.add_file("etc/motd", 0o644, b"hi\n");
        w.add_file("empty", 0o600, b"");
        let archive = w.finish();
        assert_eq!(archive.len() % 4, 0);

        let entries: Vec<Entry> = Reader::new(&archive).collect::<Result<_, _>>().unwrap();
        assert_eq!(entries.len(), 4);
        assert!(entries[0].is_dir());
        assert_eq!(entries[0].name, "bin");
        assert_eq!(entries[1].name, "bin/hello");
        assert_eq!(entries[1].data, b"\x7fELF...");
        assert_eq!(entries[1].permissions(), 0o755);
        assert_eq!(entries[2].data, b"hi\n");
        assert!(entries[3].is_file() && entries[3].data.is_empty());
    }

    #[test]
    fn rejects_garbage() {
        let mut r = Reader::new(b"not an archive at all, definitely not one, no no no no no no no no no no no no no no no no no no no no no no no");
        assert_eq!(r.next().unwrap().unwrap_err(), Error::BadMagic);
        assert!(r.next().is_none());
    }

    #[test]
    fn truncated_archive() {
        let mut w = Writer::new();
        w.add_file("f", 0o644, &vec![1u8; 100]);
        let archive = w.finish();
        let cut = &archive[..150];
        let result: Result<Vec<Entry>, Error> = Reader::new(cut).collect();
        assert_eq!(result.unwrap_err(), Error::Truncated);
    }

    #[test]
    fn strips_leading_dot_slash() {
        let mut w = Writer::new();
        w.add_file("./etc/passwd", 0o644, b"root");
        let archive = w.finish();
        let e = Reader::new(&archive).next().unwrap().unwrap();
        assert_eq!(e.name, "etc/passwd");
    }
}
