//! X25519 key agreement (RFC 7748): field arithmetic modulo 2^255 - 19 in
//! five 51-bit limbs and the constant-time Montgomery ladder.

#[derive(Clone, Copy)]
struct Fe([u64; 5]);

const MASK: u64 = (1 << 51) - 1;

impl Fe {
    const ZERO: Fe = Fe([0; 5]);
    const ONE: Fe = Fe([1, 0, 0, 0, 0]);

    fn from_bytes(b: &[u8; 32]) -> Fe {
        let load = |i: usize| -> u64 {
            let mut x = [0u8; 8];
            let n = (32 - i).min(8);
            x[..n].copy_from_slice(&b[i..i + n]);
            u64::from_le_bytes(x)
        };
        Fe([
            load(0) & MASK,
            (load(6) >> 3) & MASK,
            (load(12) >> 6) & MASK,
            (load(19) >> 1) & MASK,
            (load(24) >> 12) & MASK, // bit 255 is ignored
        ])
    }

    fn to_bytes(self) -> [u8; 32] {
        let mut t = self.carry().0;
        // Subtract p if t >= p: compute t + 19 and look at bit 255.
        let mut q = (t[0] + 19) >> 51;
        q = (t[1] + q) >> 51;
        q = (t[2] + q) >> 51;
        q = (t[3] + q) >> 51;
        q = (t[4] + q) >> 51;
        t[0] += 19 * q;
        for i in 0..4 {
            t[i + 1] += t[i] >> 51;
            t[i] &= MASK;
        }
        t[4] &= MASK;
        let mut out = [0u8; 32];
        let words = [
            t[0] | t[1] << 51,
            t[1] >> 13 | t[2] << 38,
            t[2] >> 26 | t[3] << 25,
            t[3] >> 39 | t[4] << 12,
        ];
        for (i, w) in words.iter().enumerate() {
            out[8 * i..8 * i + 8].copy_from_slice(&w.to_le_bytes());
        }
        out
    }

    fn carry(self) -> Fe {
        let mut t = self.0;
        for _ in 0..2 {
            let c4 = t[4] >> 51;
            t[4] &= MASK;
            t[0] += c4 * 19;
            for i in 0..4 {
                t[i + 1] += t[i] >> 51;
                t[i] &= MASK;
            }
        }
        Fe(t)
    }

    fn add(self, o: Fe) -> Fe {
        let mut t = self.0;
        for i in 0..5 {
            t[i] += o.0[i];
        }
        Fe(t).carry()
    }

    fn sub(self, o: Fe) -> Fe {
        // Add 2p first so nothing goes negative.
        let p2 = [0xF_FFFF_FFFF_FFDA, 0xF_FFFF_FFFF_FFFE, 0xF_FFFF_FFFF_FFFE, 0xF_FFFF_FFFF_FFFE, 0xF_FFFF_FFFF_FFFE];
        let mut t = self.0;
        for i in 0..5 {
            t[i] = t[i] + p2[i] - o.0[i];
        }
        Fe(t).carry()
    }

    fn mul(self, o: Fe) -> Fe {
        let a = self.0;
        let b = o.0;
        let m = |x: u64, y: u64| x as u128 * y as u128;
        let b19 = [b[0], b[1] * 19, b[2] * 19, b[3] * 19, b[4] * 19];
        let r0 = m(a[0], b[0]) + m(a[1], b19[4]) + m(a[2], b19[3]) + m(a[3], b19[2]) + m(a[4], b19[1]);
        let r1 = m(a[0], b[1]) + m(a[1], b[0]) + m(a[2], b19[4]) + m(a[3], b19[3]) + m(a[4], b19[2]);
        let r2 = m(a[0], b[2]) + m(a[1], b[1]) + m(a[2], b[0]) + m(a[3], b19[4]) + m(a[4], b19[3]);
        let r3 = m(a[0], b[3]) + m(a[1], b[2]) + m(a[2], b[1]) + m(a[3], b[0]) + m(a[4], b19[4]);
        let r4 = m(a[0], b[4]) + m(a[1], b[3]) + m(a[2], b[2]) + m(a[3], b[1]) + m(a[4], b[0]);
        Self::reduce([r0, r1, r2, r3, r4])
    }

    fn reduce(r: [u128; 5]) -> Fe {
        let mut r = r;
        let mut out = [0u64; 5];
        for i in 0..4 {
            r[i + 1] += r[i] >> 51;
            out[i] = r[i] as u64 & MASK;
        }
        let c = (r[4] >> 51) as u64;
        out[4] = r[4] as u64 & MASK;
        out[0] += c * 19;
        Fe(out).carry()
    }

    fn square(self) -> Fe {
        self.mul(self)
    }

    fn mul121666(self) -> Fe {
        let mut r = [0u128; 5];
        for i in 0..5 {
            r[i] = self.0[i] as u128 * 121666;
        }
        Self::reduce(r)
    }

    /// self^(p-2) = 1/self.
    fn invert(self) -> Fe {
        // 2^255 - 21 as a square-and-multiply chain over the bits.
        let mut result = Fe::ONE;
        let exp: [u8; 32] = {
            let mut e = [0xFFu8; 32];
            e[0] = 0xEB;
            e[31] = 0x7F;
            e
        };
        for i in (0..255).rev() {
            result = result.square();
            if (exp[i / 8] >> (i % 8)) & 1 == 1 {
                result = result.mul(self);
            }
        }
        result
    }

    /// Swaps a and b when `swap` is 1, without branching on it.
    fn cswap(a: &mut Fe, b: &mut Fe, swap: u64) {
        let mask = 0u64.wrapping_sub(swap);
        for i in 0..5 {
            let t = mask & (a.0[i] ^ b.0[i]);
            a.0[i] ^= t;
            b.0[i] ^= t;
        }
    }
}

/// X25519(scalar, u): the shared secret or (with the base point) the public key.
pub fn x25519(scalar: &[u8; 32], u: &[u8; 32]) -> [u8; 32] {
    let mut k = *scalar;
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;
    let x1 = Fe::from_bytes(u);
    let (mut x2, mut z2, mut x3, mut z3) = (Fe::ONE, Fe::ZERO, x1, Fe::ONE);
    let mut swap = 0u64;
    for t in (0..255).rev() {
        let bit = ((k[t / 8] >> (t % 8)) & 1) as u64;
        swap ^= bit;
        Fe::cswap(&mut x2, &mut x3, swap);
        Fe::cswap(&mut z2, &mut z3, swap);
        swap = bit;
        let a = x2.add(z2);
        let aa = a.square();
        let b = x2.sub(z2);
        let bb = b.square();
        let e = aa.sub(bb);
        let c = x3.add(z3);
        let d = x3.sub(z3);
        let da = d.mul(a);
        let cb = c.mul(b);
        x3 = da.add(cb).square();
        z3 = x1.mul(da.sub(cb).square());
        x2 = aa.mul(bb);
        // E * (AA + 121665 E) = E * (BB + 121666 E)
        z2 = e.mul(bb.add(e.mul121666()));
    }
    Fe::cswap(&mut x2, &mut x3, swap);
    Fe::cswap(&mut z2, &mut z3, swap);
    x2.mul(z2.invert()).to_bytes()
}

pub const BASE_POINT: [u8; 32] = {
    let mut b = [0u8; 32];
    b[0] = 9;
    b
};

/// The public key for a private key.
pub fn public_key(private: &[u8; 32]) -> [u8; 32] {
    x25519(private, &BASE_POINT)
}
