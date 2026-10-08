//! POSIX ustar archives: 512-byte headers, data padded to 512 bytes, two
//! zero blocks at the end. Names longer than 100 bytes use the prefix field.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

const BLOCK: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Truncated,
    BadChecksum,
    BadNumber,
    NameTooLong,
}

#[derive(Debug, Clone)]
pub struct Entry<'a> {
    pub name: String,
    pub kind: Kind,
    pub mode: u32,
    pub mtime: u64,
    pub link: String,
    pub data: &'a [u8],
}

fn octal(field: &[u8]) -> Result<u64, Error> {
    let s = core::str::from_utf8(field).map_err(|_| Error::BadNumber)?;
    let s = s.trim_matches(|c: char| c == '\0' || c == ' ');
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).map_err(|_| Error::BadNumber)
}

fn cstr(field: &[u8]) -> String {
    let n = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..n]).into_owned()
}

/// Iterates over the entries of an archive.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, pos: 0 }
    }
}

impl<'a> Iterator for Reader<'a> {
    type Item = Result<Entry<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let header = self.data.get(self.pos..self.pos + BLOCK)?;
        if header.iter().all(|&b| b == 0) {
            return None;
        }
        let stored = match octal(&header[148..156]) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        let sum: u64 = header.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u64 }).sum();
        if sum != stored {
            self.pos = self.data.len();
            return Some(Err(Error::BadChecksum));
        }
        let size = match octal(&header[124..136]) {
            Ok(v) => v as usize,
            Err(e) => return Some(Err(e)),
        };
        let mut name = cstr(&header[0..100]);
        if &header[257..262] == b"ustar" {
            let prefix = cstr(&header[345..500]);
            if !prefix.is_empty() {
                name = alloc::format!("{}/{}", prefix, name);
            }
        }
        let kind = match header[156] {
            b'0' | 0 => Kind::File,
            b'5' => Kind::Directory,
            b'2' => Kind::Symlink,
            _ => Kind::Other,
        };
        let start = self.pos + BLOCK;
        let Some(data) = self.data.get(start..start + size) else {
            self.pos = self.data.len();
            return Some(Err(Error::Truncated));
        };
        self.pos = start + size.div_ceil(BLOCK) * BLOCK;
        Some(Ok(Entry {
            name: String::from(name.trim_start_matches("./")),
            kind,
            mode: octal(&header[100..108]).unwrap_or(0o644) as u32,
            mtime: octal(&header[136..148]).unwrap_or(0),
            link: cstr(&header[157..257]),
            data,
        }))
    }
}

/// Builds an archive in memory.
#[derive(Default)]
pub struct Writer {
    out: Vec<u8>,
}

fn put_octal(field: &mut [u8], value: u64) {
    let width = field.len() - 1;
    let s = alloc::format!("{:0w$o}", value, w = width);
    field[..width].copy_from_slice(&s.as_bytes()[s.len() - width..]);
    field[width] = 0;
}

impl Writer {
    pub fn new() -> Writer {
        Writer { out: Vec::new() }
    }

    fn header(&mut self, name: &str, kind: u8, mode: u32, size: usize, mtime: u64) -> Result<(), Error> {
        let mut h = vec![0u8; BLOCK];
        let bytes = name.as_bytes();
        if bytes.len() <= 100 {
            h[..bytes.len()].copy_from_slice(bytes);
        } else {
            // Split at a '/' so that prefix <= 155 and name <= 100 bytes.
            let split = (0..bytes.len())
                .rev()
                .find(|&i| bytes[i] == b'/' && i <= 155 && bytes.len() - i - 1 <= 100)
                .ok_or(Error::NameTooLong)?;
            h[..bytes.len() - split - 1].copy_from_slice(&bytes[split + 1..]);
            h[345..345 + split].copy_from_slice(&bytes[..split]);
        }
        put_octal(&mut h[100..108], mode as u64 & 0o7777);
        put_octal(&mut h[108..116], 0);
        put_octal(&mut h[116..124], 0);
        put_octal(&mut h[124..136], size as u64);
        put_octal(&mut h[136..148], mtime);
        h[156] = kind;
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[265..269].copy_from_slice(b"root");
        h[297..301].copy_from_slice(b"root");
        h[148..156].fill(b' ');
        let sum: u32 = h.iter().map(|&b| b as u32).sum();
        let s = alloc::format!("{:06o}\0 ", sum);
        h[148..156].copy_from_slice(s.as_bytes());
        self.out.extend_from_slice(&h);
        Ok(())
    }

    pub fn add_file(&mut self, name: &str, mode: u32, mtime: u64, data: &[u8]) -> Result<(), Error> {
        self.header(name, b'0', mode, data.len(), mtime)?;
        self.out.extend_from_slice(data);
        let pad = data.len().div_ceil(BLOCK) * BLOCK - data.len();
        self.out.extend(core::iter::repeat_n(0, pad));
        Ok(())
    }

    pub fn add_dir(&mut self, name: &str, mode: u32, mtime: u64) -> Result<(), Error> {
        let n = if name.ends_with('/') { String::from(name) } else { alloc::format!("{}/", name) };
        self.header(&n, b'5', mode, 0, mtime)
    }

    pub fn finish(mut self) -> Vec<u8> {
        self.out.extend(core::iter::repeat_n(0, BLOCK * 2));
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_long_names() {
        let long = "usr/share/doc/some-package-with-a-long-name/subdirectory/another-level/yet-another-level/file-with-a-long-name.txt";
        assert!(long.len() > 100);
        let mut w = Writer::new();
        w.add_dir("usr", 0o755, 1).unwrap();
        w.add_file("usr/bin/hello", 0o755, 2, b"#!/bin/sh\necho hi\n").unwrap();
        w.add_file(long, 0o644, 3, &[7u8; 1000]).unwrap();
        w.add_file("empty", 0o600, 4, b"").unwrap();
        let data = w.finish();
        assert_eq!(data.len() % 512, 0);
        let entries: Vec<Entry> = Reader::new(&data).collect::<Result<_, _>>().unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].kind, Kind::Directory);
        assert_eq!(entries[0].name, "usr/");
        assert_eq!(entries[1].mode, 0o755);
        assert_eq!(entries[1].data, b"#!/bin/sh\necho hi\n");
        assert_eq!(entries[2].name, long);
        assert_eq!(entries[2].data.len(), 1000);
        assert_eq!(entries[3].data.len(), 0);
    }

    #[test]
    fn detects_corruption() {
        let mut w = Writer::new();
        w.add_file("a", 0o644, 0, b"data").unwrap();
        let mut data = w.finish();
        data[0] = b'b';
        assert_eq!(Reader::new(&data).next().unwrap().unwrap_err(), Error::BadChecksum);
    }
}
