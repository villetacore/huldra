//! Methods and constants of the integer and floating-point types.

use crate::intrinsics;
use crate::option::Option::{self, None, Some};

pub use crate::str::{IntErrorKind, ParseFloatError, ParseIntError};

mod libm {
    extern "C" {
        pub fn sqrt(x: f64) -> f64;
        pub fn pow(x: f64, y: f64) -> f64;
        pub fn floor(x: f64) -> f64;
        pub fn ceil(x: f64) -> f64;
        pub fn round(x: f64) -> f64;
        pub fn trunc(x: f64) -> f64;
        pub fn sin(x: f64) -> f64;
        pub fn cos(x: f64) -> f64;
        pub fn tan(x: f64) -> f64;
        pub fn asin(x: f64) -> f64;
        pub fn acos(x: f64) -> f64;
        pub fn atan(x: f64) -> f64;
        pub fn atan2(y: f64, x: f64) -> f64;
        pub fn sinh(x: f64) -> f64;
        pub fn cosh(x: f64) -> f64;
        pub fn tanh(x: f64) -> f64;
        pub fn exp(x: f64) -> f64;
        pub fn exp2(x: f64) -> f64;
        pub fn expm1(x: f64) -> f64;
        pub fn log(x: f64) -> f64;
        pub fn log2(x: f64) -> f64;
        pub fn log10(x: f64) -> f64;
        pub fn log1p(x: f64) -> f64;
        pub fn hypot(x: f64, y: f64) -> f64;
        pub fn cbrt(x: f64) -> f64;
        pub fn fmod(x: f64, y: f64) -> f64;
        pub fn rint(x: f64) -> f64;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TryFromIntError(());

impl crate::fmt::Display for TryFromIntError {
    fn fmt(&self, f: &mut crate::fmt::Formatter) -> crate::fmt::Result {
        f.pad("out of range integral type conversion attempted")
    }
}

macro_rules! int_common {
    ($t:ty, $u:ty, $bits:expr, $min:expr, $max:expr) => {
        impl $t {
            pub const MIN: $t = $min;
            pub const MAX: $t = $max;
            pub const BITS: u32 = $bits;

            pub fn min_value() -> $t {
                $min
            }
            pub fn max_value() -> $t {
                $max
            }
            pub fn from_str_radix(s: &str, radix: u32) -> Result<$t, ParseIntError> {
                match crate::str::parse_int(s, radix, $min != 0, $min as i64, $max as u64) {
                    Ok(v) => Ok(v as $t),
                    Err(e) => Err(e),
                }
            }
            pub fn pow(self, mut exp: u32) -> $t {
                let mut base = self;
                let mut acc: $t = 1;
                while exp > 1 {
                    if exp & 1 == 1 {
                        acc = acc * base;
                    }
                    exp /= 2;
                    base = base * base;
                }
                if exp == 1 {
                    acc = acc * base;
                }
                acc
            }
            pub fn checked_add(self, rhs: $t) -> Option<$t> {
                let (r, o) = intrinsics::overflowing_add(self, rhs);
                if o { None } else { Some(r) }
            }
            pub fn checked_sub(self, rhs: $t) -> Option<$t> {
                let (r, o) = intrinsics::overflowing_sub(self, rhs);
                if o { None } else { Some(r) }
            }
            pub fn checked_mul(self, rhs: $t) -> Option<$t> {
                let (r, o) = intrinsics::overflowing_mul(self, rhs);
                if o { None } else { Some(r) }
            }
            pub fn checked_div(self, rhs: $t) -> Option<$t> {
                if rhs == 0 || ($min != 0 && self == $min && rhs == (0 as $t).wrapping_sub(1)) {
                    None
                } else {
                    Some(self / rhs)
                }
            }
            pub fn checked_rem(self, rhs: $t) -> Option<$t> {
                if rhs == 0 || ($min != 0 && self == $min && rhs == (0 as $t).wrapping_sub(1)) {
                    None
                } else {
                    Some(self % rhs)
                }
            }
            pub fn checked_pow(self, mut exp: u32) -> Option<$t> {
                let mut base = self;
                let mut acc: $t = 1;
                while exp > 0 {
                    if exp & 1 == 1 {
                        acc = acc.checked_mul(base)?;
                    }
                    exp /= 2;
                    if exp > 0 {
                        base = base.checked_mul(base)?;
                    }
                }
                Some(acc)
            }
            pub fn checked_neg(self) -> Option<$t> {
                (0 as $t).checked_sub(self)
            }
            pub fn checked_shl(self, rhs: u32) -> Option<$t> {
                if rhs < $bits { Some(self << rhs) } else { None }
            }
            pub fn checked_shr(self, rhs: u32) -> Option<$t> {
                if rhs < $bits { Some(self >> rhs) } else { None }
            }
            pub fn wrapping_add(self, rhs: $t) -> $t {
                intrinsics::wrapping_add(self, rhs)
            }
            pub fn wrapping_sub(self, rhs: $t) -> $t {
                intrinsics::wrapping_sub(self, rhs)
            }
            pub fn wrapping_mul(self, rhs: $t) -> $t {
                intrinsics::wrapping_mul(self, rhs)
            }
            pub fn wrapping_neg(self) -> $t {
                intrinsics::wrapping_sub(0, self)
            }
            pub fn wrapping_div(self, rhs: $t) -> $t {
                match self.checked_div(rhs) {
                    Some(v) => v,
                    None => {
                        if rhs == 0 {
                            self / rhs
                        } else {
                            self
                        }
                    }
                }
            }
            pub fn wrapping_rem(self, rhs: $t) -> $t {
                match self.checked_rem(rhs) {
                    Some(v) => v,
                    None => {
                        if rhs == 0 {
                            self % rhs
                        } else {
                            0
                        }
                    }
                }
            }
            pub fn wrapping_shl(self, rhs: u32) -> $t {
                self << (rhs % $bits)
            }
            pub fn wrapping_shr(self, rhs: u32) -> $t {
                self >> (rhs % $bits)
            }
            pub fn wrapping_pow(self, mut exp: u32) -> $t {
                let mut base = self;
                let mut acc: $t = 1;
                while exp > 0 {
                    if exp & 1 == 1 {
                        acc = acc.wrapping_mul(base);
                    }
                    exp /= 2;
                    base = base.wrapping_mul(base);
                }
                acc
            }
            pub fn overflowing_add(self, rhs: $t) -> ($t, bool) {
                intrinsics::overflowing_add(self, rhs)
            }
            pub fn overflowing_sub(self, rhs: $t) -> ($t, bool) {
                intrinsics::overflowing_sub(self, rhs)
            }
            pub fn overflowing_mul(self, rhs: $t) -> ($t, bool) {
                intrinsics::overflowing_mul(self, rhs)
            }
            pub fn saturating_add(self, rhs: $t) -> $t {
                match self.checked_add(rhs) {
                    Some(v) => v,
                    None => {
                        if $min != 0 && rhs < (0 as $t) {
                            $min
                        } else {
                            $max
                        }
                    }
                }
            }
            pub fn saturating_sub(self, rhs: $t) -> $t {
                match self.checked_sub(rhs) {
                    Some(v) => v,
                    None => {
                        if $min != 0 && rhs < (0 as $t) {
                            $max
                        } else {
                            $min
                        }
                    }
                }
            }
            pub fn saturating_mul(self, rhs: $t) -> $t {
                match self.checked_mul(rhs) {
                    Some(v) => v,
                    None => {
                        if ($min != 0) && ((self < (0 as $t)) != (rhs < (0 as $t))) {
                            $min
                        } else {
                            $max
                        }
                    }
                }
            }
            pub fn saturating_pow(self, exp: u32) -> $t {
                match self.checked_pow(exp) {
                    Some(v) => v,
                    None => {
                        if $min != 0 && self < (0 as $t) && exp % 2 == 1 {
                            $min
                        } else {
                            $max
                        }
                    }
                }
            }
            pub fn count_ones(self) -> u32 {
                let mut v = self as $u;
                let mut n = 0;
                while v != 0 {
                    n += (v & 1) as u32;
                    v >>= 1;
                }
                n
            }
            pub fn count_zeros(self) -> u32 {
                $bits - self.count_ones()
            }
            pub fn leading_zeros(self) -> u32 {
                let v = self as $u;
                let mut n = 0;
                let mut bit = $bits;
                while bit > 0 {
                    bit -= 1;
                    if (v >> bit) & 1 == 1 {
                        break;
                    }
                    n += 1;
                }
                n
            }
            pub fn trailing_zeros(self) -> u32 {
                let v = self as $u;
                if v == 0 {
                    return $bits;
                }
                let mut n = 0;
                while (v >> n) & 1 == 0 {
                    n += 1;
                }
                n
            }
            pub fn leading_ones(self) -> u32 {
                (!self).leading_zeros()
            }
            pub fn trailing_ones(self) -> u32 {
                (!self).trailing_zeros()
            }
            pub fn rotate_left(self, n: u32) -> $t {
                let v = self as $u;
                let n = n % $bits;
                if n == 0 {
                    return self;
                }
                ((v << n) | (v >> ($bits - n))) as $t
            }
            pub fn rotate_right(self, n: u32) -> $t {
                let v = self as $u;
                let n = n % $bits;
                if n == 0 {
                    return self;
                }
                ((v >> n) | (v << ($bits - n))) as $t
            }
            pub fn swap_bytes(self) -> $t {
                let mut v = self as $u;
                let mut r: $u = 0;
                let mut i = 0;
                while i < $bits / 8 {
                    r = (r << 8) | (v & 0xff);
                    v >>= 8;
                    i += 1;
                }
                r as $t
            }
            pub fn reverse_bits(self) -> $t {
                let mut v = self as $u;
                let mut r: $u = 0;
                let mut i = 0;
                while i < $bits {
                    r = (r << 1) | (v & 1);
                    v >>= 1;
                    i += 1;
                }
                r as $t
            }
            pub fn to_be(self) -> $t {
                self.swap_bytes()
            }
            pub fn to_le(self) -> $t {
                self
            }
            pub fn from_be(x: $t) -> $t {
                x.swap_bytes()
            }
            pub fn from_le(x: $t) -> $t {
                x
            }
            pub fn ilog2(self) -> u32 {
                if self <= 0 {
                    panic!("argument of integer logarithm must be positive");
                }
                $bits - 1 - self.leading_zeros()
            }
            pub fn ilog10(self) -> u32 {
                if self <= 0 {
                    panic!("argument of integer logarithm must be positive");
                }
                let mut n = 0;
                let mut v = self;
                while v >= 10 {
                    v /= 10;
                    n += 1;
                }
                n
            }
            pub fn ilog(self, base: $t) -> u32 {
                if self <= 0 || base < 2 {
                    panic!("argument of integer logarithm must be positive");
                }
                let mut n = 0;
                let mut v = self;
                while v >= base {
                    v /= base;
                    n += 1;
                }
                n
            }
            pub fn isqrt(self) -> $t {
                if self < (0 as $t) {
                    panic!("argument of integer square root cannot be negative");
                }
                let mut r = unsafe { libm::sqrt(self as f64) } as $t;
                while r > 0 && r.checked_mul(r).map_or(true, |s| s > self) {
                    r -= 1;
                }
                while (r + 1).checked_mul(r + 1).map_or(false, |s| s <= self) {
                    r += 1;
                }
                r
            }
            pub fn abs_diff(self, other: $t) -> $u {
                if self > other {
                    (self as $u).wrapping_sub(other as $u)
                } else {
                    (other as $u).wrapping_sub(self as $u)
                }
            }
            pub fn div_euclid(self, rhs: $t) -> $t {
                let q = self / rhs;
                if self % rhs < (0 as $t) {
                    if rhs > (0 as $t) { q - 1 } else { q + 1 }
                } else {
                    q
                }
            }
            pub fn rem_euclid(self, rhs: $t) -> $t {
                let r = self % rhs;
                if r < (0 as $t) {
                    if rhs < (0 as $t) {
                        r.wrapping_sub(rhs)
                    } else {
                        r.wrapping_add(rhs)
                    }
                } else {
                    r
                }
            }
            pub fn div_ceil(self, rhs: $t) -> $t {
                let d = self / rhs;
                let r = self % rhs;
                if (r > (0 as $t) && rhs > (0 as $t)) || (r < (0 as $t) && rhs < (0 as $t)) {
                    d + 1
                } else {
                    d
                }
            }
            pub fn to_le_bytes(self) -> [u8; $bits / 8] {
                let mut b = [0u8; $bits / 8];
                let mut v = self as $u;
                let mut i = 0;
                while i < $bits / 8 {
                    b[i] = (v & 0xff) as u8;
                    v >>= 8;
                    i += 1;
                }
                b
            }
            pub fn to_be_bytes(self) -> [u8; $bits / 8] {
                let mut b = self.to_le_bytes();
                b.reverse();
                b
            }
            pub fn from_le_bytes(b: [u8; $bits / 8]) -> $t {
                let mut v: $u = 0;
                let mut i = $bits / 8;
                while i > 0 {
                    i -= 1;
                    v = (v << 8) | (b[i] as $u);
                }
                v as $t
            }
            pub fn from_be_bytes(b: [u8; $bits / 8]) -> $t {
                let mut v: $u = 0;
                let mut i = 0;
                while i < $bits / 8 {
                    v = (v << 8) | (b[i] as $u);
                    i += 1;
                }
                v as $t
            }
        }

        impl Default for $t {
            fn default() -> $t {
                0
            }
        }
    };
}

macro_rules! int_signed {
    ($($t:ty, $u:ty, $bits:expr, $min:expr, $max:expr;)*) => {$(
        int_common!($t, $u, $bits, $min, $max);
        impl $t {
            pub fn abs(self) -> $t {
                if self < 0 { -self } else { self }
            }
            pub fn unsigned_abs(self) -> $u {
                if self < 0 { (self as $u).wrapping_neg() } else { self as $u }
            }
            pub fn checked_abs(self) -> Option<$t> {
                if self == $min { None } else { Some(self.abs()) }
            }
            pub fn wrapping_abs(self) -> $t {
                if self < 0 { self.wrapping_neg() } else { self }
            }
            pub fn saturating_abs(self) -> $t {
                if self == $min { $max } else { self.abs() }
            }
            pub fn signum(self) -> $t {
                if self < 0 {
                    -1
                } else if self > 0 {
                    1
                } else {
                    0
                }
            }
            pub fn is_positive(self) -> bool {
                self > 0
            }
            pub fn is_negative(self) -> bool {
                self < 0
            }
        }
    )*};
}

macro_rules! int_unsigned {
    ($($t:ty, $s:ty, $bits:expr, $max:expr;)*) => {$(
        int_common!($t, $t, $bits, 0, $max);
        impl $t {
            pub fn is_power_of_two(self) -> bool {
                self != 0 && (self & (self - 1)) == 0
            }
            pub fn next_power_of_two(self) -> $t {
                let mut p: $t = 1;
                while p < self {
                    p = p * 2;
                }
                p
            }
            pub fn checked_next_power_of_two(self) -> Option<$t> {
                let mut p: $t = 1;
                while p < self {
                    p = p.checked_mul(2)?;
                }
                Some(p)
            }
            pub fn checked_sub_signed(self, rhs: $s) -> Option<$t> {
                if rhs >= 0 {
                    self.checked_sub(rhs as $t)
                } else {
                    self.checked_add(rhs.unsigned_abs())
                }
            }
            pub fn checked_add_signed(self, rhs: $s) -> Option<$t> {
                if rhs >= 0 {
                    self.checked_add(rhs as $t)
                } else {
                    self.checked_sub(rhs.unsigned_abs())
                }
            }
            pub fn wrapping_add_signed(self, rhs: $s) -> $t {
                self.wrapping_add(rhs as $t)
            }
            pub fn saturating_add_signed(self, rhs: $s) -> $t {
                if rhs >= 0 {
                    self.saturating_add(rhs as $t)
                } else {
                    self.saturating_sub(rhs.unsigned_abs())
                }
            }
            pub fn is_multiple_of(self, rhs: $t) -> bool {
                if rhs == 0 { self == 0 } else { self % rhs == 0 }
            }
        }
    )*};
}

int_signed! {
    i8, u8, 8, -128, 127;
    i16, u16, 16, -32768, 32767;
    i32, u32, 32, -2147483648, 2147483647;
    i64, u64, 64, -9223372036854775808, 9223372036854775807;
    isize, usize, 64, -9223372036854775808, 9223372036854775807;
}

int_unsigned! {
    u8, i8, 8, 255;
    u16, i16, 16, 65535;
    u32, i32, 32, 4294967295;
    u64, i64, 64, 18446744073709551615;
    usize, isize, 64, 18446744073709551615;
}

impl u8 {
    pub fn is_ascii(&self) -> bool {
        *self < 128
    }
    pub fn is_ascii_alphabetic(&self) -> bool {
        (*self >= b'a' && *self <= b'z') || (*self >= b'A' && *self <= b'Z')
    }
    pub fn is_ascii_uppercase(&self) -> bool {
        *self >= b'A' && *self <= b'Z'
    }
    pub fn is_ascii_lowercase(&self) -> bool {
        *self >= b'a' && *self <= b'z'
    }
    pub fn is_ascii_digit(&self) -> bool {
        *self >= b'0' && *self <= b'9'
    }
    pub fn is_ascii_hexdigit(&self) -> bool {
        self.is_ascii_digit() || (*self >= b'a' && *self <= b'f') || (*self >= b'A' && *self <= b'F')
    }
    pub fn is_ascii_alphanumeric(&self) -> bool {
        self.is_ascii_alphabetic() || self.is_ascii_digit()
    }
    pub fn is_ascii_whitespace(&self) -> bool {
        *self == b' ' || *self == b'\t' || *self == b'\n' || *self == b'\r' || *self == 0x0c
    }
    pub fn is_ascii_punctuation(&self) -> bool {
        (*self >= 33 && *self <= 47) || (*self >= 58 && *self <= 64) || (*self >= 91 && *self <= 96) || (*self >= 123 && *self <= 126)
    }
    pub fn is_ascii_graphic(&self) -> bool {
        *self >= 33 && *self <= 126
    }
    pub fn is_ascii_control(&self) -> bool {
        *self < 32 || *self == 127
    }
    pub fn to_ascii_uppercase(&self) -> u8 {
        if self.is_ascii_lowercase() { *self - 32 } else { *self }
    }
    pub fn to_ascii_lowercase(&self) -> u8 {
        if self.is_ascii_uppercase() { *self + 32 } else { *self }
    }
    pub fn make_ascii_uppercase(&mut self) {
        *self = self.to_ascii_uppercase();
    }
    pub fn make_ascii_lowercase(&mut self) {
        *self = self.to_ascii_lowercase();
    }
    pub fn eq_ignore_ascii_case(&self, other: &u8) -> bool {
        self.to_ascii_lowercase() == other.to_ascii_lowercase()
    }
}

macro_rules! try_from_int {
    ($($from:ty => $($to:ty),*;)*) => {$($(
        impl TryFrom<$from> for $to {
            type Error = TryFromIntError;
            fn try_from(v: $from) -> Result<$to, TryFromIntError> {
                let back = v as $to;
                if back as $from == v && ((back < (0 as $to)) == (v < (0 as $from))) {
                    Ok(back)
                } else {
                    Err(TryFromIntError(()))
                }
            }
        }
    )*)*};
}
try_from_int! {
    i8 => u8, u16, u32, u64, usize;
    i16 => i8, u8, u16, u32, u64, usize;
    i32 => i8, i16, u8, u16, u32, u64, usize;
    i64 => i8, i16, i32, isize, u8, u16, u32, u64, usize;
    isize => i8, i16, i32, i64, u8, u16, u32, u64, usize;
    u8 => i8;
    u16 => i8, i16, u8;
    u32 => i8, i16, i32, u8, u16, usize;
    u64 => i8, i16, i32, i64, isize, u8, u16, u32, usize;
    usize => i8, i16, i32, i64, isize, u8, u16, u32, u64;
}

macro_rules! from_int {
    ($($from:ty => $($to:ty),*;)*) => {$($(
        impl From<$from> for $to {
            fn from(v: $from) -> $to {
                v as $to
            }
        }
    )*)*};
}
from_int! {
    u8 => u16, u32, u64, usize, i16, i32, i64, isize, f32, f64;
    u16 => u32, u64, usize, i32, i64, f32, f64;
    u32 => u64, i64, f64;
    i8 => i16, i32, i64, isize, f32, f64;
    i16 => i32, i64, isize, f32, f64;
    i32 => i64, f64;
    f32 => f64;
    bool => u8, u16, u32, u64, usize, i8, i16, i32, i64, isize;
}

impl From<char> for u32 {
    fn from(c: char) -> u32 {
        c as u32
    }
}

impl From<char> for u64 {
    fn from(c: char) -> u64 {
        c as u64
    }
}

impl From<u8> for char {
    fn from(b: u8) -> char {
        b as char
    }
}

impl TryFrom<u32> for char {
    type Error = crate::char::CharTryFromError;
    fn try_from(v: u32) -> Result<char, crate::char::CharTryFromError> {
        match crate::char::from_u32(v) {
            Some(c) => Ok(c),
            None => Err(crate::char::CharTryFromError(())),
        }
    }
}

macro_rules! float_impl {
    ($($t:ty, $bits:ty, $is32:expr;)*) => {$(
        impl $t {
            pub fn is_nan(self) -> bool {
                self != self
            }
            pub fn is_infinite(self) -> bool {
                self == 1.0 / 0.0 || self == -1.0 / 0.0
            }
            pub fn is_finite(self) -> bool {
                !self.is_nan() && !self.is_infinite()
            }
            pub fn is_normal(self) -> bool {
                self.is_finite() && self != 0.0 && self.abs() >= Self::MIN_POSITIVE
            }
            pub fn is_sign_negative(self) -> bool {
                if $is32 {
                    intrinsics::f32_to_bits(self as f32) >> 31 != 0
                } else {
                    intrinsics::f64_to_bits(self as f64) >> 63 != 0
                }
            }
            pub fn is_sign_positive(self) -> bool {
                !self.is_sign_negative()
            }
            pub fn abs(self) -> $t {
                if self.is_sign_negative() { -self } else { self }
            }
            pub fn signum(self) -> $t {
                if self.is_nan() {
                    self
                } else if self.is_sign_negative() {
                    -1.0
                } else {
                    1.0
                }
            }
            pub fn copysign(self, sign: $t) -> $t {
                if self.is_sign_negative() == sign.is_sign_negative() { self } else { -self }
            }
            pub fn sqrt(self) -> $t {
                unsafe { libm::sqrt(self as f64) as $t }
            }
            pub fn cbrt(self) -> $t {
                unsafe { libm::cbrt(self as f64) as $t }
            }
            pub fn powi(self, n: i32) -> $t {
                let mut base = self;
                let mut e = if n < 0 { -(n as i64) } else { n as i64 };
                let mut acc: $t = 1.0;
                while e > 0 {
                    if e & 1 == 1 {
                        acc = acc * base;
                    }
                    base = base * base;
                    e >>= 1;
                }
                if n < 0 { 1.0 / acc } else { acc }
            }
            pub fn powf(self, n: $t) -> $t {
                unsafe { libm::pow(self as f64, n as f64) as $t }
            }
            pub fn floor(self) -> $t {
                unsafe { libm::floor(self as f64) as $t }
            }
            pub fn ceil(self) -> $t {
                unsafe { libm::ceil(self as f64) as $t }
            }
            pub fn round(self) -> $t {
                unsafe { libm::round(self as f64) as $t }
            }
            pub fn round_ties_even(self) -> $t {
                unsafe { libm::rint(self as f64) as $t }
            }
            pub fn trunc(self) -> $t {
                unsafe { libm::trunc(self as f64) as $t }
            }
            pub fn fract(self) -> $t {
                self - self.trunc()
            }
            pub fn sin(self) -> $t {
                unsafe { libm::sin(self as f64) as $t }
            }
            pub fn cos(self) -> $t {
                unsafe { libm::cos(self as f64) as $t }
            }
            pub fn tan(self) -> $t {
                unsafe { libm::tan(self as f64) as $t }
            }
            pub fn sin_cos(self) -> ($t, $t) {
                (self.sin(), self.cos())
            }
            pub fn asin(self) -> $t {
                unsafe { libm::asin(self as f64) as $t }
            }
            pub fn acos(self) -> $t {
                unsafe { libm::acos(self as f64) as $t }
            }
            pub fn atan(self) -> $t {
                unsafe { libm::atan(self as f64) as $t }
            }
            pub fn atan2(self, other: $t) -> $t {
                unsafe { libm::atan2(self as f64, other as f64) as $t }
            }
            pub fn sinh(self) -> $t {
                unsafe { libm::sinh(self as f64) as $t }
            }
            pub fn cosh(self) -> $t {
                unsafe { libm::cosh(self as f64) as $t }
            }
            pub fn tanh(self) -> $t {
                unsafe { libm::tanh(self as f64) as $t }
            }
            pub fn exp(self) -> $t {
                unsafe { libm::exp(self as f64) as $t }
            }
            pub fn exp2(self) -> $t {
                unsafe { libm::exp2(self as f64) as $t }
            }
            pub fn exp_m1(self) -> $t {
                unsafe { libm::expm1(self as f64) as $t }
            }
            pub fn ln(self) -> $t {
                unsafe { libm::log(self as f64) as $t }
            }
            pub fn ln_1p(self) -> $t {
                unsafe { libm::log1p(self as f64) as $t }
            }
            pub fn log(self, base: $t) -> $t {
                self.ln() / base.ln()
            }
            pub fn log2(self) -> $t {
                unsafe { libm::log2(self as f64) as $t }
            }
            pub fn log10(self) -> $t {
                unsafe { libm::log10(self as f64) as $t }
            }
            pub fn hypot(self, other: $t) -> $t {
                unsafe { libm::hypot(self as f64, other as f64) as $t }
            }
            pub fn mul_add(self, a: $t, b: $t) -> $t {
                self * a + b
            }
            pub fn recip(self) -> $t {
                1.0 / self
            }
            pub fn to_degrees(self) -> $t {
                self * (180.0 / consts::PI as $t)
            }
            pub fn to_radians(self) -> $t {
                self * (consts::PI as $t / 180.0)
            }
            pub fn max(self, other: $t) -> $t {
                if self.is_nan() {
                    other
                } else if other.is_nan() {
                    self
                } else if self < other {
                    other
                } else {
                    self
                }
            }
            pub fn min(self, other: $t) -> $t {
                if self.is_nan() {
                    other
                } else if other.is_nan() {
                    self
                } else if other < self {
                    other
                } else {
                    self
                }
            }
            pub fn clamp(self, min: $t, max: $t) -> $t {
                if min > max {
                    panic!("min > max, or either was NaN. min = {:?}, max = {:?}", min, max);
                }
                let mut x = self;
                if x < min {
                    x = min;
                }
                if x > max {
                    x = max;
                }
                x
            }
            pub fn rem_euclid(self, rhs: $t) -> $t {
                let r = self % rhs;
                if r < 0.0 { r + rhs.abs() } else { r }
            }
            pub fn div_euclid(self, rhs: $t) -> $t {
                let q = (self / rhs).trunc();
                if self % rhs < 0.0 {
                    if rhs > 0.0 { q - 1.0 } else { q + 1.0 }
                } else {
                    q
                }
            }
            pub fn total_cmp(&self, other: &$t) -> crate::cmp::Ordering {
                let mut a = self.to_bits() as i64;
                let mut b = other.to_bits() as i64;
                if $is32 {
                    a = (self.to_bits() as i32) as i64;
                    b = (other.to_bits() as i32) as i64;
                }
                a ^= (((a >> 63) as u64) >> 1) as i64;
                b ^= (((b >> 63) as u64) >> 1) as i64;
                a.cmp(&b)
            }
        }

        impl Default for $t {
            fn default() -> $t {
                0.0
            }
        }

        impl crate::ops::Neg for &$t {
            type Output = $t;
            fn neg(self) -> $t {
                -*self
            }
        }
    )*};
}
float_impl! {
    f64, u64, false;
    f32, u32, true;
}

impl f64 {
    pub const EPSILON: f64 = 2.220446049250313e-16;
    pub const MAX: f64 = 1.7976931348623157e308;
    pub const MIN: f64 = -1.7976931348623157e308;
    pub const MIN_POSITIVE: f64 = 2.2250738585072014e-308;
    pub const INFINITY: f64 = 1.0 / 0.0;
    pub const NEG_INFINITY: f64 = -1.0 / 0.0;
    pub const NAN: f64 = 0.0 / 0.0;
    pub const DIGITS: u32 = 15;
    pub const MANTISSA_DIGITS: u32 = 53;
    pub fn to_bits(self) -> u64 {
        intrinsics::f64_to_bits(self)
    }
    pub fn from_bits(v: u64) -> f64 {
        intrinsics::f64_from_bits(v)
    }
}

impl f32 {
    pub const EPSILON: f32 = 1.1920929e-7;
    pub const MAX: f32 = 3.40282347e38;
    pub const MIN: f32 = -3.40282347e38;
    pub const MIN_POSITIVE: f32 = 1.17549435e-38;
    pub const INFINITY: f32 = 1.0 / 0.0;
    pub const NEG_INFINITY: f32 = -1.0 / 0.0;
    pub const NAN: f32 = 0.0 / 0.0;
    pub const DIGITS: u32 = 6;
    pub const MANTISSA_DIGITS: u32 = 24;
    pub fn to_bits(self) -> u32 {
        intrinsics::f32_to_bits(self)
    }
    pub fn from_bits(v: u32) -> f32 {
        intrinsics::f32_from_bits(v)
    }
}

pub mod consts {
    pub const PI: f64 = 3.14159265358979323846264338327950288;
    pub const TAU: f64 = 6.28318530717958647692528676655900577;
    pub const E: f64 = 2.71828182845904523536028747135266250;
    pub const SQRT_2: f64 = 1.41421356237309504880168872420969808;
    pub const FRAC_1_SQRT_2: f64 = 0.707106781186547524400844362104849039;
    pub const LN_2: f64 = 0.693147180559945309417232121458176568;
    pub const LN_10: f64 = 2.30258509299404568401799145468436421;
    pub const LOG2_E: f64 = 1.44269504088896340735992468100189214;
    pub const LOG10_E: f64 = 0.434294481903251827651128918916605082;
    pub const LOG2_10: f64 = 3.32192809488736234787031942948939018;
    pub const LOG10_2: f64 = 0.301029995663981195213738894724493027;
    pub const FRAC_PI_2: f64 = 1.57079632679489661923132169163975144;
    pub const FRAC_PI_3: f64 = 1.04719755119659774615421446109316763;
    pub const FRAC_PI_4: f64 = 0.785398163397448309615660845819875721;
    pub const FRAC_PI_6: f64 = 0.52359877559829887307710723054658381;
    pub const FRAC_PI_8: f64 = 0.39269908169872415480783042290993786;
    pub const FRAC_1_PI: f64 = 0.318309886183790671537767526745028724;
    pub const FRAC_2_PI: f64 = 0.636619772367581343075535053490057448;
    pub const FRAC_2_SQRT_PI: f64 = 1.12837916709551257389615890312154517;
}

/// `std::f64` / `std::f32` modules.
pub mod f64 {
    pub use crate::num::consts;
    pub const EPSILON: f64 = 2.220446049250313e-16;
    pub const MAX: f64 = 1.7976931348623157e308;
    pub const MIN: f64 = -1.7976931348623157e308;
    pub const MIN_POSITIVE: f64 = 2.2250738585072014e-308;
    pub const INFINITY: f64 = 1.0 / 0.0;
    pub const NEG_INFINITY: f64 = -1.0 / 0.0;
    pub const NAN: f64 = 0.0 / 0.0;
}

pub mod f32 {
    pub mod consts {
        pub const PI: f32 = 3.14159265358979323846264338327950288;
        pub const TAU: f32 = 6.28318530717958647692528676655900577;
        pub const E: f32 = 2.71828182845904523536028747135266250;
        pub const SQRT_2: f32 = 1.41421356237309504880168872420969808;
        pub const LN_2: f32 = 0.693147180559945309417232121458176568;
        pub const FRAC_PI_2: f32 = 1.57079632679489661923132169163975144;
    }
    pub const EPSILON: f32 = 1.1920929e-7;
    pub const MAX: f32 = 3.40282347e38;
    pub const MIN: f32 = -3.40282347e38;
    pub const INFINITY: f32 = 1.0 / 0.0;
    pub const NEG_INFINITY: f32 = -1.0 / 0.0;
    pub const NAN: f32 = 0.0 / 0.0;
}

/// `std::i32::MAX` and friends (deprecated in Rust but still common).
pub mod int_mods {
    macro_rules! int_mod {
        ($($m:ident $t:ty;)*) => {$(
            pub mod $m {
                pub const MIN: $t = <$t>::MIN;
                pub const MAX: $t = <$t>::MAX;
            }
        )*};
    }
    int_mod! {
        i8 i8; i16 i16; i32 i32; i64 i64; isize isize;
        u8 u8; u16 u16; u32 u32; u64 u64; usize usize;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Wrapping<T>(pub T);

impl<T: crate::fmt::Display> crate::fmt::Display for Wrapping<T> {
    fn fmt(&self, f: &mut crate::fmt::Formatter) -> crate::fmt::Result {
        self.0.fmt(f)
    }
}

macro_rules! wrapping_ops {
    ($($t:ty)*) => {$(
        impl crate::ops::Add for Wrapping<$t> {
            type Output = Wrapping<$t>;
            fn add(self, o: Wrapping<$t>) -> Wrapping<$t> {
                Wrapping(self.0.wrapping_add(o.0))
            }
        }
        impl crate::ops::Sub for Wrapping<$t> {
            type Output = Wrapping<$t>;
            fn sub(self, o: Wrapping<$t>) -> Wrapping<$t> {
                Wrapping(self.0.wrapping_sub(o.0))
            }
        }
        impl crate::ops::Mul for Wrapping<$t> {
            type Output = Wrapping<$t>;
            fn mul(self, o: Wrapping<$t>) -> Wrapping<$t> {
                Wrapping(self.0.wrapping_mul(o.0))
            }
        }
        impl crate::ops::AddAssign for Wrapping<$t> {
            fn add_assign(&mut self, o: Wrapping<$t>) {
                self.0 = self.0.wrapping_add(o.0);
            }
        }
        impl crate::ops::SubAssign for Wrapping<$t> {
            fn sub_assign(&mut self, o: Wrapping<$t>) {
                self.0 = self.0.wrapping_sub(o.0);
            }
        }
        impl crate::ops::MulAssign for Wrapping<$t> {
            fn mul_assign(&mut self, o: Wrapping<$t>) {
                self.0 = self.0.wrapping_mul(o.0);
            }
        }
    )*};
}
wrapping_ops!(i8 i16 i32 i64 isize u8 u16 u32 u64 usize);
