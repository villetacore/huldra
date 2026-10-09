//! Authenticated encryption for TLS 1.3 records: ChaCha20-Poly1305
//! (RFC 8439) and AES-128-GCM (FIPS 197, SP 800-38D).
//!
//! These are straightforward table-free implementations. AES uses its
//! S-box table, so it is not constant-time; ChaCha20-Poly1305 is preferred
//! when the server allows it.

use alloc::vec::Vec;

/// Seals or opens records with one key.
pub enum Aead {
    ChaCha20Poly1305([u8; 32]),
    Aes128Gcm(Box<Aes128>),
}

use alloc::boxed::Box;

/// The 16-byte authentication tag did not match.
#[derive(Debug, PartialEq, Eq)]
pub struct BadTag;

impl Aead {
    pub const TAG: usize = 16;

    pub fn chacha20_poly1305(key: &[u8]) -> Aead {
        let mut k = [0u8; 32];
        k.copy_from_slice(&key[..32]);
        Aead::ChaCha20Poly1305(k)
    }

    pub fn aes128_gcm(key: &[u8]) -> Aead {
        let mut k = [0u8; 16];
        k.copy_from_slice(&key[..16]);
        Aead::Aes128Gcm(Box::new(Aes128::new(&k)))
    }

    pub fn key_len(&self) -> usize {
        match self {
            Aead::ChaCha20Poly1305(_) => 32,
            Aead::Aes128Gcm(_) => 16,
        }
    }

    /// Encrypts `plaintext`; returns ciphertext followed by the tag.
    pub fn seal(&self, nonce: &[u8; 12], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
        let mut out = plaintext.to_vec();
        let tag = match self {
            Aead::ChaCha20Poly1305(k) => {
                chacha20_xor(k, nonce, 1, &mut out);
                poly1305_tag(k, nonce, aad, &out)
            }
            Aead::Aes128Gcm(aes) => {
                aes.ctr(nonce, 2, &mut out);
                aes.gcm_tag(nonce, aad, &out)
            }
        };
        out.extend_from_slice(&tag);
        out
    }

    /// Decrypts ciphertext-plus-tag.
    pub fn open(&self, nonce: &[u8; 12], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, BadTag> {
        if sealed.len() < 16 {
            return Err(BadTag);
        }
        let (ct, tag) = sealed.split_at(sealed.len() - 16);
        let expect = match self {
            Aead::ChaCha20Poly1305(k) => poly1305_tag(k, nonce, aad, ct),
            Aead::Aes128Gcm(aes) => aes.gcm_tag(nonce, aad, ct),
        };
        if !crate::ct_eq(&expect, tag) {
            return Err(BadTag);
        }
        let mut out = ct.to_vec();
        match self {
            Aead::ChaCha20Poly1305(k) => chacha20_xor(k, nonce, 1, &mut out),
            Aead::Aes128Gcm(aes) => aes.ctr(nonce, 2, &mut out),
        }
        Ok(out)
    }
}

// ------------------------------------------------------------------ ChaCha20

fn quarter(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

pub fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut s = [0u32; 16];
    s[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
    for i in 0..8 {
        s[4 + i] = le32(&key[4 * i..]);
    }
    s[12] = counter;
    for i in 0..3 {
        s[13 + i] = le32(&nonce[4 * i..]);
    }
    let init = s;
    for _ in 0..10 {
        quarter(&mut s, 0, 4, 8, 12);
        quarter(&mut s, 1, 5, 9, 13);
        quarter(&mut s, 2, 6, 10, 14);
        quarter(&mut s, 3, 7, 11, 15);
        quarter(&mut s, 0, 5, 10, 15);
        quarter(&mut s, 1, 6, 11, 12);
        quarter(&mut s, 2, 7, 8, 13);
        quarter(&mut s, 3, 4, 9, 14);
    }
    let mut out = [0u8; 64];
    for i in 0..16 {
        out[4 * i..4 * i + 4].copy_from_slice(&s[i].wrapping_add(init[i]).to_le_bytes());
    }
    out
}

pub fn chacha20_xor(key: &[u8; 32], nonce: &[u8; 12], counter: u32, data: &mut [u8]) {
    for (i, chunk) in data.chunks_mut(64).enumerate() {
        let ks = chacha20_block(key, counter.wrapping_add(i as u32), nonce);
        for (b, k) in chunk.iter_mut().zip(ks.iter()) {
            *b ^= k;
        }
    }
}

// ------------------------------------------------------------------ Poly1305

/// Poly1305 with 26-bit limbs (the "donna" layout).
pub fn poly1305(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
    let r0 = le32(&key[0..]) & 0x3ff_ffff;
    let r1 = (le32(&key[3..]) >> 2) & 0x3ff_ff03;
    let r2 = (le32(&key[6..]) >> 4) & 0x3ff_c0ff;
    let r3 = (le32(&key[9..]) >> 6) & 0x3f0_3fff;
    let r4 = (le32(&key[12..]) >> 8) & 0x00f_ffff;
    let (s1, s2, s3, s4) = (r1 * 5, r2 * 5, r3 * 5, r4 * 5);
    let (mut h0, mut h1, mut h2, mut h3, mut h4) = (0u32, 0u32, 0u32, 0u32, 0u32);
    for chunk in msg.chunks(16) {
        let mut b = [0u8; 17];
        b[..chunk.len()].copy_from_slice(chunk);
        b[chunk.len()] = 1;
        h0 += le32(&b[0..]) & 0x3ff_ffff;
        h1 += (le32(&b[3..]) >> 2) & 0x3ff_ffff;
        h2 += (le32(&b[6..]) >> 4) & 0x3ff_ffff;
        h3 += (le32(&b[9..]) >> 6) & 0x3ff_ffff;
        h4 += (le32(&b[12..]) >> 8) | ((b[16] as u32) << 24);
        let m = |a: u32, b: u32| a as u64 * b as u64;
        let d0 = m(h0, r0) + m(h1, s4) + m(h2, s3) + m(h3, s2) + m(h4, s1);
        let mut d1 = m(h0, r1) + m(h1, r0) + m(h2, s4) + m(h3, s3) + m(h4, s2);
        let mut d2 = m(h0, r2) + m(h1, r1) + m(h2, r0) + m(h3, s4) + m(h4, s3);
        let mut d3 = m(h0, r3) + m(h1, r2) + m(h2, r1) + m(h3, r0) + m(h4, s4);
        let mut d4 = m(h0, r4) + m(h1, r3) + m(h2, r2) + m(h3, r1) + m(h4, r0);
        let mut c = (d0 >> 26) as u32;
        h0 = d0 as u32 & 0x3ff_ffff;
        d1 += c as u64;
        c = (d1 >> 26) as u32;
        h1 = d1 as u32 & 0x3ff_ffff;
        d2 += c as u64;
        c = (d2 >> 26) as u32;
        h2 = d2 as u32 & 0x3ff_ffff;
        d3 += c as u64;
        c = (d3 >> 26) as u32;
        h3 = d3 as u32 & 0x3ff_ffff;
        d4 += c as u64;
        c = (d4 >> 26) as u32;
        h4 = d4 as u32 & 0x3ff_ffff;
        h0 += c * 5;
        c = h0 >> 26;
        h0 &= 0x3ff_ffff;
        h1 += c;
    }
    // Full carry, then h - p if h >= p.
    let mut c = h1 >> 26;
    h1 &= 0x3ff_ffff;
    h2 += c;
    c = h2 >> 26;
    h2 &= 0x3ff_ffff;
    h3 += c;
    c = h3 >> 26;
    h3 &= 0x3ff_ffff;
    h4 += c;
    c = h4 >> 26;
    h4 &= 0x3ff_ffff;
    h0 += c * 5;
    c = h0 >> 26;
    h0 &= 0x3ff_ffff;
    h1 += c;
    let mut g0 = h0.wrapping_add(5);
    c = g0 >> 26;
    g0 &= 0x3ff_ffff;
    let mut g1 = h1.wrapping_add(c);
    c = g1 >> 26;
    g1 &= 0x3ff_ffff;
    let mut g2 = h2.wrapping_add(c);
    c = g2 >> 26;
    g2 &= 0x3ff_ffff;
    let mut g3 = h3.wrapping_add(c);
    c = g3 >> 26;
    g3 &= 0x3ff_ffff;
    let g4 = h4.wrapping_add(c).wrapping_sub(1 << 26);
    let mask = (g4 >> 31).wrapping_sub(1); // all ones if g4 did not underflow
    h0 = (h0 & !mask) | (g0 & mask);
    h1 = (h1 & !mask) | (g1 & mask);
    h2 = (h2 & !mask) | (g2 & mask);
    h3 = (h3 & !mask) | (g3 & mask);
    h4 = (h4 & !mask) | (g4 & mask);
    // h mod 2^128, plus s.
    let w0 = h0 | (h1 << 26);
    let w1 = (h1 >> 6) | (h2 << 20);
    let w2 = (h2 >> 12) | (h3 << 14);
    let w3 = (h3 >> 18) | (h4 << 8);
    let mut f = w0 as u64 + le32(&key[16..]) as u64;
    let o0 = f as u32;
    f = w1 as u64 + le32(&key[20..]) as u64 + (f >> 32);
    let o1 = f as u32;
    f = w2 as u64 + le32(&key[24..]) as u64 + (f >> 32);
    let o2 = f as u32;
    f = w3 as u64 + le32(&key[28..]) as u64 + (f >> 32);
    let o3 = f as u32;
    let mut out = [0u8; 16];
    for (i, v) in [o0, o1, o2, o3].iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

fn pad16(v: &mut Vec<u8>) {
    while v.len() % 16 != 0 {
        v.push(0);
    }
}

fn poly1305_tag(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], ct: &[u8]) -> [u8; 16] {
    let block = chacha20_block(key, 0, nonce);
    let mut otk = [0u8; 32];
    otk.copy_from_slice(&block[..32]);
    let mut mac = Vec::with_capacity(aad.len() + ct.len() + 48);
    mac.extend_from_slice(aad);
    pad16(&mut mac);
    mac.extend_from_slice(ct);
    pad16(&mut mac);
    mac.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    mac.extend_from_slice(&(ct.len() as u64).to_le_bytes());
    poly1305(&otk, &mac)
}

// ----------------------------------------------------------------------- AES

const SBOX: [u8; 256] = {
    // Generated at compile time from the multiplicative inverse in GF(2^8).
    let mut sbox = [0u8; 256];
    let mut p: u8 = 1;
    let mut q: u8 = 1;
    loop {
        // p * 3
        p = p ^ (p << 1) ^ (if p & 0x80 != 0 { 0x1B } else { 0 });
        // q / 3
        q ^= q << 1;
        q ^= q << 2;
        q ^= q << 4;
        if q & 0x80 != 0 {
            q ^= 0x09;
        }
        let x = q ^ q.rotate_left(1) ^ q.rotate_left(2) ^ q.rotate_left(3) ^ q.rotate_left(4);
        sbox[p as usize] = x ^ 0x63;
        if p == 1 {
            break;
        }
    }
    sbox[0] = 0x63;
    sbox
};

fn xtime(b: u8) -> u8 {
    (b << 1) ^ if b & 0x80 != 0 { 0x1B } else { 0 }
}

/// AES-128 with an expanded key.
pub struct Aes128 {
    round_keys: [[u8; 16]; 11],
    /// GHASH key H = E(K, 0^128).
    h: u128,
}

impl Aes128 {
    pub fn new(key: &[u8; 16]) -> Aes128 {
        let mut w = [[0u8; 4]; 44];
        for i in 0..4 {
            w[i].copy_from_slice(&key[4 * i..4 * i + 4]);
        }
        let mut rcon = 1u8;
        for i in 4..44 {
            let mut t = w[i - 1];
            if i % 4 == 0 {
                t = [SBOX[t[1] as usize] ^ rcon, SBOX[t[2] as usize], SBOX[t[3] as usize], SBOX[t[0] as usize]];
                rcon = xtime(rcon);
            }
            for j in 0..4 {
                w[i][j] = w[i - 4][j] ^ t[j];
            }
        }
        let mut round_keys = [[0u8; 16]; 11];
        for r in 0..11 {
            for c in 0..4 {
                round_keys[r][4 * c..4 * c + 4].copy_from_slice(&w[4 * r + c]);
            }
        }
        let mut aes = Aes128 { round_keys, h: 0 };
        aes.h = u128::from_be_bytes(aes.encrypt_block(&[0; 16]));
        aes
    }

    pub fn encrypt_block(&self, input: &[u8; 16]) -> [u8; 16] {
        let mut s = *input;
        for (b, k) in s.iter_mut().zip(self.round_keys[0].iter()) {
            *b ^= k;
        }
        for round in 1..11 {
            for b in s.iter_mut() {
                *b = SBOX[*b as usize];
            }
            // ShiftRows (column-major state).
            let t = s;
            for c in 0..4 {
                for r in 0..4 {
                    s[4 * c + r] = t[4 * ((c + r) % 4) + r];
                }
            }
            if round != 10 {
                for c in 0..4 {
                    let a = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
                    let all = a[0] ^ a[1] ^ a[2] ^ a[3];
                    for r in 0..4 {
                        s[4 * c + r] = a[r] ^ all ^ xtime(a[r] ^ a[(r + 1) % 4]);
                    }
                }
            }
            for (b, k) in s.iter_mut().zip(self.round_keys[round].iter()) {
                *b ^= k;
            }
        }
        s
    }

    /// CTR mode with a 96-bit nonce and a 32-bit counter starting at `start`.
    fn ctr(&self, nonce: &[u8; 12], start: u32, data: &mut [u8]) {
        let mut block = [0u8; 16];
        block[..12].copy_from_slice(nonce);
        for (i, chunk) in data.chunks_mut(16).enumerate() {
            block[12..].copy_from_slice(&start.wrapping_add(i as u32).to_be_bytes());
            let ks = self.encrypt_block(&block);
            for (b, k) in chunk.iter_mut().zip(ks.iter()) {
                *b ^= k;
            }
        }
    }

    fn gcm_tag(&self, nonce: &[u8; 12], aad: &[u8], ct: &[u8]) -> [u8; 16] {
        let mut y = 0u128;
        for part in [aad, ct] {
            for chunk in part.chunks(16) {
                let mut b = [0u8; 16];
                b[..chunk.len()].copy_from_slice(chunk);
                y = gf_mul(y ^ u128::from_be_bytes(b), self.h);
            }
        }
        let lens = ((aad.len() as u128 * 8) << 64) | (ct.len() as u128 * 8);
        y = gf_mul(y ^ lens, self.h);
        let mut j0 = [0u8; 16];
        j0[..12].copy_from_slice(nonce);
        j0[15] = 1;
        (y ^ u128::from_be_bytes(self.encrypt_block(&j0))).to_be_bytes()
    }
}

/// Multiplication in GF(2^128) with GCM's bit order.
fn gf_mul(x: u128, y: u128) -> u128 {
    let r: u128 = 0xE1 << 120;
    let mut z = 0u128;
    let mut v = y;
    for i in 0..128 {
        if (x >> (127 - i)) & 1 == 1 {
            z ^= v;
        }
        v = if v & 1 == 1 { (v >> 1) ^ r } else { v >> 1 };
    }
    z
}
