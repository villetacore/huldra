//! Signature verification: RSA (PKCS #1 v1.5 and PSS, RFC 8017) and ECDSA
//! over P-256 and P-384 (FIPS 186-4), as certificates and TLS 1.3 use them.

use crate::bigint::{self, Limbs, Modulus};
use crate::sha::{Hash, Sha1, Sha256, Sha384, Sha512};
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashAlg {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl HashAlg {
    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            HashAlg::Sha1 => Sha1::digest(data),
            HashAlg::Sha256 => Sha256::digest(data),
            HashAlg::Sha384 => Sha384::digest(data),
            HashAlg::Sha512 => Sha512::digest(data),
        }
    }

    pub fn len(self) -> usize {
        match self {
            HashAlg::Sha1 => 20,
            HashAlg::Sha256 => 32,
            HashAlg::Sha384 => 48,
            HashAlg::Sha512 => 64,
        }
    }

    /// DER of DigestInfo up to the digest (PKCS #1 v1.5).
    fn digest_info_prefix(self) -> &'static [u8] {
        match self {
            HashAlg::Sha1 => &[0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14],
            HashAlg::Sha256 => &[0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20],
            HashAlg::Sha384 => &[0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02, 0x05, 0x00, 0x04, 0x30],
            HashAlg::Sha512 => &[0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05, 0x00, 0x04, 0x40],
        }
    }
}

// ----------------------------------------------------------------------- RSA

pub struct RsaPublicKey {
    pub n: Vec<u8>,
    pub e: Vec<u8>,
}

impl RsaPublicKey {
    /// s^e mod n as a big-endian string of the modulus length.
    fn encrypt(&self, sig: &[u8]) -> Option<Vec<u8>> {
        let m = Modulus::new(&self.n);
        let k = m.bytes();
        if sig.len() != k || m.n[0] & 1 == 0 {
            return None;
        }
        let s = bigint::from_be(sig, m.limbs());
        if bigint::cmp(&s, &m.n) != Ordering::Less {
            return None;
        }
        Some(bigint::to_be(&m.pow(&s, &self.e), k))
    }

    pub fn verify_pkcs1(&self, hash: HashAlg, message: &[u8], sig: &[u8]) -> bool {
        let Some(em) = self.encrypt(sig) else { return false };
        let prefix = hash.digest_info_prefix();
        let digest = hash.digest(message);
        let t_len = prefix.len() + digest.len();
        if em.len() < t_len + 11 {
            return false;
        }
        let mut expect = vec![0xFFu8; em.len()];
        expect[0] = 0;
        expect[1] = 1;
        let start = em.len() - t_len;
        expect[start - 1] = 0;
        expect[start..start + prefix.len()].copy_from_slice(prefix);
        expect[start + prefix.len()..].copy_from_slice(&digest);
        crate::ct_eq(&em, &expect)
    }

    /// RSASSA-PSS with MGF1 of the same hash and a salt as long as the hash.
    pub fn verify_pss(&self, hash: HashAlg, message: &[u8], sig: &[u8]) -> bool {
        let Some(em_full) = self.encrypt(sig) else { return false };
        let mod_bits = {
            let n = bigint::from_be(&self.n, 0);
            let top = n.iter().rposition(|&x| x != 0).unwrap_or(0);
            32 * top + 32 - n[top].leading_zeros() as usize
        };
        let em_bits = mod_bits - 1;
        let em_len = em_bits.div_ceil(8);
        // em_full has the modulus length; EM is its last em_len bytes.
        if em_full.len() > em_len && em_full[..em_full.len() - em_len].iter().any(|&b| b != 0) {
            return false;
        }
        let em = &em_full[em_full.len() - em_len..];
        let h_len = hash.len();
        let s_len = h_len;
        if em_len < h_len + s_len + 2 || em[em_len - 1] != 0xBC {
            return false;
        }
        let (masked_db, h) = em[..em_len - 1].split_at(em_len - h_len - 1);
        let zero_bits = 8 * em_len - em_bits;
        if zero_bits > 0 && masked_db[0] >> (8 - zero_bits) != 0 {
            return false;
        }
        let mask = mgf1(hash, h, masked_db.len());
        let mut db: Vec<u8> = masked_db.iter().zip(mask.iter()).map(|(a, b)| a ^ b).collect();
        if zero_bits > 0 {
            db[0] &= 0xFF >> zero_bits;
        }
        let ps_len = em_len - h_len - s_len - 2;
        if db[..ps_len].iter().any(|&b| b != 0) || db[ps_len] != 1 {
            return false;
        }
        let salt = &db[db.len() - s_len..];
        let mut m_prime = vec![0u8; 8];
        m_prime.extend_from_slice(&hash.digest(message));
        m_prime.extend_from_slice(salt);
        crate::ct_eq(&hash.digest(&m_prime), h)
    }
}

fn mgf1(hash: HashAlg, seed: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 64);
    let mut counter = 0u32;
    while out.len() < len {
        let mut input = seed.to_vec();
        input.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&hash.digest(&input));
        counter += 1;
    }
    out.truncate(len);
    out
}

// --------------------------------------------------------------------- ECDSA

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    P256,
    P384,
}

struct CurveParams {
    p: &'static str,
    n: &'static str,
    gx: &'static str,
    gy: &'static str,
}

impl Curve {
    fn params(self) -> CurveParams {
        match self {
            Curve::P256 => CurveParams {
                p: "ffffffff00000001000000000000000000000000ffffffffffffffffffffffff",
                n: "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
                gx: "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
                gy: "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
            },
            Curve::P384 => CurveParams {
                p: "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffeffffffff0000000000000000ffffffff",
                n: "ffffffffffffffffffffffffffffffffffffffffffffffffc7634d81f4372ddf581a0db248b0a77aecec196accc52973",
                gx: "aa87ca22be8b05378eb1c71ef320ad746e1d3b628ba79b9859f741e082542a385502f25dbf55296c3a545e3872760ab7",
                gy: "3617de4a96262c6f5d9e98bf9292dc29f8f41dbd289a147ce9da3113b5f0b8c00a60b1ce1d7e819d7a431d7c90ea0e5f",
            },
        }
    }

    pub fn bytes(self) -> usize {
        match self {
            Curve::P256 => 32,
            Curve::P384 => 48,
        }
    }
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// A point in Jacobian coordinates (Montgomery form); z = 0 is infinity.
#[derive(Clone)]
struct Point {
    x: Limbs,
    y: Limbs,
    z: Limbs,
}

struct Field {
    m: Modulus,
}

impl Field {
    fn mul(&self, a: &[u32], b: &[u32]) -> Limbs {
        self.m.mont_mul(a, b)
    }
    fn sqr(&self, a: &[u32]) -> Limbs {
        self.m.mont_mul(a, a)
    }
    fn add(&self, a: &[u32], b: &[u32]) -> Limbs {
        self.m.add(a, b)
    }
    fn sub(&self, a: &[u32], b: &[u32]) -> Limbs {
        self.m.sub(a, b)
    }

    fn infinity(&self) -> Point {
        let k = self.m.limbs();
        Point { x: self.m.mont_one(), y: self.m.mont_one(), z: vec![0; k] }
    }

    /// Doubling for a = -3 ("dbl-2001-b").
    fn double(&self, p: &Point) -> Point {
        if bigint::is_zero(&p.z) || bigint::is_zero(&p.y) {
            return self.infinity();
        }
        let delta = self.sqr(&p.z);
        let gamma = self.sqr(&p.y);
        let beta = self.mul(&p.x, &gamma);
        let t = self.mul(&self.sub(&p.x, &delta), &self.add(&p.x, &delta));
        let alpha = self.add(&self.add(&t, &t), &t);
        let beta4 = { let b2 = self.add(&beta, &beta); self.add(&b2, &b2) };
        let x3 = self.sub(&self.sqr(&alpha), &self.add(&beta4, &beta4));
        let yz = self.add(&p.y, &p.z);
        let z3 = self.sub(&self.sub(&self.sqr(&yz), &gamma), &delta);
        let g2 = self.sqr(&gamma);
        let g8 = { let a = self.add(&g2, &g2); let b = self.add(&a, &a); self.add(&b, &b) };
        let y3 = self.sub(&self.mul(&alpha, &self.sub(&beta4, &x3)), &g8);
        Point { x: x3, y: y3, z: z3 }
    }

    /// General addition ("add-2007-bl" without the shortcuts).
    fn add_points(&self, p: &Point, q: &Point) -> Point {
        if bigint::is_zero(&p.z) {
            return q.clone();
        }
        if bigint::is_zero(&q.z) {
            return p.clone();
        }
        let z1z1 = self.sqr(&p.z);
        let z2z2 = self.sqr(&q.z);
        let u1 = self.mul(&p.x, &z2z2);
        let u2 = self.mul(&q.x, &z1z1);
        let s1 = self.mul(&self.mul(&p.y, &q.z), &z2z2);
        let s2 = self.mul(&self.mul(&q.y, &p.z), &z1z1);
        let h = self.sub(&u2, &u1);
        let r = self.sub(&s2, &s1);
        if bigint::is_zero(&h) {
            return if bigint::is_zero(&r) { self.double(p) } else { self.infinity() };
        }
        let hh = self.sqr(&h);
        let hhh = self.mul(&h, &hh);
        let v = self.mul(&u1, &hh);
        let x3 = self.sub(&self.sub(&self.sqr(&r), &hhh), &self.add(&v, &v));
        let y3 = self.sub(&self.mul(&r, &self.sub(&v, &x3)), &self.mul(&s1, &hhh));
        let z3 = self.mul(&self.mul(&p.z, &q.z), &h);
        Point { x: x3, y: y3, z: z3 }
    }

    /// u1 G + u2 Q (Shamir's trick), scalars as big-endian bytes.
    fn double_mul(&self, g: &Point, u1: &[u8], q: &Point, u2: &[u8]) -> Point {
        let gq = self.add_points(g, q);
        let mut acc = self.infinity();
        for i in 0..u1.len() * 8 {
            acc = self.double(&acc);
            let b1 = (u1[i / 8] >> (7 - i % 8)) & 1;
            let b2 = (u2[i / 8] >> (7 - i % 8)) & 1;
            acc = match (b1, b2) {
                (1, 1) => self.add_points(&acc, &gq),
                (1, 0) => self.add_points(&acc, g),
                (0, 1) => self.add_points(&acc, q),
                _ => acc,
            };
        }
        acc
    }

    /// Affine x of a point (plain value).
    fn affine_x(&self, p: &Point) -> Limbs {
        let z = self.m.from_mont(&p.z);
        let zinv = self.m.to_mont(&self.m.inverse_prime(&z));
        let zinv2 = self.sqr(&zinv);
        self.m.from_mont(&self.mul(&p.x, &zinv2))
    }
}

/// Verifies an ECDSA signature given as (r, s); `point` is the uncompressed
/// public key (0x04 || x || y), `digest` the message hash.
pub fn ecdsa_verify(curve: Curve, point: &[u8], digest: &[u8], r: &[u8], s: &[u8]) -> bool {
    let len = curve.bytes();
    if point.len() != 1 + 2 * len || point[0] != 4 {
        return false;
    }
    let prm = curve.params();
    let field = Field { m: Modulus::new(&hex(prm.p)) };
    let order = Modulus::new(&hex(prm.n));
    let k = order.limbs();
    let strip = |v: &[u8]| -> Vec<u8> { v.iter().skip_while(|&&b| b == 0).copied().collect() };
    let (r, s) = (strip(r), strip(s));
    if r.is_empty() || s.is_empty() || r.len() > len || s.len() > len {
        return false;
    }
    let r_l = bigint::from_be(&r, k);
    let s_l = bigint::from_be(&s, k);
    if bigint::cmp(&r_l, &order.n) != Ordering::Less || bigint::cmp(&s_l, &order.n) != Ordering::Less {
        return false;
    }
    // e: the leftmost bits of the digest, as many as the order has.
    let mut e_bytes = digest[..digest.len().min(len)].to_vec();
    while e_bytes.len() < len {
        e_bytes.insert(0, 0);
    }
    let e = order.reduce(&bigint::from_be(&e_bytes, k));
    let w = order.inverse_prime(&s_l);
    let u1 = order.from_mont(&order.mont_mul(&order.to_mont(&e), &order.to_mont(&w)));
    let u2 = order.from_mont(&order.mont_mul(&order.to_mont(&r_l), &order.to_mont(&w)));
    let fk = field.m.limbs();
    let mk = |v: &[u8]| field.m.to_mont(&bigint::from_be(v, fk));
    let g = Point { x: mk(&hex(prm.gx)), y: mk(&hex(prm.gy)), z: field.m.mont_one() };
    let q = Point { x: mk(&point[1..1 + len]), y: mk(&point[1 + len..]), z: field.m.mont_one() };
    let x = field.double_mul(&g, &bigint::to_be(&u1, len), &q, &bigint::to_be(&u2, len));
    if bigint::is_zero(&x.z) {
        return false;
    }
    let xr = order.reduce(&field.affine_x(&x));
    bigint::cmp(&xr, &r_l) == Ordering::Equal
}
