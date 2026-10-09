//! Arbitrary-size unsigned integers for signature verification: values are
//! little-endian `u32` limbs, arithmetic modulo an odd number uses
//! Montgomery multiplication. Only public data passes through here (keys,
//! signatures), so nothing needs to be constant-time.

use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

pub type Limbs = Vec<u32>;

pub fn from_be(bytes: &[u8], limbs: usize) -> Limbs {
    let mut out = vec![0u32; limbs.max(bytes.len().div_ceil(4))];
    for (i, b) in bytes.iter().rev().enumerate() {
        out[i / 4] |= (*b as u32) << (8 * (i % 4));
    }
    out
}

pub fn to_be(a: &[u32], len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    for i in 0..len {
        let limb = a.get(i / 4).copied().unwrap_or(0);
        out[len - 1 - i] = (limb >> (8 * (i % 4))) as u8;
    }
    out
}

pub fn is_zero(a: &[u32]) -> bool {
    a.iter().all(|&x| x == 0)
}

pub fn cmp(a: &[u32], b: &[u32]) -> Ordering {
    let n = a.len().max(b.len());
    for i in (0..n).rev() {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x.cmp(&y);
        }
    }
    Ordering::Equal
}

/// a -= b (a >= b, same length or longer).
pub fn sub_in_place(a: &mut [u32], b: &[u32]) {
    let mut borrow = 0i64;
    for i in 0..a.len() {
        let v = a[i] as i64 - b.get(i).copied().unwrap_or(0) as i64 - borrow;
        a[i] = v as u32;
        borrow = (v < 0) as i64;
    }
}

/// a += b; returns the carry out.
fn add_in_place(a: &mut [u32], b: &[u32]) -> u32 {
    let mut carry = 0u64;
    for i in 0..a.len() {
        let v = a[i] as u64 + b.get(i).copied().unwrap_or(0) as u64 + carry;
        a[i] = v as u32;
        carry = v >> 32;
    }
    carry as u32
}

/// Arithmetic modulo an odd `n`.
#[derive(Clone)]
pub struct Modulus {
    pub n: Limbs,
    /// -n^-1 mod 2^32
    ninv: u32,
    /// R^2 mod n, R = 2^(32 k)
    rr: Limbs,
}

impl Modulus {
    pub fn new(n_be: &[u8]) -> Modulus {
        let mut n = from_be(n_be, 0);
        while n.len() > 1 && *n.last().unwrap() == 0 {
            n.pop();
        }
        let k = n.len();
        // Newton iteration for n^-1 mod 2^32.
        let mut inv: u32 = 1;
        for _ in 0..5 {
            inv = inv.wrapping_mul(2u32.wrapping_sub(n[0].wrapping_mul(inv)));
        }
        let ninv = inv.wrapping_neg();
        // R^2 mod n by doubling 1 (2 * 32 k) times.
        let mut r = vec![0u32; k];
        r[0] = 1;
        for _ in 0..64 * k {
            let carry = add_in_place_self(&mut r);
            if carry != 0 || cmp(&r, &n) != Ordering::Less {
                sub_in_place(&mut r, &n);
            }
        }
        Modulus { n, ninv, rr: r }
    }

    pub fn limbs(&self) -> usize {
        self.n.len()
    }

    pub fn bytes(&self) -> usize {
        let bits = 32 * self.n.len() - self.n.last().map_or(32, |l| l.leading_zeros() as usize);
        bits.div_ceil(8)
    }

    /// a * b * R^-1 mod n (CIOS).
    pub fn mont_mul(&self, a: &[u32], b: &[u32]) -> Limbs {
        let k = self.n.len();
        let mut t = vec![0u32; k + 2];
        for i in 0..k {
            let ai = a[i] as u64;
            let mut c = 0u64;
            for j in 0..k {
                let v = t[j] as u64 + ai * b[j] as u64 + c;
                t[j] = v as u32;
                c = v >> 32;
            }
            let v = t[k] as u64 + c;
            t[k] = v as u32;
            t[k + 1] = (v >> 32) as u32;
            let m = t[0].wrapping_mul(self.ninv) as u64;
            let v = t[0] as u64 + m * self.n[0] as u64;
            let mut c = v >> 32;
            for j in 1..k {
                let v = t[j] as u64 + m * self.n[j] as u64 + c;
                t[j - 1] = v as u32;
                c = v >> 32;
            }
            let v = t[k] as u64 + c;
            t[k - 1] = v as u32;
            t[k] = t[k + 1] + (v >> 32) as u32;
        }
        let mut r: Limbs = t[..k].to_vec();
        if t[k] != 0 || cmp(&r, &self.n) != Ordering::Less {
            sub_in_place(&mut r, &self.n);
        }
        r
    }

    /// Reduces any value (up to twice the modulus length) modulo n.
    pub fn reduce(&self, a: &[u32]) -> Limbs {
        let k = self.n.len();
        // a = hi * R + lo: (lo + hi * R) mod n via Montgomery: mont(x, R^2) = x R.
        let lo: Limbs = (0..k).map(|i| a.get(i).copied().unwrap_or(0)).collect();
        let hi: Limbs = (0..k).map(|i| a.get(k + i).copied().unwrap_or(0)).collect();
        assert!(a.len() <= 2 * k || a[2 * k..].iter().all(|&x| x == 0), "value too large to reduce");
        // lo mod n: lo < R; mont(lo, R^2) = lo R mod n; mont(that, 1) = lo mod n.
        let one = self.one_plain();
        let lo_r = self.mont_mul(&lo, &self.rr);
        let lo_m = self.mont_mul(&lo_r, &one);
        // mont(hi, R^2) = hi R mod n, the value of the high half.
        let hi_m = self.mont_mul(&hi, &self.rr);
        self.add(&lo_m, &hi_m)
    }

    fn one_plain(&self) -> Limbs {
        let mut one = vec![0u32; self.n.len()];
        one[0] = 1;
        one
    }

    pub fn to_mont(&self, a: &[u32]) -> Limbs {
        let r = self.reduce(a);
        self.mont_mul(&r, &self.rr)
    }

    pub fn from_mont(&self, a: &[u32]) -> Limbs {
        self.mont_mul(a, &self.one_plain())
    }

    /// Montgomery form of 1.
    pub fn mont_one(&self) -> Limbs {
        self.to_mont(&self.one_plain())
    }

    pub fn add(&self, a: &[u32], b: &[u32]) -> Limbs {
        let mut r: Limbs = a[..self.n.len()].to_vec();
        let carry = add_in_place(&mut r, b);
        if carry != 0 || cmp(&r, &self.n) != Ordering::Less {
            sub_in_place(&mut r, &self.n);
        }
        r
    }

    pub fn sub(&self, a: &[u32], b: &[u32]) -> Limbs {
        let mut r: Limbs = a[..self.n.len()].to_vec();
        if cmp(a, b) == Ordering::Less {
            add_in_place(&mut r, &self.n);
        }
        sub_in_place(&mut r, b);
        r
    }

    /// base^exp mod n (plain values in and out).
    pub fn pow(&self, base: &[u32], exp_be: &[u8]) -> Limbs {
        let b = self.to_mont(base);
        let mut acc = self.mont_one();
        for byte in exp_be {
            for bit in (0..8).rev() {
                acc = self.mont_mul(&acc, &acc);
                if (byte >> bit) & 1 == 1 {
                    acc = self.mont_mul(&acc, &b);
                }
            }
        }
        self.from_mont(&acc)
    }

    /// 1/a mod n for a prime n (Fermat), plain values.
    pub fn inverse_prime(&self, a: &[u32]) -> Limbs {
        let mut e = self.n.clone();
        let two = [2u32];
        sub_in_place(&mut e, &two);
        self.pow(a, &to_be(&e, 4 * self.n.len()))
    }
}

fn add_in_place_self(a: &mut [u32]) -> u32 {
    let mut carry = 0u32;
    for x in a.iter_mut() {
        let v = ((*x as u64) << 1) | carry as u64;
        *x = v as u32;
        carry = (v >> 32) as u32;
    }
    carry
}
