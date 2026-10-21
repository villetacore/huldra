//! Slices: methods on `[T]` and their iterators.

use crate::cmp::Ordering;
use crate::intrinsics;
use crate::option::Option::{self, None, Some};
use crate::vec::Vec;

pub fn from_raw_parts<'a, T>(p: *const T, len: usize) -> &'a [T] {
    intrinsics::slice_from_raw(p, len)
}

pub fn from_raw_parts_mut<'a, T>(p: *mut T, len: usize) -> &'a mut [T] {
    intrinsics::slice_from_raw_mut(p, len)
}

pub fn from_ref<T>(x: &T) -> &[T] {
    intrinsics::slice_from_raw(x as *const T, 1)
}

impl<T> [T] {
    pub fn len(&self) -> usize {
        intrinsics::slice_len(self)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn as_ptr(&self) -> *const T {
        intrinsics::slice_ptr(self)
    }
    pub fn as_mut_ptr(&mut self) -> *mut T {
        intrinsics::slice_ptr(self) as *mut T
    }
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            ptr: self.as_ptr(),
            i: 0,
            j: self.len(),
        }
    }
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut {
            ptr: self.as_mut_ptr(),
            i: 0,
            j: self.len(),
        }
    }
    pub fn get(&self, i: usize) -> Option<&T> {
        if i < self.len() {
            Some(&self[i])
        } else {
            None
        }
    }
    pub fn get_mut(&mut self, i: usize) -> Option<&mut T> {
        if i < self.len() {
            Some(&mut self[i])
        } else {
            None
        }
    }
    pub fn first(&self) -> Option<&T> {
        self.get(0)
    }
    pub fn last(&self) -> Option<&T> {
        if self.is_empty() {
            None
        } else {
            Some(&self[self.len() - 1])
        }
    }
    pub fn first_mut(&mut self) -> Option<&mut T> {
        self.get_mut(0)
    }
    pub fn last_mut(&mut self) -> Option<&mut T> {
        let n = self.len();
        if n == 0 {
            None
        } else {
            Some(&mut self[n - 1])
        }
    }
    pub fn split_first(&self) -> Option<(&T, &[T])> {
        if self.is_empty() {
            None
        } else {
            Some((&self[0], &self[1..]))
        }
    }
    pub fn split_last(&self) -> Option<(&T, &[T])> {
        let n = self.len();
        if n == 0 {
            None
        } else {
            Some((&self[n - 1], &self[..n - 1]))
        }
    }
    pub fn split_at(&self, mid: usize) -> (&[T], &[T]) {
        (&self[..mid], &self[mid..])
    }
    pub fn split_at_mut(&mut self, mid: usize) -> (&mut [T], &mut [T]) {
        let n = self.len();
        let p = self.as_mut_ptr();
        if mid > n {
            panic!("mid > len");
        }
        unsafe { (from_raw_parts_mut(p, mid), from_raw_parts_mut(p.add(mid), n - mid)) }
    }
    pub fn swap(&mut self, a: usize, b: usize) {
        let n = self.len();
        if a >= n || b >= n {
            panic!("index out of bounds: the len is {} but the index is {}", n, if a >= n { a } else { b });
        }
        if a != b {
            unsafe {
                let p = self.as_mut_ptr();
                let x = intrinsics::ptr_read(p.add(a));
                intrinsics::ptr_write(p.add(a), intrinsics::ptr_read(p.add(b)));
                intrinsics::ptr_write(p.add(b), x);
            }
        }
    }
    pub fn reverse(&mut self) {
        let n = self.len();
        let mut i = 0;
        while i < n / 2 {
            self.swap(i, n - 1 - i);
            i += 1;
        }
    }
    pub fn contains(&self, x: &T) -> bool
    where
        T: PartialEq,
    {
        for y in self.iter() {
            if *y == *x {
                return true;
            }
        }
        false
    }
    pub fn starts_with(&self, needle: &[T]) -> bool
    where
        T: PartialEq,
    {
        let n = needle.len();
        self.len() >= n && self[..n] == *needle
    }
    pub fn ends_with(&self, needle: &[T]) -> bool
    where
        T: PartialEq,
    {
        let (m, n) = (self.len(), needle.len());
        m >= n && self[m - n..] == *needle
    }
    pub fn fill(&mut self, value: T)
    where
        T: Clone,
    {
        for x in self.iter_mut() {
            *x = value.clone();
        }
    }
    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        let mut v = Vec::with_capacity(self.len());
        for x in self.iter() {
            v.push(x.clone());
        }
        v
    }
    pub fn repeat(&self, n: usize) -> Vec<T>
    where
        T: Copy,
    {
        let mut v = Vec::with_capacity(self.len() * n);
        let mut i = 0;
        while i < n {
            v.extend_from_slice(self);
            i += 1;
        }
        v
    }
    pub fn copy_from_slice(&mut self, src: &[T])
    where
        T: Copy,
    {
        if self.len() != src.len() {
            panic!("source slice length ({}) does not match destination slice length ({})", src.len(), self.len());
        }
        let mut i = 0;
        while i < src.len() {
            self[i] = src[i];
            i += 1;
        }
    }
    pub fn clone_from_slice(&mut self, src: &[T])
    where
        T: Clone,
    {
        if self.len() != src.len() {
            panic!("destination and source slices have different lengths");
        }
        let mut i = 0;
        while i < src.len() {
            self[i] = src[i].clone();
            i += 1;
        }
    }
    pub fn rotate_left(&mut self, k: usize) {
        let n = self.len();
        if n == 0 {
            return;
        }
        let k = k % n;
        self[..k].reverse();
        self[k..].reverse();
        self.reverse();
    }
    pub fn rotate_right(&mut self, k: usize) {
        let n = self.len();
        if n == 0 {
            return;
        }
        let k = k % n;
        self.rotate_left(n - k);
    }
    pub fn windows(&self, size: usize) -> Windows<'_, T> {
        if size == 0 {
            panic!("window size must be non-zero");
        }
        Windows { s: self, size, i: 0 }
    }
    pub fn chunks(&self, size: usize) -> Chunks<'_, T> {
        if size == 0 {
            panic!("chunk size must be non-zero");
        }
        Chunks { s: self, size, i: 0 }
    }
    pub fn chunks_exact(&self, size: usize) -> Chunks<'_, T> {
        let n = self.len() / size * size;
        Chunks { s: &self[..n], size, i: 0 }
    }
    pub fn chunks_mut(&mut self, size: usize) -> ChunksMut<'_, T> {
        if size == 0 {
            panic!("chunk size must be non-zero");
        }
        ChunksMut {
            ptr: self.as_mut_ptr(),
            len: self.len(),
            size,
            i: 0,
        }
    }
    pub fn split<F: FnMut(&T) -> bool>(&self, pred: F) -> Split<'_, T, F> {
        Split {
            s: self,
            pred,
            i: 0,
            done: false,
        }
    }
    pub fn concat(&self) -> T::Output
    where
        T: crate::slice::Concat,
    {
        T::concat_slice(self)
    }
    pub fn join<S: crate::slice::Join<T>>(&self, sep: S) -> S::Output {
        sep.join_slice(self)
    }

    // ---- sorting and searching ----

    pub fn sort(&mut self)
    where
        T: Ord,
    {
        merge_sort(self, &mut |a: &T, b: &T| a.lt(b));
    }
    pub fn sort_unstable(&mut self)
    where
        T: Ord,
    {
        merge_sort(self, &mut |a: &T, b: &T| a.lt(b));
    }
    pub fn sort_by<F: FnMut(&T, &T) -> Ordering>(&mut self, mut compare: F) {
        merge_sort(self, &mut |a: &T, b: &T| compare(a, b) == Ordering::Less);
    }
    pub fn sort_unstable_by<F: FnMut(&T, &T) -> Ordering>(&mut self, mut compare: F) {
        merge_sort(self, &mut |a: &T, b: &T| compare(a, b) == Ordering::Less);
    }
    pub fn sort_by_key<K: Ord, F: FnMut(&T) -> K>(&mut self, mut f: F) {
        merge_sort(self, &mut |a: &T, b: &T| f(a).lt(&f(b)));
    }
    pub fn sort_unstable_by_key<K: Ord, F: FnMut(&T) -> K>(&mut self, mut f: F) {
        merge_sort(self, &mut |a: &T, b: &T| f(a).lt(&f(b)));
    }
    pub fn sort_by_cached_key<K: Ord, F: FnMut(&T) -> K>(&mut self, mut f: F) {
        merge_sort(self, &mut |a: &T, b: &T| f(a).lt(&f(b)));
    }
    pub fn is_sorted(&self) -> bool
    where
        T: PartialOrd,
    {
        let mut i = 1;
        while i < self.len() {
            if self[i - 1] > self[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn binary_search(&self, x: &T) -> Result<usize, usize>
    where
        T: Ord,
    {
        self.binary_search_by(|p| p.cmp(x))
    }
    pub fn binary_search_by<F: FnMut(&T) -> Ordering>(&self, mut f: F) -> Result<usize, usize> {
        let mut lo = 0;
        let mut hi = self.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match f(&self[mid]) {
                Ordering::Less => lo = mid + 1,
                Ordering::Greater => hi = mid,
                Ordering::Equal => return Ok(mid),
            }
        }
        Err(lo)
    }
    pub fn binary_search_by_key<B: Ord, F: FnMut(&T) -> B>(&self, b: &B, mut f: F) -> Result<usize, usize> {
        self.binary_search_by(|k| f(k).cmp(b))
    }
    pub fn partition_point<P: FnMut(&T) -> bool>(&self, mut pred: P) -> usize {
        let mut lo = 0;
        let mut hi = self.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if pred(&self[mid]) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }
    pub fn select_nth_unstable(&mut self, index: usize)
    where
        T: Ord,
    {
        self.sort();
        let _ = index;
    }
}

/// A stable merge sort; `less(a, b)` is `a < b`.
fn merge_sort<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], less: &mut F) {
    let n = v.len();
    if n < 2 {
        return;
    }
    if n <= 16 {
        // Insertion sort.
        let mut i = 1;
        while i < n {
            let mut j = i;
            while j > 0 && less(&v[j], &v[j - 1]) {
                v.swap(j, j - 1);
                j -= 1;
            }
            i += 1;
        }
        return;
    }
    let mid = n / 2;
    merge_sort(&mut v[..mid], less);
    merge_sort(&mut v[mid..], less);
    if !less(&v[mid], &v[mid - 1]) {
        return;
    }
    unsafe {
        let buf = crate::alloc::alloc_array::<T>(n);
        let p = v.as_mut_ptr();
        let (mut i, mut j, mut k) = (0, mid, 0);
        while i < mid && j < n {
            if less(&*p.add(j), &*p.add(i)) {
                intrinsics::ptr_write(buf.add(k), intrinsics::ptr_read(p.add(j)));
                j += 1;
            } else {
                intrinsics::ptr_write(buf.add(k), intrinsics::ptr_read(p.add(i)));
                i += 1;
            }
            k += 1;
        }
        while i < mid {
            intrinsics::ptr_write(buf.add(k), intrinsics::ptr_read(p.add(i)));
            i += 1;
            k += 1;
        }
        while j < n {
            intrinsics::ptr_write(buf.add(k), intrinsics::ptr_read(p.add(j)));
            j += 1;
            k += 1;
        }
        intrinsics::ptr_copy(buf as *const T, p, n);
        crate::sys::free(buf as *mut u8);
    }
}

/// `[a, b].concat()`: what the elements concatenate to.
pub trait Concat: Sized {
    type Output;
    fn concat_slice(s: &[Self]) -> Self::Output;
}

impl Concat for crate::string::String {
    type Output = crate::string::String;
    fn concat_slice(s: &[crate::string::String]) -> crate::string::String {
        let mut out = crate::string::String::new();
        for x in s.iter() {
            out.push_str(x);
        }
        out
    }
}

impl<'a> Concat for &'a str {
    type Output = crate::string::String;
    fn concat_slice(s: &[&'a str]) -> crate::string::String {
        let mut out = crate::string::String::new();
        for x in s.iter() {
            out.push_str(x);
        }
        out
    }
}

impl<T: Clone> Concat for Vec<T> {
    type Output = Vec<T>;
    fn concat_slice(s: &[Vec<T>]) -> Vec<T> {
        let mut out = Vec::new();
        for x in s.iter() {
            out.extend_from_slice(x);
        }
        out
    }
}

impl<'a, T: Clone> Concat for &'a [T] {
    type Output = Vec<T>;
    fn concat_slice(s: &[&'a [T]]) -> Vec<T> {
        let mut out = Vec::new();
        for x in s.iter() {
            out.extend_from_slice(x);
        }
        out
    }
}

impl<T: Clone, const N: usize> Concat for [T; N] {
    type Output = Vec<T>;
    fn concat_slice(s: &[[T; N]]) -> Vec<T> {
        let mut out = Vec::new();
        for x in s.iter() {
            out.extend_from_slice(x);
        }
        out
    }
}

/// `[a, b].join(sep)` for slices of strings and of vectors.
pub trait Join<T: ?Sized> {
    type Output;
    fn join_slice(self, s: &[T]) -> Self::Output;
}

impl<'a, S: crate::borrow::Borrow<str>> Join<S> for &'a str {
    type Output = crate::string::String;
    fn join_slice(self, s: &[S]) -> crate::string::String {
        let mut out = crate::string::String::new();
        let mut first = true;
        for x in s.iter() {
            if !first {
                out.push_str(self);
            }
            first = false;
            out.push_str(<S as crate::borrow::Borrow<str>>::borrow(x));
        }
        out
    }
}

impl<'a, T: Clone, V: crate::borrow::Borrow<[T]>> Join<V> for &'a [T] {
    type Output = Vec<T>;
    fn join_slice(self, s: &[V]) -> Vec<T> {
        let mut out = Vec::new();
        let mut first = true;
        for x in s.iter() {
            if !first {
                out.extend_from_slice(self);
            }
            first = false;
            out.extend_from_slice(<V as crate::borrow::Borrow<[T]>>::borrow(x));
        }
        out
    }
}

impl<'a, T: Clone, V: crate::borrow::Borrow<[T]>> Join<V> for &'a T {
    type Output = Vec<T>;
    fn join_slice(self, s: &[V]) -> Vec<T> {
        let mut out = Vec::new();
        let mut first = true;
        for x in s.iter() {
            if !first {
                out.push(self.clone());
            }
            first = false;
            out.extend_from_slice(<V as crate::borrow::Borrow<[T]>>::borrow(x));
        }
        out
    }
}

pub struct Iter<'a, T> {
    ptr: *const T,
    i: usize,
    j: usize,
}

impl<'a, T> Clone for Iter<'a, T> {
    fn clone(&self) -> Iter<'a, T> {
        Iter {
            ptr: self.ptr,
            i: self.i,
            j: self.j,
        }
    }
}

impl<'a, T> Iter<'a, T> {
    pub fn as_slice(&self) -> &'a [T] {
        unsafe { from_raw_parts(self.ptr.add(self.i), self.j - self.i) }
    }
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<&'a T> {
        if self.i < self.j {
            let x = unsafe { &*self.ptr.add(self.i) };
            self.i += 1;
            Some(x)
        } else {
            None
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.j - self.i, Some(self.j - self.i))
    }
    fn nth(&mut self, n: usize) -> Option<&'a T> {
        if n < self.j - self.i {
            self.i += n;
            self.next()
        } else {
            self.i = self.j;
            None
        }
    }
    fn count(self) -> usize {
        self.j - self.i
    }
}

impl<'a, T> DoubleEndedIterator for Iter<'a, T> {
    fn next_back(&mut self) -> Option<&'a T> {
        if self.i < self.j {
            self.j -= 1;
            Some(unsafe { &*self.ptr.add(self.j) })
        } else {
            None
        }
    }
}

impl<'a, T> ExactSizeIterator for Iter<'a, T> {}

pub struct IterMut<'a, T> {
    ptr: *mut T,
    i: usize,
    j: usize,
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;
    fn next(&mut self) -> Option<&'a mut T> {
        if self.i < self.j {
            let x = unsafe { &mut *self.ptr.add(self.i) };
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

impl<'a, T> DoubleEndedIterator for IterMut<'a, T> {
    fn next_back(&mut self) -> Option<&'a mut T> {
        if self.i < self.j {
            self.j -= 1;
            Some(unsafe { &mut *self.ptr.add(self.j) })
        } else {
            None
        }
    }
}

impl<'a, T> ExactSizeIterator for IterMut<'a, T> {}

impl<'a, T> IntoIterator for &'a [T] {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;
    fn into_iter(self) -> Iter<'a, T> {
        self.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut [T] {
    type Item = &'a mut T;
    type IntoIter = IterMut<'a, T>;
    fn into_iter(self) -> IterMut<'a, T> {
        self.iter_mut()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a [T; N] {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;
    fn into_iter(self) -> Iter<'a, T> {
        self[..].iter()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a mut [T; N] {
    type Item = &'a mut T;
    type IntoIter = IterMut<'a, T>;
    fn into_iter(self) -> IterMut<'a, T> {
        self[..].iter_mut()
    }
}

impl<T, const N: usize> IntoIterator for [T; N] {
    type Item = T;
    type IntoIter = crate::vec::IntoIter<T>;
    fn into_iter(self) -> crate::vec::IntoIter<T> {
        crate::vec::from_array(self).into_iter()
    }
}

pub struct Windows<'a, T> {
    s: &'a [T],
    size: usize,
    i: usize,
}

impl<'a, T> Iterator for Windows<'a, T> {
    type Item = &'a [T];
    fn next(&mut self) -> Option<&'a [T]> {
        if self.i + self.size <= self.s.len() {
            let w = &self.s[self.i..self.i + self.size];
            self.i += 1;
            Some(w)
        } else {
            None
        }
    }
}

pub struct Chunks<'a, T> {
    s: &'a [T],
    size: usize,
    i: usize,
}

impl<'a, T> Iterator for Chunks<'a, T> {
    type Item = &'a [T];
    fn next(&mut self) -> Option<&'a [T]> {
        if self.i < self.s.len() {
            let end = if self.i + self.size < self.s.len() { self.i + self.size } else { self.s.len() };
            let c = &self.s[self.i..end];
            self.i = end;
            Some(c)
        } else {
            None
        }
    }
}

pub struct ChunksMut<'a, T> {
    ptr: *mut T,
    len: usize,
    size: usize,
    i: usize,
}

impl<'a, T> Iterator for ChunksMut<'a, T> {
    type Item = &'a mut [T];
    fn next(&mut self) -> Option<&'a mut [T]> {
        if self.i < self.len {
            let end = if self.i + self.size < self.len { self.i + self.size } else { self.len };
            let c = unsafe { from_raw_parts_mut(self.ptr.add(self.i), end - self.i) };
            self.i = end;
            Some(c)
        } else {
            None
        }
    }
}

pub struct Split<'a, T, F> {
    s: &'a [T],
    pred: F,
    i: usize,
    done: bool,
}

impl<'a, T, F: FnMut(&T) -> bool> Iterator for Split<'a, T, F> {
    type Item = &'a [T];
    fn next(&mut self) -> Option<&'a [T]> {
        if self.done {
            return None;
        }
        let mut j = self.i;
        while j < self.s.len() {
            if (self.pred)(&self.s[j]) {
                let part = &self.s[self.i..j];
                self.i = j + 1;
                return Some(part);
            }
            j += 1;
        }
        self.done = true;
        Some(&self.s[self.i..])
    }
}
