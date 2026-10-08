//! Small helpers usable before the heap exists.

use core::ops::Deref;

/// Fixed-capacity vector for `Copy` types; extra pushes are dropped.
#[derive(Clone, Copy)]
pub struct ArrayVec<T: Copy, const N: usize> {
    items: [T; N],
    len: usize,
}

impl<T: Copy + Default, const N: usize> ArrayVec<T, N> {
    pub fn new() -> Self {
        ArrayVec {
            items: [T::default(); N],
            len: 0,
        }
    }
}

impl<T: Copy, const N: usize> ArrayVec<T, N> {
    pub fn push(&mut self, item: T) -> bool {
        if self.len == N {
            return false;
        }
        self.items[self.len] = item;
        self.len += 1;
        true
    }
}

impl<T: Copy, const N: usize> Deref for ArrayVec<T, N> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.items[..self.len]
    }
}

/// Fixed-capacity string; longer input is truncated at a char boundary.
#[derive(Clone, Copy)]
pub struct ArrayString<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> ArrayString<N> {
    pub const fn new() -> Self {
        ArrayString {
            bytes: [0; N],
            len: 0,
        }
    }

    pub fn from_bytes(src: &[u8]) -> Self {
        let mut s = Self::new();
        let text = match core::str::from_utf8(src) {
            Ok(t) => t,
            Err(e) => core::str::from_utf8(&src[..e.valid_up_to()]).unwrap_or(""),
        };
        let mut n = text.len().min(N);
        while !text.is_char_boundary(n) {
            n -= 1;
        }
        s.bytes[..n].copy_from_slice(&text.as_bytes()[..n]);
        s.len = n;
        s
    }

    pub fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.bytes[..self.len]) }
    }
}

impl<const N: usize> Default for ArrayString<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Deref for ArrayString<N> {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}
