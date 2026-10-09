//! SHA-1 (for git object names), SHA-256, SHA-384 and SHA-512 (FIPS 180-4).

/// A hash function as TLS and HMAC use it.
pub trait Hash: Clone {
    const OUTPUT: usize;
    const BLOCK: usize;
    fn new() -> Self;
    fn update(&mut self, data: &[u8]);
    /// Writes the digest to the start of `out`.
    fn finish_into(self, out: &mut [u8]);

    fn digest(data: &[u8]) -> alloc::vec::Vec<u8> {
        let mut h = Self::new();
        h.update(data);
        let mut out = alloc::vec![0u8; Self::OUTPUT];
        h.finish_into(&mut out);
        out
    }
}

/// Buffers input into blocks for a compression function.
#[derive(Clone)]
struct Blocks<const N: usize> {
    buf: [u8; N],
    len: usize,
    total: u128,
}

impl<const N: usize> Blocks<N> {
    const fn new() -> Self {
        Blocks { buf: [0; N], len: 0, total: 0 }
    }

    fn update(&mut self, mut data: &[u8], mut compress: impl FnMut(&[u8])) {
        self.total += data.len() as u128;
        if self.len > 0 {
            let take = (N - self.len).min(data.len());
            self.buf[self.len..self.len + take].copy_from_slice(&data[..take]);
            self.len += take;
            data = &data[take..];
            if self.len < N {
                return; // everything fit in the partial block
            }
            compress(&self.buf);
            self.len = 0;
        }
        while data.len() >= N {
            compress(&data[..N]);
            data = &data[N..];
        }
        self.buf[..data.len()].copy_from_slice(data);
        self.len = data.len();
    }

    /// MD-style padding with a big-endian bit length of `len_bytes` bytes.
    fn pad(&mut self, len_bytes: usize, mut compress: impl FnMut(&[u8])) {
        let bits = self.total * 8;
        let mut tail = [0u8; 256];
        let mut n = 0;
        tail[n] = 0x80;
        n += 1;
        while (self.len + n) % N != N - len_bytes {
            n += 1;
        }
        let be = bits.to_be_bytes();
        tail[n..n + len_bytes].copy_from_slice(&be[16 - len_bytes..]);
        n += len_bytes;
        let total = self.total;
        let buffered = self.len;
        let mut all = [0u8; 512];
        all[..buffered].copy_from_slice(&self.buf[..buffered]);
        all[buffered..buffered + n].copy_from_slice(&tail[..n]);
        for block in all[..buffered + n].chunks(N) {
            compress(block);
        }
        self.total = total;
    }
}

// ------------------------------------------------------------------- SHA-1

#[derive(Clone)]
pub struct Sha1 {
    h: [u32; 5],
    blocks: Blocks<64>,
}

fn sha1_compress(h: &mut [u32; 5], block: &[u8]) {
    let mut w = [0u32; 80];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]]);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *h;
    for (i, wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
            20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
            _ => (b ^ c ^ d, 0xCA62_C1D6),
        };
        let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = t;
    }
    for (x, v) in h.iter_mut().zip([a, b, c, d, e]) {
        *x = x.wrapping_add(v);
    }
}

impl Hash for Sha1 {
    const OUTPUT: usize = 20;
    const BLOCK: usize = 64;
    fn new() -> Self {
        Sha1 { h: [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0], blocks: Blocks::new() }
    }
    fn update(&mut self, data: &[u8]) {
        let h = &mut self.h;
        self.blocks.update(data, |b| sha1_compress(h, b));
    }
    fn finish_into(mut self, out: &mut [u8]) {
        let h = &mut self.h;
        self.blocks.pad(8, |b| sha1_compress(h, b));
        for (i, v) in self.h.iter().enumerate() {
            out[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
        }
    }
}

impl Sha1 {
    pub fn finish(self) -> [u8; 20] {
        let mut out = [0u8; 20];
        self.finish_into(&mut out);
        out
    }
}

// ----------------------------------------------------------------- SHA-256

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

#[derive(Clone)]
pub struct Sha256 {
    h: [u32; 8],
    blocks: Blocks<64>,
}

fn sha256_compress(h: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
    }
    let mut v = *h;
    for i in 0..64 {
        let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
        let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
        let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K256[i]).wrapping_add(w[i]);
        let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
        let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
        let t2 = s0.wrapping_add(maj);
        v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
    }
    for (x, y) in h.iter_mut().zip(v) {
        *x = x.wrapping_add(y);
    }
}

impl Hash for Sha256 {
    const OUTPUT: usize = 32;
    const BLOCK: usize = 64;
    fn new() -> Self {
        Sha256 { h: [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19], blocks: Blocks::new() }
    }
    fn update(&mut self, data: &[u8]) {
        let h = &mut self.h;
        self.blocks.update(data, |b| sha256_compress(h, b));
    }
    fn finish_into(mut self, out: &mut [u8]) {
        let h = &mut self.h;
        self.blocks.pad(8, |b| sha256_compress(h, b));
        for (i, v) in self.h.iter().enumerate() {
            out[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
        }
    }
}

impl Sha256 {
    pub fn finish(self) -> [u8; 32] {
        let mut out = [0u8; 32];
        self.finish_into(&mut out);
        out
    }
}

// ----------------------------------------------------------- SHA-384/512

const K512: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc, 0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118,
    0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2, 0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694,
    0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65, 0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
    0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4, 0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70,
    0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df, 0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
    0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30, 0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8, 0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec, 0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b,
    0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178, 0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
    0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c, 0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

fn sha512_compress(h: &mut [u64; 8], block: &[u8]) {
    let mut w = [0u64; 80];
    for i in 0..16 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&block[8 * i..8 * i + 8]);
        w[i] = u64::from_be_bytes(b);
    }
    for i in 16..80 {
        let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
        let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
    }
    let mut v = *h;
    for i in 0..80 {
        let s1 = v[4].rotate_right(14) ^ v[4].rotate_right(18) ^ v[4].rotate_right(41);
        let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
        let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K512[i]).wrapping_add(w[i]);
        let s0 = v[0].rotate_right(28) ^ v[0].rotate_right(34) ^ v[0].rotate_right(39);
        let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
        let t2 = s0.wrapping_add(maj);
        v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
    }
    for (x, y) in h.iter_mut().zip(v) {
        *x = x.wrapping_add(y);
    }
}

#[derive(Clone)]
pub struct Sha512 {
    h: [u64; 8],
    blocks: Blocks<128>,
}

impl Sha512 {
    fn with(h: [u64; 8]) -> Self {
        Sha512 { h, blocks: Blocks::new() }
    }
    fn finish_words(mut self) -> [u64; 8] {
        let h = &mut self.h;
        self.blocks.pad(16, |b| sha512_compress(h, b));
        self.h
    }
}

impl Hash for Sha512 {
    const OUTPUT: usize = 64;
    const BLOCK: usize = 128;
    fn new() -> Self {
        Sha512::with([
            0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1, 0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179,
        ])
    }
    fn update(&mut self, data: &[u8]) {
        let h = &mut self.h;
        self.blocks.update(data, |b| sha512_compress(h, b));
    }
    fn finish_into(self, out: &mut [u8]) {
        for (i, v) in self.finish_words().iter().enumerate() {
            out[8 * i..8 * i + 8].copy_from_slice(&v.to_be_bytes());
        }
    }
}

#[derive(Clone)]
pub struct Sha384(Sha512);

impl Hash for Sha384 {
    const OUTPUT: usize = 48;
    const BLOCK: usize = 128;
    fn new() -> Self {
        Sha384(Sha512::with([
            0xcbbb9d5dc1059ed8, 0x629a292a367cd507, 0x9159015a3070dd17, 0x152fecd8f70e5939, 0x67332667ffc00b31, 0x8eb44a8768581511, 0xdb0c2e0d64f98fa7, 0x47b5481dbefa4fa4,
        ]))
    }
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finish_into(self, out: &mut [u8]) {
        for (i, v) in self.0.finish_words().iter().take(6).enumerate() {
            out[8 * i..8 * i + 8].copy_from_slice(&v.to_be_bytes());
        }
    }
}

// --------------------------------------------------------------- HMAC/HKDF

/// HMAC (RFC 2104).
pub fn hmac<H: Hash>(key: &[u8], data: &[&[u8]]) -> alloc::vec::Vec<u8> {
    let mut k = alloc::vec![0u8; H::BLOCK];
    if key.len() > H::BLOCK {
        let d = H::digest(key);
        k[..d.len()].copy_from_slice(&d);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = H::new();
    let ipad: alloc::vec::Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    inner.update(&ipad);
    for d in data {
        inner.update(d);
    }
    let mut ih = alloc::vec![0u8; H::OUTPUT];
    inner.finish_into(&mut ih);
    let mut outer = H::new();
    let opad: alloc::vec::Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    outer.update(&opad);
    outer.update(&ih);
    let mut out = alloc::vec![0u8; H::OUTPUT];
    outer.finish_into(&mut out);
    out
}

/// HKDF-Extract (RFC 5869).
pub fn hkdf_extract<H: Hash>(salt: &[u8], ikm: &[u8]) -> alloc::vec::Vec<u8> {
    let zeros = alloc::vec![0u8; H::OUTPUT];
    hmac::<H>(if salt.is_empty() { &zeros } else { salt }, &[ikm])
}

/// HKDF-Expand (RFC 5869).
pub fn hkdf_expand<H: Hash>(prk: &[u8], info: &[u8], len: usize) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::with_capacity(len);
    let mut t: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let mut i = 1u8;
    while out.len() < len {
        t = hmac::<H>(prk, &[&t, info, &[i]]);
        out.extend_from_slice(&t);
        i += 1;
    }
    out.truncate(len);
    out
}
