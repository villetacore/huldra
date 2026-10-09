//! DEFLATE (RFC 1951) with the zlib (RFC 1950) and gzip (RFC 1952)
//! wrappers: decompression of everything a server or a git repository may
//! send, and compression with LZ77 and the fixed Huffman code (good
//! enough for git objects and HTTP bodies, simple enough to read).

#![no_std]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The input ended in the middle of the stream.
    Truncated,
    /// Not valid DEFLATE / zlib / gzip data.
    Corrupt(&'static str),
    /// A checksum (Adler-32 or CRC-32) did not match.
    Checksum,
}

pub type Result<T> = core::result::Result<T, Error>;

// ------------------------------------------------------------------ inflate

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
    nbits: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bits { data, pos: 0, bit: 0, nbits: 0 }
    }

    fn need(&mut self, n: u32) -> Result<()> {
        while self.nbits < n {
            let b = *self.data.get(self.pos).ok_or(Error::Truncated)?;
            self.pos += 1;
            self.bit |= (b as u32) << self.nbits;
            self.nbits += 8;
        }
        Ok(())
    }

    fn get(&mut self, n: u32) -> Result<u32> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let v = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.nbits -= n;
        Ok(v)
    }

    fn align(&mut self) {
        self.bit = 0;
        self.nbits = 0;
    }

    /// Bytes consumed so far (after `align`).
    fn byte_pos(&self) -> usize {
        self.pos - (self.nbits / 8) as usize
    }
}

/// A canonical Huffman code: counts per length and symbols by code.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Huffman> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        let mut offs = [0u16; 16];
        for i in 1..16 {
            offs[i] = offs[i - 1] + counts[i - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        // Over-subscribed codes are invalid; incomplete ones are allowed
        // (a single distance code is common).
        let mut left: i32 = 1;
        for &c in &counts[1..] {
            left = left * 2 - c as i32;
            if left < 0 {
                return Err(Error::Corrupt("over-subscribed Huffman code"));
            }
        }
        Ok(Huffman { counts, symbols })
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.get(1)? as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(Error::Corrupt("bad Huffman code"))
    }
}

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

fn fixed_lengths() -> ([u8; 288], [u8; 30]) {
    let mut lit = [0u8; 288];
    for (i, l) in lit.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    (lit, [5u8; 30])
}

fn codes(bits: &mut Bits, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman) -> Result<()> {
    loop {
        let sym = lit.decode(bits)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            257..=285 => {
                let i = (sym - 257) as usize;
                let len = LEN_BASE[i] as usize + bits.get(LEN_EXTRA[i] as u32)? as usize;
                let d = dist.decode(bits)? as usize;
                if d >= 30 {
                    return Err(Error::Corrupt("bad distance symbol"));
                }
                let distance = DIST_BASE[d] as usize + bits.get(DIST_EXTRA[d] as u32)? as usize;
                if distance > out.len() {
                    return Err(Error::Corrupt("distance too far back"));
                }
                let start = out.len() - distance;
                for k in 0..len {
                    out.push(out[start + k]);
                }
            }
            _ => return Err(Error::Corrupt("bad literal/length symbol")),
        }
    }
}

const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

fn dynamic(bits: &mut Bits) -> Result<(Huffman, Huffman)> {
    let nlen = bits.get(5)? as usize + 257;
    let ndist = bits.get(5)? as usize + 1;
    let ncode = bits.get(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(Error::Corrupt("too many codes"));
    }
    let mut cl = [0u8; 19];
    for &i in CL_ORDER.iter().take(ncode) {
        cl[i] = bits.get(3)? as u8;
    }
    let clh = Huffman::new(&cl)?;
    let mut lengths = vec![0u8; nlen + ndist];
    let mut i = 0;
    while i < nlen + ndist {
        let sym = clh.decode(bits)?;
        let (val, rep) = match sym {
            0..=15 => (sym as u8, 1),
            16 => {
                let prev = *lengths.get(i.wrapping_sub(1)).filter(|_| i > 0).ok_or(Error::Corrupt("repeat with no previous length"))?;
                (prev, 3 + bits.get(2)? as usize)
            }
            17 => (0, 3 + bits.get(3)? as usize),
            _ => (0, 11 + bits.get(7)? as usize),
        };
        if i + rep > nlen + ndist {
            return Err(Error::Corrupt("too many lengths"));
        }
        lengths[i..i + rep].fill(val);
        i += rep;
    }
    if lengths[256] == 0 {
        return Err(Error::Corrupt("no end-of-block code"));
    }
    Ok((Huffman::new(&lengths[..nlen])?, Huffman::new(&lengths[nlen..])?))
}

/// Decompresses raw DEFLATE data. Returns the output and the number of
/// input bytes used (data may follow the stream, as in git pack files).
pub fn inflate(data: &[u8]) -> Result<(Vec<u8>, usize)> {
    let mut out = Vec::with_capacity(data.len() * 3);
    let mut bits = Bits::new(data);
    loop {
        let last = bits.get(1)? == 1;
        match bits.get(2)? {
            0 => {
                bits.align();
                let p = bits.pos;
                let hdr = data.get(p..p + 4).ok_or(Error::Truncated)?;
                let len = u16::from_le_bytes([hdr[0], hdr[1]]) as usize;
                let nlen = u16::from_le_bytes([hdr[2], hdr[3]]) as usize;
                if len != !nlen & 0xFFFF {
                    return Err(Error::Corrupt("stored block length"));
                }
                out.extend_from_slice(data.get(p + 4..p + 4 + len).ok_or(Error::Truncated)?);
                bits.pos = p + 4 + len;
            }
            1 => {
                let (l, d) = fixed_lengths();
                codes(&mut bits, &mut out, &Huffman::new(&l)?, &Huffman::new(&d)?)?;
            }
            2 => {
                let (l, d) = dynamic(&mut bits)?;
                codes(&mut bits, &mut out, &l, &d)?;
            }
            _ => return Err(Error::Corrupt("reserved block type")),
        }
        if last {
            bits.align();
            return Ok((out, bits.byte_pos()));
        }
    }
}

// ----------------------------------------------------------------- checksums

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

// ------------------------------------------------------------------ wrappers

/// Decompresses a zlib stream; returns the data and the bytes consumed.
pub fn zlib_decompress(data: &[u8]) -> Result<(Vec<u8>, usize)> {
    if data.len() < 2 {
        return Err(Error::Truncated);
    }
    let (cmf, flg) = (data[0], data[1]);
    if cmf & 0x0F != 8 || (cmf as u16 * 256 + flg as u16) % 31 != 0 {
        return Err(Error::Corrupt("not a zlib stream"));
    }
    if flg & 0x20 != 0 {
        return Err(Error::Corrupt("zlib preset dictionary"));
    }
    let (out, used) = inflate(&data[2..])?;
    let end = 2 + used;
    let sum = data.get(end..end + 4).ok_or(Error::Truncated)?;
    if u32::from_be_bytes([sum[0], sum[1], sum[2], sum[3]]) != adler32(&out) {
        return Err(Error::Checksum);
    }
    Ok((out, end + 4))
}

/// Decompresses gzip data (all members).
pub fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < data.len() {
        let h = data.get(p..p + 10).ok_or(Error::Truncated)?;
        if h[0] != 0x1F || h[1] != 0x8B || h[2] != 8 {
            return Err(Error::Corrupt("not gzip data"));
        }
        let flags = h[3];
        p += 10;
        if flags & 4 != 0 {
            let x = data.get(p..p + 2).ok_or(Error::Truncated)?;
            p += 2 + u16::from_le_bytes([x[0], x[1]]) as usize;
        }
        for bit in [8u8, 16] {
            if flags & bit != 0 {
                p += data.get(p..).ok_or(Error::Truncated)?.iter().position(|&b| b == 0).ok_or(Error::Truncated)? + 1;
            }
        }
        if flags & 2 != 0 {
            p += 2;
        }
        let (member, used) = inflate(data.get(p..).ok_or(Error::Truncated)?)?;
        p += used;
        let t = data.get(p..p + 8).ok_or(Error::Truncated)?;
        if u32::from_le_bytes([t[0], t[1], t[2], t[3]]) != crc32(&member) {
            return Err(Error::Checksum);
        }
        p += 8;
        out.extend_from_slice(&member);
        // Trailing zero padding is allowed after the last member.
        if data[p..].iter().all(|&b| b == 0) {
            break;
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------- deflate

struct BitWriter {
    out: Vec<u8>,
    bit: u64,
    nbits: u32,
}

impl BitWriter {
    fn put(&mut self, v: u32, n: u32) {
        self.bit |= (v as u64) << self.nbits;
        self.nbits += n;
        while self.nbits >= 8 {
            self.out.push(self.bit as u8);
            self.bit >>= 8;
            self.nbits -= 8;
        }
    }

    /// Huffman codes are sent most significant bit first.
    fn put_rev(&mut self, code: u32, len: u32) {
        let mut r = 0;
        for i in 0..len {
            r |= ((code >> i) & 1) << (len - 1 - i);
        }
        self.put(r, len);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            self.out.push(self.bit as u8);
        }
        self.out
    }
}

fn put_literal(w: &mut BitWriter, sym: u32) {
    match sym {
        0..=143 => w.put_rev(0x30 + sym, 8),
        144..=255 => w.put_rev(0x190 + sym - 144, 9),
        256..=279 => w.put_rev(sym - 256, 7),
        _ => w.put_rev(0xC0 + sym - 280, 8),
    }
}

fn put_match(w: &mut BitWriter, len: usize, dist: usize) {
    let i = LEN_BASE.iter().rposition(|&b| b as usize <= len).unwrap();
    put_literal(w, 257 + i as u32);
    w.put((len - LEN_BASE[i] as usize) as u32, LEN_EXTRA[i] as u32);
    let d = DIST_BASE.iter().rposition(|&b| b as usize <= dist).unwrap();
    w.put_rev(d as u32, 5);
    w.put((dist - DIST_BASE[d] as usize) as u32, DIST_EXTRA[d] as u32);
}

/// Compresses with LZ77 (hash chains over a 32 KiB window) and the fixed
/// Huffman code, in one block.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    const WINDOW: usize = 32768;
    const HASH: usize = 1 << 14;
    const MAX_CHAIN: usize = 64;
    let mut w = BitWriter { out: Vec::with_capacity(data.len() / 2 + 16), bit: 0, nbits: 0 };
    w.put(1, 1); // last block
    w.put(1, 2); // fixed Huffman
    let mut head = vec![usize::MAX; HASH];
    let mut prev = vec![usize::MAX; WINDOW];
    let hash = |p: usize| -> usize { ((data[p] as usize) << 10 ^ (data[p + 1] as usize) << 5 ^ data[p + 2] as usize) & (HASH - 1) };
    let insert = |p: usize, head: &mut Vec<usize>, prev: &mut Vec<usize>| {
        if p + 3 <= data.len() {
            let h = hash(p);
            prev[p % WINDOW] = head[h];
            head[h] = p;
        }
    };
    let mut i = 0;
    while i < data.len() {
        let (mut best_len, mut best_dist) = (0, 0);
        if i + 3 <= data.len() {
            let mut cand = head[hash(i)];
            let mut chain = 0;
            while cand != usize::MAX && i - cand <= WINDOW - 1 && chain < MAX_CHAIN {
                let max = (data.len() - i).min(258);
                let mut l = 0;
                while l < max && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - cand;
                    if l == max {
                        break;
                    }
                }
                let next = prev[cand % WINDOW];
                if next == usize::MAX || next >= cand {
                    break;
                }
                cand = next;
                chain += 1;
            }
        }
        if best_len >= 3 {
            put_match(&mut w, best_len, best_dist);
            for k in 0..best_len {
                insert(i + k, &mut head, &mut prev);
            }
            i += best_len;
        } else {
            put_literal(&mut w, data[i] as u32);
            insert(i, &mut head, &mut prev);
            i += 1;
        }
    }
    put_literal(&mut w, 256);
    w.finish()
}

/// Compresses into a zlib stream.
pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut out = alloc::vec![0x78, 0x9C];
    out.extend_from_slice(&deflate(data));
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

#[cfg(test)]
mod tests;
