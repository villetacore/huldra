//! The index (`.git/index`, the staging area): versions 2 and 3 are read,
//! version 2 is written. Extensions (cached trees and so on) are dropped
//! on write, which git accepts and rebuilds.

use crate::object::Id;
use crate::{Error, Result};
use alloc::string::String;
use alloc::vec::Vec;
use huldra_crypto::sha::{Hash, Sha1};

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Entry {
    pub ctime: (u32, u32),
    pub mtime: (u32, u32),
    pub dev: u32,
    pub ino: u32,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u32,
    pub id: Id,
    /// Stage (merge conflicts) and flags other than the name length.
    pub flags: u16,
    pub path: String,
}

fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes(b[o..o + 4].try_into().unwrap())
}

pub fn parse(data: &[u8]) -> Result<Vec<Entry>> {
    if data.len() < 32 || &data[..4] != b"DIRC" {
        return Err(Error("index: bad signature".into()));
    }
    let body = data.len() - 20;
    if Sha1::digest(&data[..body])[..] != data[body..] {
        return Err(Error("index: checksum mismatch".into()));
    }
    let version = be32(data, 4);
    if version != 2 && version != 3 {
        return Err(Error(alloc::format!("index: version {} is not supported", version)));
    }
    let count = be32(data, 8) as usize;
    let mut entries = Vec::with_capacity(count);
    let mut p = 12;
    for _ in 0..count {
        let e = data.get(p..p + 62).ok_or_else(|| Error("index: truncated".into()))?;
        let flags = u16::from_be_bytes([e[60], e[61]]);
        let mut name_start = p + 62;
        if version == 3 && flags & 0x4000 != 0 {
            name_start += 2; // extended flags
        }
        let nul = data[name_start..].iter().position(|&b| b == 0).ok_or_else(|| Error("index: bad name".into()))? + name_start;
        entries.push(Entry {
            ctime: (be32(e, 0), be32(e, 4)),
            mtime: (be32(e, 8), be32(e, 12)),
            dev: be32(e, 16),
            ino: be32(e, 20),
            mode: be32(e, 24),
            uid: be32(e, 28),
            gid: be32(e, 32),
            size: be32(e, 36),
            id: Id(e[40..60].try_into().unwrap()),
            flags: flags & 0x3000,
            path: String::from_utf8_lossy(&data[name_start..nul]).into_owned(),
        });
        // Entries are padded with NULs to a multiple of 8 bytes.
        let len = nul - p;
        p += (len + 8) & !7;
    }
    Ok(entries)
}

pub fn write(entries: &[Entry]) -> Vec<u8> {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let mut out = Vec::new();
    out.extend_from_slice(b"DIRC");
    out.extend_from_slice(&2u32.to_be_bytes());
    out.extend_from_slice(&(sorted.len() as u32).to_be_bytes());
    for e in sorted {
        let start = out.len();
        for v in [e.ctime.0, e.ctime.1, e.mtime.0, e.mtime.1, e.dev, e.ino, e.mode, e.uid, e.gid, e.size] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&e.id.0);
        let name_len = e.path.len().min(0xFFF) as u16;
        out.extend_from_slice(&((e.flags & 0x3000) | name_len).to_be_bytes());
        out.extend_from_slice(e.path.as_bytes());
        let len = out.len() - start;
        let padded = (len + 8) & !7;
        out.resize(start + padded, 0);
    }
    let sum = Sha1::digest(&out);
    out.extend_from_slice(&sum);
    out
}
