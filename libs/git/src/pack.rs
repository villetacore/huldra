//! Pack files (`objects/pack/pack-*.pack`) and their `.idx` (version 2):
//! parsing what a server sends, resolving deltas, writing the index git
//! expects next to it, reading single objects back, and building packs to
//! push.

use crate::object::{self, Id, Kind};
use crate::{Error, Result};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::vec::Vec;
use huldra_crypto::sha::{Hash, Sha1};

const OFS_DELTA: u8 = 6;
const REF_DELTA: u8 = 7;

fn err(s: &str) -> Error {
    Error(format!("pack: {}", s))
}

/// Applies a git delta to `base`.
pub fn apply_delta(base: &[u8], delta: &[u8]) -> Result<Vec<u8>> {
    let mut p = 0;
    let varint = |p: &mut usize| -> Result<usize> {
        let (mut v, mut shift) = (0usize, 0);
        loop {
            let b = *delta.get(*p).ok_or_else(|| err("delta truncated"))?;
            *p += 1;
            v |= ((b & 0x7F) as usize) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
    };
    let src_len = varint(&mut p)?;
    let dst_len = varint(&mut p)?;
    if src_len != base.len() {
        return Err(err("delta base size mismatch"));
    }
    let mut out = Vec::with_capacity(dst_len);
    while p < delta.len() {
        let op = delta[p];
        p += 1;
        if op & 0x80 != 0 {
            let mut fields = [0usize; 7];
            for (bit, f) in fields.iter_mut().enumerate() {
                if op & (1 << bit) != 0 {
                    *f = *delta.get(p).ok_or_else(|| err("delta truncated"))? as usize;
                    p += 1;
                }
            }
            let offset = fields[0] | fields[1] << 8 | fields[2] << 16 | fields[3] << 24;
            let mut size = fields[4] | fields[5] << 8 | fields[6] << 16;
            if size == 0 {
                size = 0x10000;
            }
            out.extend_from_slice(base.get(offset..offset + size).ok_or_else(|| err("delta copy out of range"))?);
        } else if op != 0 {
            let n = op as usize;
            out.extend_from_slice(delta.get(p..p + n).ok_or_else(|| err("delta truncated"))?);
            p += n;
        } else {
            return Err(err("delta opcode 0"));
        }
    }
    if out.len() != dst_len {
        return Err(err("delta result size mismatch"));
    }
    Ok(out)
}

/// One entry as stored in the pack.
struct Raw {
    offset: usize,
    end: usize,
    ty: u8,
    data: Vec<u8>,
    base_offset: Option<usize>,
    base_id: Option<Id>,
}

/// Reads the entry header and inflates its data.
fn read_entry(pack: &[u8], offset: usize) -> Result<Raw> {
    let mut p = offset;
    let mut b = *pack.get(p).ok_or_else(|| err("truncated"))?;
    p += 1;
    let ty = (b >> 4) & 7;
    let mut size = (b & 15) as usize;
    let mut shift = 4;
    while b & 0x80 != 0 {
        b = *pack.get(p).ok_or_else(|| err("truncated"))?;
        p += 1;
        size |= ((b & 0x7F) as usize) << shift;
        shift += 7;
    }
    let (mut base_offset, mut base_id) = (None, None);
    if ty == OFS_DELTA {
        b = *pack.get(p).ok_or_else(|| err("truncated"))?;
        p += 1;
        let mut off = (b & 0x7F) as usize;
        while b & 0x80 != 0 {
            b = *pack.get(p).ok_or_else(|| err("truncated"))?;
            p += 1;
            off = ((off + 1) << 7) | (b & 0x7F) as usize;
        }
        base_offset = Some(offset.checked_sub(off).ok_or_else(|| err("bad delta offset"))?);
    } else if ty == REF_DELTA {
        base_id = Some(Id(pack.get(p..p + 20).ok_or_else(|| err("truncated"))?.try_into().unwrap()));
        p += 20;
    } else if Kind::from_pack_type(ty).is_none() {
        return Err(err(&format!("bad object type {}", ty)));
    }
    let (data, used) = huldra_flate::zlib_decompress(&pack[p..]).map_err(|e| err(&format!("bad zlib data at {}: {:?}", offset, e)))?;
    if data.len() != size {
        return Err(err("object size mismatch"));
    }
    Ok(Raw { offset, end: p + used, ty, data, base_offset, base_id })
}

/// An object after delta resolution.
#[derive(Clone, Debug)]
pub struct Object {
    pub id: Id,
    pub kind: Kind,
    pub data: Vec<u8>,
    pub offset: usize,
    pub crc: u32,
}

/// Parses a whole pack, resolving deltas. `external` supplies bases that
/// are not in the pack (thin packs).
pub fn parse(pack: &[u8], external: &dyn Fn(&Id) -> Option<(Kind, Vec<u8>)>) -> Result<Vec<Object>> {
    if pack.len() < 32 || &pack[..4] != b"PACK" {
        return Err(err("not a pack file"));
    }
    let version = u32::from_be_bytes(pack[4..8].try_into().unwrap());
    if version != 2 && version != 3 {
        return Err(err("unsupported version"));
    }
    let count = u32::from_be_bytes(pack[8..12].try_into().unwrap()) as usize;
    let body = pack.len() - 20;
    if Sha1::digest(&pack[..body])[..] != pack[body..] {
        return Err(err("checksum mismatch (download corrupted)"));
    }
    let mut raws = Vec::with_capacity(count);
    let mut p = 12;
    for _ in 0..count {
        let r = read_entry(pack, p)?;
        p = r.end;
        raws.push(r);
    }
    // Resolve: plain objects first, then deltas whose base is known, until
    // nothing changes.
    let mut done: BTreeMap<usize, (Kind, Id)> = BTreeMap::new();
    let mut by_id: BTreeMap<Id, usize> = BTreeMap::new();
    let mut out: Vec<Option<Object>> = (0..raws.len()).map(|_| None).collect();
    let crc = |r: &Raw| huldra_flate::crc32(&pack[r.offset..r.end]);
    for (i, r) in raws.iter().enumerate() {
        if let Some(kind) = Kind::from_pack_type(r.ty) {
            let id = object::hash(kind, &r.data);
            done.insert(r.offset, (kind, id));
            by_id.insert(id, i);
            out[i] = Some(Object { id, kind, data: r.data.clone(), offset: r.offset, crc: crc(r) });
        }
    }
    let index_of: BTreeMap<usize, usize> = raws.iter().enumerate().map(|(i, r)| (r.offset, i)).collect();
    loop {
        let mut progress = false;
        let mut pending = false;
        for i in 0..raws.len() {
            if out[i].is_some() {
                continue;
            }
            let r = &raws[i];
            let base: Option<(Kind, Vec<u8>)> = if let Some(off) = r.base_offset {
                index_of.get(&off).and_then(|&j| out[j].as_ref()).map(|o| (o.kind, o.data.clone()))
            } else {
                let id = r.base_id.unwrap();
                match by_id.get(&id) {
                    Some(&j) => out[j].as_ref().map(|o| (o.kind, o.data.clone())),
                    None => external(&id),
                }
            };
            match base {
                Some((kind, base)) => {
                    let data = apply_delta(&base, &r.data)?;
                    let id = object::hash(kind, &data);
                    by_id.insert(id, i);
                    out[i] = Some(Object { id, kind, data, offset: r.offset, crc: crc(r) });
                    progress = true;
                }
                None => pending = true,
            }
        }
        if !pending {
            break;
        }
        if !progress {
            return Err(err("delta base missing"));
        }
    }
    Ok(out.into_iter().map(|o| o.unwrap()).collect())
}

/// The `.idx` (version 2) for a parsed pack.
pub fn write_index(objects: &[Object], pack_checksum: &[u8]) -> Vec<u8> {
    let mut sorted: Vec<&Object> = objects.iter().collect();
    sorted.sort_by_key(|o| o.id);
    let mut out = Vec::new();
    out.extend_from_slice(&[0xFF, b't', b'O', b'c', 0, 0, 0, 2]);
    let mut fanout = [0u32; 256];
    for o in &sorted {
        fanout[o.id.0[0] as usize] += 1;
    }
    let mut acc = 0;
    for f in fanout.iter_mut() {
        acc += *f;
        *f = acc;
    }
    for f in fanout {
        out.extend_from_slice(&f.to_be_bytes());
    }
    for o in &sorted {
        out.extend_from_slice(&o.id.0);
    }
    for o in &sorted {
        out.extend_from_slice(&o.crc.to_be_bytes());
    }
    let mut large = Vec::new();
    for o in &sorted {
        if o.offset < 0x8000_0000 {
            out.extend_from_slice(&(o.offset as u32).to_be_bytes());
        } else {
            out.extend_from_slice(&(0x8000_0000u32 | large.len() as u32).to_be_bytes());
            large.push(o.offset as u64);
        }
    }
    for l in large {
        out.extend_from_slice(&l.to_be_bytes());
    }
    out.extend_from_slice(pack_checksum);
    let sum = Sha1::digest(&out);
    out.extend_from_slice(&sum);
    out
}

/// A loaded `.idx` file.
pub struct Index {
    data: Vec<u8>,
    count: usize,
}

impl Index {
    pub fn parse(data: Vec<u8>) -> Result<Index> {
        if data.len() < 8 + 1024 || data[..8] != [0xFF, b't', b'O', b'c', 0, 0, 0, 2] {
            return Err(err("unsupported index"));
        }
        let count = u32::from_be_bytes(data[8 + 255 * 4..8 + 256 * 4].try_into().unwrap()) as usize;
        if data.len() < 8 + 1024 + count * 28 + 40 {
            return Err(err("index truncated"));
        }
        Ok(Index { data, count })
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn id_at(&self, i: usize) -> &[u8] {
        let base = 8 + 1024;
        &self.data[base + 20 * i..base + 20 * i + 20]
    }

    /// All ids in the pack.
    pub fn ids(&self) -> impl Iterator<Item = Id> + '_ {
        (0..self.count).map(|i| Id(self.id_at(i).try_into().unwrap()))
    }

    /// The offset of `id` in the pack, if it is there.
    pub fn find(&self, id: &Id) -> Option<usize> {
        let first = id.0[0] as usize;
        let fan = |k: usize| u32::from_be_bytes(self.data[8 + 4 * k..12 + 4 * k].try_into().unwrap()) as usize;
        let (mut lo, mut hi) = (if first == 0 { 0 } else { fan(first - 1) }, fan(first));
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.id_at(mid).cmp(&id.0[..]) {
                core::cmp::Ordering::Equal => {
                    let ofs_base = 8 + 1024 + self.count * 24;
                    let v = u32::from_be_bytes(self.data[ofs_base + 4 * mid..ofs_base + 4 * mid + 4].try_into().unwrap());
                    if v & 0x8000_0000 == 0 {
                        return Some(v as usize);
                    }
                    let large = ofs_base + 4 * self.count + 8 * (v & 0x7FFF_FFFF) as usize;
                    return Some(u64::from_be_bytes(self.data[large..large + 8].try_into().unwrap()) as usize);
                }
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
            }
        }
        None
    }
}

/// Reads the object at `offset`, resolving deltas (bases by offset in the
/// same pack, or by id through `lookup`).
pub fn read_at(pack: &[u8], offset: usize, lookup: &dyn Fn(&Id) -> Option<(Kind, Vec<u8>)>) -> Result<(Kind, Vec<u8>)> {
    let r = read_entry(pack, offset)?;
    if let Some(kind) = Kind::from_pack_type(r.ty) {
        return Ok((kind, r.data));
    }
    let (kind, base) = match (r.base_offset, r.base_id) {
        (Some(off), _) => read_at(pack, off, lookup)?,
        (_, Some(id)) => lookup(&id).ok_or_else(|| err("delta base missing"))?,
        _ => unreachable!(),
    };
    Ok((kind, apply_delta(&base, &r.data)?))
}

/// Builds a pack of whole (undeltified) objects, for pushing.
pub fn build(objects: &[(Kind, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"PACK");
    out.extend_from_slice(&2u32.to_be_bytes());
    out.extend_from_slice(&(objects.len() as u32).to_be_bytes());
    for (kind, data) in objects {
        let mut size = data.len();
        let mut b = (kind.pack_type() << 4) | (size & 15) as u8;
        size >>= 4;
        while size > 0 {
            out.push(b | 0x80);
            b = (size & 0x7F) as u8;
            size >>= 7;
        }
        out.push(b);
        out.extend_from_slice(&huldra_flate::zlib_compress(data));
    }
    let sum = Sha1::digest(&out);
    out.extend_from_slice(&sum);
    out
}
