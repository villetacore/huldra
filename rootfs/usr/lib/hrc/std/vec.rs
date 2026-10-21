//! `Vec<T>`, a growable array.

use crate::alloc::{alloc_array, realloc_array};
use crate::fmt;
use crate::intrinsics;
use crate::option::Option::{self, None, Some};

pub struct Vec<T> {
    ptr: *mut T,
    cap: usize,
    len: usize,
}

/// `vec![a, b, c]`
pub fn from_array<T, const N: usize>(a: [T; N]) -> Vec<T> {
    let mut v = Vec::with_capacity(N);
    unsafe {
        intrinsics::ptr_copy(&a as *const [T; N] as *const T, v.ptr, N);
    }
    v.len = N;
    crate::mem::forget(a);
    v
}

/// `vec![x; n]`
pub fn from_elem<T: Clone>(x: T, n: usize) -> Vec<T> {
    let mut v = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        v.push(x.clone());
        i += 1;
    }
    v
}

impl<T> Vec<T> {
    pub fn new() -> Vec<T> {
        Vec {
            ptr: intrinsics::ptr_null(),
            cap: 0,
            len: 0,
        }
    }
    pub fn with_capacity(n: usize) -> Vec<T> {
        let mut v = Vec::new();
        if n > 0 {
            v.ptr = unsafe { alloc_array::<T>(n) };
            v.cap = n;
        }
        v
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn capacity(&self) -> usize {
        self.cap
    }
    pub fn as_ptr(&self) -> *const T {
        self.ptr as *const T
    }
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.ptr
    }
    pub fn as_slice(&self) -> &[T] {
        intrinsics::slice_from_raw(self.ptr as *const T, self.len)
    }
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        intrinsics::slice_from_raw_mut(self.ptr, self.len)
    }
    pub unsafe fn set_len(&mut self, n: usize) {
        self.len = n;
    }
    pub fn reserve(&mut self, additional: usize) {
        if self.len + additional > self.cap {
            let mut c = if self.cap == 0 { 4 } else { self.cap * 2 };
            if c < self.len + additional {
                c = self.len + additional;
            }
            self.ptr = unsafe {
                if self.cap == 0 {
                    alloc_array::<T>(c)
                } else {
                    realloc_array(self.ptr, c)
                }
            };
            self.cap = c;
        }
    }
    pub fn reserve_exact(&mut self, additional: usize) {
        self.reserve(additional);
    }
    pub fn shrink_to_fit(&mut self) {}
    pub fn push(&mut self, x: T) {
        if self.len == self.cap {
            self.reserve(1);
        }
        unsafe {
            intrinsics::ptr_write(self.ptr.add(self.len), x);
        }
        self.len += 1;
    }
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            Some(unsafe { intrinsics::ptr_read(self.ptr.add(self.len) as *const T) })
        }
    }
    #[track_caller]
    pub fn insert(&mut self, index: usize, x: T) {
        if index > self.len {
            crate::rt::begin_panic(format!("insertion index (is {}) should be <= len (is {})", index, self.len), crate::intrinsics::caller_location());
        }
        if self.len == self.cap {
            self.reserve(1);
        }
        unsafe {
            let p = self.ptr.add(index);
            intrinsics::ptr_copy(p as *const T, p.add(1), self.len - index);
            intrinsics::ptr_write(p, x);
        }
        self.len += 1;
    }
    #[track_caller]
    pub fn remove(&mut self, index: usize) -> T {
        if index >= self.len {
            crate::rt::begin_panic(format!("removal index (is {}) should be < len (is {})", index, self.len), crate::intrinsics::caller_location());
        }
        unsafe {
            let p = self.ptr.add(index);
            let x = intrinsics::ptr_read(p as *const T);
            intrinsics::ptr_copy(p.add(1) as *const T, p, self.len - index - 1);
            self.len -= 1;
            x
        }
    }
    #[track_caller]
    pub fn swap_remove(&mut self, index: usize) -> T {
        if index >= self.len {
            crate::rt::begin_panic(format!("swap_remove index (is {}) should be < len (is {})", index, self.len), crate::intrinsics::caller_location());
        }
        let last = self.len - 1;
        self.as_mut_slice().swap(index, last);
        self.pop().unwrap()
    }
    pub fn truncate(&mut self, n: usize) {
        if n < self.len {
            self.len = n;
        }
    }
    pub fn clear(&mut self) {
        self.len = 0;
    }
    pub fn append(&mut self, other: &mut Vec<T>) {
        self.reserve(other.len);
        unsafe {
            intrinsics::ptr_copy(other.ptr as *const T, self.ptr.add(self.len), other.len);
        }
        self.len += other.len;
        other.len = 0;
    }
    pub fn split_off(&mut self, at: usize) -> Vec<T> {
        if at > self.len {
            panic!("`at` split index (is {}) should be <= len (is {})", at, self.len);
        }
        let n = self.len - at;
        let mut v = Vec::with_capacity(n);
        unsafe {
            intrinsics::ptr_copy(self.ptr.add(at) as *const T, v.ptr, n);
        }
        v.len = n;
        self.len = at;
        v
    }
    pub fn retain<F: FnMut(&T) -> bool>(&mut self, mut f: F) {
        let mut j = 0;
        let mut i = 0;
        while i < self.len {
            unsafe {
                let p = self.ptr.add(i);
                if f(&*p) {
                    if i != j {
                        intrinsics::ptr_write(self.ptr.add(j), intrinsics::ptr_read(p as *const T));
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        self.len = j;
    }
    pub fn retain_mut<F: FnMut(&mut T) -> bool>(&mut self, mut f: F) {
        let mut j = 0;
        let mut i = 0;
        while i < self.len {
            unsafe {
                let p = self.ptr.add(i);
                if f(&mut *p) {
                    if i != j {
                        intrinsics::ptr_write(self.ptr.add(j), intrinsics::ptr_read(p as *const T));
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        self.len = j;
    }
    pub fn dedup_by_key<K: PartialEq, F: FnMut(&mut T) -> K>(&mut self, mut key: F) {
        self.dedup_by(|a, b| key(a) == key(b))
    }
    pub fn dedup_by<F: FnMut(&mut T, &mut T) -> bool>(&mut self, mut same: F) {
        if self.len < 2 {
            return;
        }
        let mut j = 1;
        let mut i = 1;
        while i < self.len {
            unsafe {
                let cur = self.ptr.add(i);
                let prev = self.ptr.add(j - 1);
                if !same(&mut *cur, &mut *prev) {
                    if i != j {
                        intrinsics::ptr_write(self.ptr.add(j), intrinsics::ptr_read(cur as *const T));
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        self.len = j;
    }
    pub fn drain<R: crate::vec::RangeArg>(&mut self, range: R) -> IntoIter<T> {
        let (lo, hi) = range.bounds(self.len);
        if lo > hi || hi > self.len {
            panic!("range end index {} out of range for slice of length {}", hi, self.len);
        }
        let n = hi - lo;
        let mut out = Vec::with_capacity(n);
        unsafe {
            intrinsics::ptr_copy(self.ptr.add(lo) as *const T, out.ptr, n);
            out.len = n;
            intrinsics::ptr_copy(self.ptr.add(hi) as *const T, self.ptr.add(lo), self.len - hi);
        }
        self.len -= n;
        out.into_iter()
    }
    pub fn leak<'a>(self) -> &'a mut [T] {
        intrinsics::slice_from_raw_mut(self.ptr, self.len)
    }
    pub fn iter(&self) -> crate::slice::Iter<'_, T> {
        self.as_slice().iter()
    }
    pub fn iter_mut(&mut self) -> crate::slice::IterMut<'_, T> {
        self.as_mut_slice().iter_mut()
    }
    pub fn resize_with<F: FnMut() -> T>(&mut self, n: usize, mut f: F) {
        while self.len < n {
            self.push(f());
        }
        self.truncate(n);
    }
}

impl<T: Clone> Vec<T> {
    pub fn extend_from_slice(&mut self, other: &[T]) {
        self.reserve(other.len());
        for x in other.iter() {
            self.push(x.clone());
        }
    }
    pub fn resize(&mut self, n: usize, value: T) {
        while self.len < n {
            self.push(value.clone());
        }
        self.truncate(n);
    }
    pub fn extend_from_within<R: crate::vec::RangeArg>(&mut self, range: R) {
        let (lo, hi) = range.bounds(self.len);
        let mut i = lo;
        while i < hi {
            let x = self[i].clone();
            self.push(x);
            i += 1;
        }
    }
}

impl<T: PartialEq> Vec<T> {
    pub fn dedup(&mut self) {
        self.dedup_by(|a, b| *a == *b)
    }
}

/// Ranges accepted by `drain`.
pub trait RangeArg {
    fn bounds(&self, len: usize) -> (usize, usize);
}
impl RangeArg for crate::ops::Range<usize> {
    fn bounds(&self, _len: usize) -> (usize, usize) {
        (self.start, self.end)
    }
}
impl RangeArg for crate::ops::RangeInclusive<usize> {
    fn bounds(&self, _len: usize) -> (usize, usize) {
        (self.start, self.end + 1)
    }
}
impl RangeArg for crate::ops::RangeFrom<usize> {
    fn bounds(&self, len: usize) -> (usize, usize) {
        (self.start, len)
    }
}
impl RangeArg for crate::ops::RangeTo<usize> {
    fn bounds(&self, _len: usize) -> (usize, usize) {
        (0, self.end)
    }
}
impl RangeArg for crate::ops::RangeToInclusive<usize> {
    fn bounds(&self, _len: usize) -> (usize, usize) {
        (0, self.end + 1)
    }
}
impl RangeArg for crate::ops::RangeFull {
    fn bounds(&self, len: usize) -> (usize, usize) {
        (0, len)
    }
}

impl<T> crate::ops::Deref for Vec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        intrinsics::slice_from_raw(self.ptr as *const T, self.len)
    }
}

impl<T> crate::ops::DerefMut for Vec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        intrinsics::slice_from_raw_mut(self.ptr, self.len)
    }
}

impl<T: Clone> Clone for Vec<T> {
    fn clone(&self) -> Vec<T> {
        let mut v = Vec::with_capacity(self.len);
        for x in self.iter() {
            v.push(x.clone());
        }
        v
    }
}

impl<T> Default for Vec<T> {
    fn default() -> Vec<T> {
        Vec::new()
    }
}

impl<T: fmt::Debug> fmt::Debug for Vec<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self.as_slice(), f)
    }
}

impl<T: PartialEq<U>, U> PartialEq<Vec<U>> for Vec<T> {
    fn eq(&self, other: &Vec<U>) -> bool {
        PartialEq::eq(self.as_slice(), other.as_slice())
    }
}
impl<T: PartialEq<U>, U> PartialEq<[U]> for Vec<T> {
    fn eq(&self, other: &[U]) -> bool {
        PartialEq::eq(self.as_slice(), other)
    }
}
impl<'a, T: PartialEq<U>, U> PartialEq<&'a [U]> for Vec<T> {
    fn eq(&self, other: &&[U]) -> bool {
        PartialEq::eq(self.as_slice(), *other)
    }
}
impl<T: PartialEq<U>, U, const N: usize> PartialEq<[U; N]> for Vec<T> {
    fn eq(&self, other: &[U; N]) -> bool {
        PartialEq::eq(self.as_slice(), &other[..])
    }
}
impl<'a, T: PartialEq<U>, U, const N: usize> PartialEq<&'a [U; N]> for Vec<T> {
    fn eq(&self, other: &&[U; N]) -> bool {
        PartialEq::eq(self.as_slice(), &other[..])
    }
}
impl<T: Eq> Eq for Vec<T> {}
impl<T: PartialOrd> PartialOrd for Vec<T> {
    fn partial_cmp(&self, other: &Vec<T>) -> Option<crate::cmp::Ordering> {
        PartialOrd::partial_cmp(self.as_slice(), other.as_slice())
    }
}
impl<T: Ord> Ord for Vec<T> {
    fn cmp(&self, other: &Vec<T>) -> crate::cmp::Ordering {
        Ord::cmp(self.as_slice(), other.as_slice())
    }
}

impl<T: crate::hash::Hash> crate::hash::Hash for Vec<T> {
    fn hash<H: crate::hash::Hasher>(&self, state: &mut H) {
        crate::hash::Hash::hash(self.as_slice(), state)
    }
}

impl<T> AsRef<[T]> for Vec<T> {
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T> AsMut<[T]> for Vec<T> {
    fn as_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T> crate::borrow::Borrow<[T]> for Vec<T> {
    fn borrow(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: Clone> From<&[T]> for Vec<T> {
    fn from(s: &[T]) -> Vec<T> {
        s.to_vec()
    }
}

impl<T: Clone> From<&mut [T]> for Vec<T> {
    fn from(s: &mut [T]) -> Vec<T> {
        s.to_vec()
    }
}

impl<T, const N: usize> From<[T; N]> for Vec<T> {
    fn from(a: [T; N]) -> Vec<T> {
        from_array(a)
    }
}

impl From<&str> for Vec<u8> {
    fn from(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }
}

impl<T> crate::iter::FromIterator<T> for Vec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Vec<T> {
        let mut it = iter.into_iter();
        let (lo, _) = it.size_hint();
        let mut v = Vec::with_capacity(lo);
        while let Some(x) = it.next() {
            v.push(x);
        }
        v
    }
}

impl<T> crate::iter::Extend<T> for Vec<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for x in iter {
            self.push(x);
        }
    }
    fn extend_one(&mut self, x: T) {
        self.push(x);
    }
}

impl<'a, T: Copy + 'a> crate::iter::Extend<&'a T> for Vec<T> {
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        for x in iter {
            self.push(*x);
        }
    }
    fn extend_one(&mut self, x: &'a T) {
        self.push(*x);
    }
}

pub struct IntoIter<T> {
    buf: *mut T,
    i: usize,
    j: usize,
}

impl<T> IntoIter<T> {
    pub fn as_slice(&self) -> &[T] {
        unsafe { intrinsics::slice_from_raw(self.buf.add(self.i) as *const T, self.j - self.i) }
    }
}

impl<T: Clone> Clone for IntoIter<T> {
    fn clone(&self) -> IntoIter<T> {
        self.as_slice().to_vec().into_iter()
    }
}

impl<T> Iterator for IntoIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.i < self.j {
            let x = unsafe { intrinsics::ptr_read(self.buf.add(self.i) as *const T) };
            self.i += 1;
            Some(x)
        } else {
            None
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.j - self.i, Some(self.j - self.i))
    }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
    fn next_back(&mut self) -> Option<T> {
        if self.i < self.j {
            self.j -= 1;
            Some(unsafe { intrinsics::ptr_read(self.buf.add(self.j) as *const T) })
        } else {
            None
        }
    }
}

impl<T> ExactSizeIterator for IntoIter<T> {}

impl<T> IntoIterator for Vec<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;
    fn into_iter(self) -> IntoIter<T> {
        IntoIter {
            buf: self.ptr,
            i: 0,
            j: self.len,
        }
    }
}

impl<'a, T> IntoIterator for &'a Vec<T> {
    type Item = &'a T;
    type IntoIter = crate::slice::Iter<'a, T>;
    fn into_iter(self) -> crate::slice::Iter<'a, T> {
        self.as_slice().iter()
    }
}

impl<'a, T> IntoIterator for &'a mut Vec<T> {
    type Item = &'a mut T;
    type IntoIter = crate::slice::IterMut<'a, T>;
    fn into_iter(self) -> crate::slice::IterMut<'a, T> {
        self.as_mut_slice().iter_mut()
    }
}

impl crate::io::Write for Vec<u8> {
    fn write(&mut self, buf: &[u8]) -> crate::io::Result<usize> {
        self.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> crate::io::Result<()> {
        Ok(())
    }
}
