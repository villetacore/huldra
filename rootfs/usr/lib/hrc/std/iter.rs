//! Iterators: the traits, adapters and sources.

use crate::cmp::Ordering;
use crate::option::Option::{self, None, Some};

pub trait Iterator {
    type Item;

    fn next(&mut self) -> Option<Self::Item>;

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }

    fn count(mut self) -> usize
    where
        Self: Sized,
    {
        let mut n = 0;
        while let Some(_) = self.next() {
            n += 1;
        }
        n
    }

    fn last(mut self) -> Option<Self::Item>
    where
        Self: Sized,
    {
        let mut last = None;
        while let Some(x) = self.next() {
            last = Some(x);
        }
        last
    }

    fn nth(&mut self, mut n: usize) -> Option<Self::Item> {
        while let Some(x) = self.next() {
            if n == 0 {
                return Some(x);
            }
            n -= 1;
        }
        None
    }

    fn step_by(self, step: usize) -> StepBy<Self>
    where
        Self: Sized,
    {
        if step == 0 {
            panic!("assertion failed: step != 0");
        }
        StepBy { iter: self, step: step - 1, first: true }
    }

    fn chain<U: IntoIterator<Item = Self::Item>>(self, other: U) -> Chain<Self, U::IntoIter>
    where
        Self: Sized,
    {
        Chain {
            a: Some(self),
            b: Some(other.into_iter()),
        }
    }

    fn zip<U: IntoIterator>(self, other: U) -> Zip<Self, U::IntoIter>
    where
        Self: Sized,
    {
        Zip {
            a: self,
            b: other.into_iter(),
        }
    }

    fn map<B, F: FnMut(Self::Item) -> B>(self, f: F) -> Map<Self, F>
    where
        Self: Sized,
    {
        Map { iter: self, f }
    }

    fn for_each<F: FnMut(Self::Item)>(mut self, mut f: F)
    where
        Self: Sized,
    {
        while let Some(x) = self.next() {
            f(x);
        }
    }

    fn filter<P: FnMut(&Self::Item) -> bool>(self, predicate: P) -> Filter<Self, P>
    where
        Self: Sized,
    {
        Filter { iter: self, predicate }
    }

    fn filter_map<B, F: FnMut(Self::Item) -> Option<B>>(self, f: F) -> FilterMap<Self, F>
    where
        Self: Sized,
    {
        FilterMap { iter: self, f }
    }

    fn enumerate(self) -> Enumerate<Self>
    where
        Self: Sized,
    {
        Enumerate { iter: self, count: 0 }
    }

    fn peekable(self) -> Peekable<Self>
    where
        Self: Sized,
    {
        Peekable { iter: self, peeked: None }
    }

    fn skip_while<P: FnMut(&Self::Item) -> bool>(self, predicate: P) -> SkipWhile<Self, P>
    where
        Self: Sized,
    {
        SkipWhile {
            iter: self,
            flag: false,
            predicate,
        }
    }

    fn take_while<P: FnMut(&Self::Item) -> bool>(self, predicate: P) -> TakeWhile<Self, P>
    where
        Self: Sized,
    {
        TakeWhile {
            iter: self,
            flag: false,
            predicate,
        }
    }

    fn map_while<B, P: FnMut(Self::Item) -> Option<B>>(self, predicate: P) -> MapWhile<Self, P>
    where
        Self: Sized,
    {
        MapWhile { iter: self, predicate }
    }

    fn skip(self, n: usize) -> Skip<Self>
    where
        Self: Sized,
    {
        Skip { iter: self, n }
    }

    fn take(self, n: usize) -> Take<Self>
    where
        Self: Sized,
    {
        Take { iter: self, n }
    }

    fn scan<St, B, F: FnMut(&mut St, Self::Item) -> Option<B>>(self, initial_state: St, f: F) -> Scan<Self, St, F>
    where
        Self: Sized,
    {
        Scan {
            iter: self,
            state: initial_state,
            f,
        }
    }

    fn flat_map<U: IntoIterator, F: FnMut(Self::Item) -> U>(self, f: F) -> FlatMap<Self, U, F>
    where
        Self: Sized,
    {
        FlatMap {
            iter: self,
            f,
            front: None,
            back: None,
        }
    }

    fn flatten(self) -> Flatten<Self>
    where
        Self: Sized,
        Self::Item: IntoIterator,
    {
        Flatten {
            iter: self,
            front: None,
            back: None,
        }
    }

    fn fuse(self) -> Fuse<Self>
    where
        Self: Sized,
    {
        Fuse { iter: Some(self) }
    }

    fn inspect<F: FnMut(&Self::Item)>(self, f: F) -> Inspect<Self, F>
    where
        Self: Sized,
    {
        Inspect { iter: self, f }
    }

    fn by_ref(&mut self) -> &mut Self
    where
        Self: Sized,
    {
        self
    }

    fn collect<B: FromIterator<Self::Item>>(self) -> B
    where
        Self: Sized,
    {
        B::from_iter(self)
    }

    fn partition<B: Default + Extend<Self::Item>, F: FnMut(&Self::Item) -> bool>(mut self, mut f: F) -> (B, B)
    where
        Self: Sized,
    {
        let mut left: B = Default::default();
        let mut right: B = Default::default();
        while let Some(x) = self.next() {
            if f(&x) {
                left.extend_one(x);
            } else {
                right.extend_one(x);
            }
        }
        (left, right)
    }

    fn fold<B, F: FnMut(B, Self::Item) -> B>(mut self, init: B, mut f: F) -> B
    where
        Self: Sized,
    {
        let mut acc = init;
        while let Some(x) = self.next() {
            acc = f(acc, x);
        }
        acc
    }

    fn try_fold<B, E, F: FnMut(B, Self::Item) -> Result<B, E>>(&mut self, init: B, mut f: F) -> Result<B, E>
    where
        Self: Sized,
    {
        let mut acc = init;
        while let Some(x) = self.next() {
            acc = f(acc, x)?;
        }
        Ok(acc)
    }

    fn try_for_each<E, F: FnMut(Self::Item) -> Result<(), E>>(&mut self, mut f: F) -> Result<(), E>
    where
        Self: Sized,
    {
        while let Some(x) = self.next() {
            f(x)?;
        }
        Ok(())
    }

    fn reduce<F: FnMut(Self::Item, Self::Item) -> Self::Item>(mut self, f: F) -> Option<Self::Item>
    where
        Self: Sized,
    {
        let first = self.next()?;
        Some(self.fold(first, f))
    }

    fn all<F: FnMut(Self::Item) -> bool>(&mut self, mut f: F) -> bool
    where
        Self: Sized,
    {
        while let Some(x) = self.next() {
            if !f(x) {
                return false;
            }
        }
        true
    }

    fn any<F: FnMut(Self::Item) -> bool>(&mut self, mut f: F) -> bool
    where
        Self: Sized,
    {
        while let Some(x) = self.next() {
            if f(x) {
                return true;
            }
        }
        false
    }

    fn find<P: FnMut(&Self::Item) -> bool>(&mut self, mut predicate: P) -> Option<Self::Item>
    where
        Self: Sized,
    {
        while let Some(x) = self.next() {
            if predicate(&x) {
                return Some(x);
            }
        }
        None
    }

    fn find_map<B, F: FnMut(Self::Item) -> Option<B>>(&mut self, mut f: F) -> Option<B>
    where
        Self: Sized,
    {
        while let Some(x) = self.next() {
            if let Some(y) = f(x) {
                return Some(y);
            }
        }
        None
    }

    fn position<P: FnMut(Self::Item) -> bool>(&mut self, mut predicate: P) -> Option<usize>
    where
        Self: Sized,
    {
        let mut i = 0;
        while let Some(x) = self.next() {
            if predicate(x) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn rposition<P: FnMut(Self::Item) -> bool>(&mut self, mut predicate: P) -> Option<usize>
    where
        Self: Sized + DoubleEndedIterator + ExactSizeIterator,
    {
        let mut i = self.len();
        while let Some(x) = self.next_back() {
            i -= 1;
            if predicate(x) {
                return Some(i);
            }
        }
        None
    }

    fn max(self) -> Option<Self::Item>
    where
        Self: Sized,
        Self::Item: Ord,
    {
        self.max_by(|a, b| a.cmp(b))
    }

    fn min(self) -> Option<Self::Item>
    where
        Self: Sized,
        Self::Item: Ord,
    {
        self.min_by(|a, b| a.cmp(b))
    }

    fn max_by_key<B: Ord, F: FnMut(&Self::Item) -> B>(mut self, mut f: F) -> Option<Self::Item>
    where
        Self: Sized,
    {
        let mut best = self.next()?;
        let mut best_key = f(&best);
        while let Some(x) = self.next() {
            let k = f(&x);
            if k >= best_key {
                best = x;
                best_key = k;
            }
        }
        Some(best)
    }

    fn max_by<F: FnMut(&Self::Item, &Self::Item) -> Ordering>(mut self, mut compare: F) -> Option<Self::Item>
    where
        Self: Sized,
    {
        let mut best = self.next()?;
        while let Some(x) = self.next() {
            match compare(&best, &x) {
                Ordering::Greater => {}
                _ => best = x,
            }
        }
        Some(best)
    }

    fn min_by_key<B: Ord, F: FnMut(&Self::Item) -> B>(mut self, mut f: F) -> Option<Self::Item>
    where
        Self: Sized,
    {
        let mut best = self.next()?;
        let mut best_key = f(&best);
        while let Some(x) = self.next() {
            let k = f(&x);
            if k < best_key {
                best = x;
                best_key = k;
            }
        }
        Some(best)
    }

    fn min_by<F: FnMut(&Self::Item, &Self::Item) -> Ordering>(mut self, mut compare: F) -> Option<Self::Item>
    where
        Self: Sized,
    {
        let mut best = self.next()?;
        while let Some(x) = self.next() {
            match compare(&best, &x) {
                Ordering::Greater => best = x,
                _ => {}
            }
        }
        Some(best)
    }

    fn rev(self) -> Rev<Self>
    where
        Self: Sized + DoubleEndedIterator,
    {
        Rev { iter: self }
    }

    fn unzip<A, B, FromA: Default + Extend<A>, FromB: Default + Extend<B>>(mut self) -> (FromA, FromB)
    where
        Self: Sized + Iterator<Item = (A, B)>,
    {
        let mut a: FromA = Default::default();
        let mut b: FromB = Default::default();
        while let Some((x, y)) = self.next() {
            a.extend_one(x);
            b.extend_one(y);
        }
        (a, b)
    }

    fn copied<'a, T: 'a + Copy>(self) -> Copied<Self>
    where
        Self: Sized + Iterator<Item = &'a T>,
    {
        Copied { iter: self }
    }

    fn cloned<'a, T: 'a + Clone>(self) -> Cloned<Self>
    where
        Self: Sized + Iterator<Item = &'a T>,
    {
        Cloned { iter: self }
    }

    fn cycle(self) -> Cycle<Self>
    where
        Self: Sized + Clone,
    {
        Cycle {
            orig: self.clone(),
            iter: self,
        }
    }

    fn sum<S: Sum<Self::Item>>(self) -> S
    where
        Self: Sized,
    {
        S::sum(self)
    }

    fn product<P: Product<Self::Item>>(self) -> P
    where
        Self: Sized,
    {
        P::product(self)
    }

    fn cmp<I: IntoIterator<Item = Self::Item>>(mut self, other: I) -> Ordering
    where
        Self::Item: Ord,
        Self: Sized,
    {
        let mut other = other.into_iter();
        loop {
            match (self.next(), other.next()) {
                (None, None) => return Ordering::Equal,
                (None, _) => return Ordering::Less,
                (_, None) => return Ordering::Greater,
                (Some(x), Some(y)) => match x.cmp(&y) {
                    Ordering::Equal => {}
                    c => return c,
                },
            }
        }
    }

    fn eq<I: IntoIterator>(mut self, other: I) -> bool
    where
        Self::Item: PartialEq<I::Item>,
        Self: Sized,
    {
        let mut other = other.into_iter();
        loop {
            match (self.next(), other.next()) {
                (None, None) => return true,
                (Some(x), Some(y)) => {
                    if x != y {
                        return false;
                    }
                }
                _ => return false,
            }
        }
    }

    fn is_sorted(mut self) -> bool
    where
        Self: Sized,
        Self::Item: PartialOrd,
    {
        let mut last = match self.next() {
            Some(x) => x,
            None => return true,
        };
        while let Some(x) = self.next() {
            if last > x {
                return false;
            }
            last = x;
        }
        true
    }
}

pub trait DoubleEndedIterator: Iterator {
    fn next_back(&mut self) -> Option<Self::Item>;

    fn nth_back(&mut self, mut n: usize) -> Option<Self::Item> {
        while let Some(x) = self.next_back() {
            if n == 0 {
                return Some(x);
            }
            n -= 1;
        }
        None
    }

    fn rfind<P: FnMut(&Self::Item) -> bool>(&mut self, mut predicate: P) -> Option<Self::Item>
    where
        Self: Sized,
    {
        while let Some(x) = self.next_back() {
            if predicate(&x) {
                return Some(x);
            }
        }
        None
    }

    fn rfold<B, F: FnMut(B, Self::Item) -> B>(mut self, init: B, mut f: F) -> B
    where
        Self: Sized,
    {
        let mut acc = init;
        while let Some(x) = self.next_back() {
            acc = f(acc, x);
        }
        acc
    }
}

pub trait ExactSizeIterator: Iterator {
    fn len(&self) -> usize {
        let (lo, _) = self.size_hint();
        lo
    }
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub trait FusedIterator: Iterator {}

pub trait IntoIterator {
    type Item;
    type IntoIter: Iterator<Item = Self::Item>;
    fn into_iter(self) -> Self::IntoIter;
}

impl<I: Iterator> IntoIterator for I {
    type Item = I::Item;
    type IntoIter = I;
    fn into_iter(self) -> I {
        self
    }
}

pub trait FromIterator<A>: Sized {
    fn from_iter<T: IntoIterator<Item = A>>(iter: T) -> Self;
}

pub trait Extend<A> {
    fn extend<T: IntoIterator<Item = A>>(&mut self, iter: T);
    fn extend_one(&mut self, item: A) {
        self.extend(Some(item));
    }
}

pub trait Sum<A = Self>: Sized {
    fn sum<I: Iterator<Item = A>>(iter: I) -> Self;
}

pub trait Product<A = Self>: Sized {
    fn product<I: Iterator<Item = A>>(iter: I) -> Self;
}

macro_rules! num_sum {
    ($($t:ty, $zero:expr, $one:expr;)*) => {$(
        impl Sum for $t {
            fn sum<I: Iterator<Item = $t>>(iter: I) -> $t {
                iter.fold($zero, |a, b| a + b)
            }
        }
        impl<'a> Sum<&'a $t> for $t {
            fn sum<I: Iterator<Item = &'a $t>>(iter: I) -> $t {
                iter.fold($zero, |a, b| a + *b)
            }
        }
        impl Product for $t {
            fn product<I: Iterator<Item = $t>>(iter: I) -> $t {
                iter.fold($one, |a, b| a * b)
            }
        }
        impl<'a> Product<&'a $t> for $t {
            fn product<I: Iterator<Item = &'a $t>>(iter: I) -> $t {
                iter.fold($one, |a, b| a * *b)
            }
        }
    )*};
}
num_sum! {
    i8, 0, 1; i16, 0, 1; i32, 0, 1; i64, 0, 1; isize, 0, 1;
    u8, 0, 1; u16, 0, 1; u32, 0, 1; u64, 0, 1; usize, 0, 1;
    f32, 0.0, 1.0; f64, 0.0, 1.0;
}

impl<I: Iterator + ?Sized> Iterator for &mut I {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        (**self).next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (**self).size_hint()
    }
}

impl<I: DoubleEndedIterator + ?Sized> DoubleEndedIterator for &mut I {
    fn next_back(&mut self) -> Option<I::Item> {
        (**self).next_back()
    }
}

// ---- adapters ----

#[derive(Clone)]
pub struct Map<I, F> {
    iter: I,
    f: F,
}

impl<B, I: Iterator, F: FnMut(I::Item) -> B> Iterator for Map<I, F> {
    type Item = B;
    fn next(&mut self) -> Option<B> {
        match self.iter.next() {
            Some(x) => Some((self.f)(x)),
            None => None,
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<B, I: DoubleEndedIterator, F: FnMut(I::Item) -> B> DoubleEndedIterator for Map<I, F> {
    fn next_back(&mut self) -> Option<B> {
        match self.iter.next_back() {
            Some(x) => Some((self.f)(x)),
            None => None,
        }
    }
}

impl<B, I: ExactSizeIterator, F: FnMut(I::Item) -> B> ExactSizeIterator for Map<I, F> {}

#[derive(Clone)]
pub struct Filter<I, P> {
    iter: I,
    predicate: P,
}

impl<I: Iterator, P: FnMut(&I::Item) -> bool> Iterator for Filter<I, P> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        while let Some(x) = self.iter.next() {
            if (self.predicate)(&x) {
                return Some(x);
            }
        }
        None
    }
}

impl<I: DoubleEndedIterator, P: FnMut(&I::Item) -> bool> DoubleEndedIterator for Filter<I, P> {
    fn next_back(&mut self) -> Option<I::Item> {
        while let Some(x) = self.iter.next_back() {
            if (self.predicate)(&x) {
                return Some(x);
            }
        }
        None
    }
}

#[derive(Clone)]
pub struct FilterMap<I, F> {
    iter: I,
    f: F,
}

impl<B, I: Iterator, F: FnMut(I::Item) -> Option<B>> Iterator for FilterMap<I, F> {
    type Item = B;
    fn next(&mut self) -> Option<B> {
        while let Some(x) = self.iter.next() {
            if let Some(y) = (self.f)(x) {
                return Some(y);
            }
        }
        None
    }
}

impl<B, I: DoubleEndedIterator, F: FnMut(I::Item) -> Option<B>> DoubleEndedIterator for FilterMap<I, F> {
    fn next_back(&mut self) -> Option<B> {
        while let Some(x) = self.iter.next_back() {
            if let Some(y) = (self.f)(x) {
                return Some(y);
            }
        }
        None
    }
}

#[derive(Clone)]
pub struct Enumerate<I> {
    iter: I,
    count: usize,
}

impl<I: Iterator> Iterator for Enumerate<I> {
    type Item = (usize, I::Item);
    fn next(&mut self) -> Option<(usize, I::Item)> {
        match self.iter.next() {
            Some(x) => {
                let i = self.count;
                self.count += 1;
                Some((i, x))
            }
            None => None,
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<I: DoubleEndedIterator + ExactSizeIterator> DoubleEndedIterator for Enumerate<I> {
    fn next_back(&mut self) -> Option<(usize, I::Item)> {
        let len = self.iter.len();
        match self.iter.next_back() {
            Some(x) => Some((self.count + len - 1, x)),
            None => None,
        }
    }
}

impl<I: ExactSizeIterator> ExactSizeIterator for Enumerate<I> {}

#[derive(Clone)]
pub struct Peekable<I: Iterator> {
    iter: I,
    peeked: Option<Option<I::Item>>,
}

impl<I: Iterator> Iterator for Peekable<I> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        match self.peeked.take() {
            Some(v) => v,
            None => self.iter.next(),
        }
    }
}

impl<I: Iterator> Peekable<I> {
    pub fn peek(&mut self) -> Option<&I::Item> {
        if self.peeked.is_none() {
            self.peeked = Some(self.iter.next());
        }
        match &self.peeked {
            Some(Some(x)) => Some(x),
            _ => None,
        }
    }
    pub fn peek_mut(&mut self) -> Option<&mut I::Item> {
        if self.peeked.is_none() {
            self.peeked = Some(self.iter.next());
        }
        match &mut self.peeked {
            Some(Some(x)) => Some(x),
            _ => None,
        }
    }
    pub fn next_if<F: FnOnce(&I::Item) -> bool>(&mut self, f: F) -> Option<I::Item> {
        let ok = match self.peek() {
            Some(x) => f(x),
            None => false,
        };
        if ok {
            self.next()
        } else {
            None
        }
    }
    pub fn next_if_eq<T: ?Sized>(&mut self, expected: &T) -> Option<I::Item>
    where
        I::Item: PartialEq<T>,
    {
        let ok = match self.peek() {
            Some(x) => *x == *expected,
            None => false,
        };
        if ok {
            self.next()
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub struct SkipWhile<I, P> {
    iter: I,
    flag: bool,
    predicate: P,
}

impl<I: Iterator, P: FnMut(&I::Item) -> bool> Iterator for SkipWhile<I, P> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        while let Some(x) = self.iter.next() {
            if self.flag || !(self.predicate)(&x) {
                self.flag = true;
                return Some(x);
            }
        }
        None
    }
}

#[derive(Clone)]
pub struct TakeWhile<I, P> {
    iter: I,
    flag: bool,
    predicate: P,
}

impl<I: Iterator, P: FnMut(&I::Item) -> bool> Iterator for TakeWhile<I, P> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        if self.flag {
            return None;
        }
        match self.iter.next() {
            Some(x) => {
                if (self.predicate)(&x) {
                    Some(x)
                } else {
                    self.flag = true;
                    None
                }
            }
            None => None,
        }
    }
}

#[derive(Clone)]
pub struct MapWhile<I, P> {
    iter: I,
    predicate: P,
}

impl<B, I: Iterator, P: FnMut(I::Item) -> Option<B>> Iterator for MapWhile<I, P> {
    type Item = B;
    fn next(&mut self) -> Option<B> {
        match self.iter.next() {
            Some(x) => (self.predicate)(x),
            None => None,
        }
    }
}

#[derive(Clone)]
pub struct Skip<I> {
    iter: I,
    n: usize,
}

impl<I: Iterator> Iterator for Skip<I> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        while self.n > 0 {
            self.n -= 1;
            self.iter.next()?;
        }
        self.iter.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (lo, hi) = self.iter.size_hint();
        let lo = if lo > self.n { lo - self.n } else { 0 };
        let hi = match hi {
            Some(h) => Some(if h > self.n { h - self.n } else { 0 }),
            None => None,
        };
        (lo, hi)
    }
}

impl<I: DoubleEndedIterator + ExactSizeIterator> DoubleEndedIterator for Skip<I> {
    fn next_back(&mut self) -> Option<I::Item> {
        if self.iter.len() > self.n {
            self.iter.next_back()
        } else {
            None
        }
    }
}

impl<I: ExactSizeIterator> ExactSizeIterator for Skip<I> {}

#[derive(Clone)]
pub struct Take<I> {
    iter: I,
    n: usize,
}

impl<I: Iterator> Iterator for Take<I> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        if self.n == 0 {
            return None;
        }
        self.n -= 1;
        self.iter.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (lo, hi) = self.iter.size_hint();
        let lo = if lo < self.n { lo } else { self.n };
        let hi = match hi {
            Some(h) => Some(if h < self.n { h } else { self.n }),
            None => Some(self.n),
        };
        (lo, hi)
    }
}

impl<I: DoubleEndedIterator + ExactSizeIterator> DoubleEndedIterator for Take<I> {
    fn next_back(&mut self) -> Option<I::Item> {
        if self.n == 0 {
            return None;
        }
        while self.iter.len() > self.n {
            self.iter.next_back()?;
        }
        self.n -= 1;
        self.iter.next_back()
    }
}

impl<I: ExactSizeIterator> ExactSizeIterator for Take<I> {}

#[derive(Clone)]
pub struct Scan<I, St, F> {
    iter: I,
    state: St,
    f: F,
}

impl<B, I: Iterator, St, F: FnMut(&mut St, I::Item) -> Option<B>> Iterator for Scan<I, St, F> {
    type Item = B;
    fn next(&mut self) -> Option<B> {
        let x = self.iter.next()?;
        (self.f)(&mut self.state, x)
    }
}

pub struct FlatMap<I, U: IntoIterator, F> {
    iter: I,
    f: F,
    front: Option<U::IntoIter>,
    back: Option<U::IntoIter>,
}

impl<I: Iterator, U: IntoIterator, F: FnMut(I::Item) -> U> Iterator for FlatMap<I, U, F> {
    type Item = U::Item;
    fn next(&mut self) -> Option<U::Item> {
        loop {
            if let Some(inner) = &mut self.front {
                if let Some(x) = inner.next() {
                    return Some(x);
                }
                self.front = None;
            }
            match self.iter.next() {
                Some(x) => self.front = Some((self.f)(x).into_iter()),
                None => {
                    return match &mut self.back {
                        Some(b) => b.next(),
                        None => None,
                    }
                }
            }
        }
    }
}

impl<I: DoubleEndedIterator, U: IntoIterator, F: FnMut(I::Item) -> U> DoubleEndedIterator for FlatMap<I, U, F>
where
    U::IntoIter: DoubleEndedIterator,
{
    fn next_back(&mut self) -> Option<U::Item> {
        loop {
            if let Some(inner) = &mut self.back {
                if let Some(x) = inner.next_back() {
                    return Some(x);
                }
                self.back = None;
            }
            match self.iter.next_back() {
                Some(x) => self.back = Some((self.f)(x).into_iter()),
                None => {
                    return match &mut self.front {
                        Some(b) => b.next_back(),
                        None => None,
                    }
                }
            }
        }
    }
}

pub struct Flatten<I: Iterator>
where
    I::Item: IntoIterator,
{
    iter: I,
    front: Option<<I::Item as IntoIterator>::IntoIter>,
    back: Option<<I::Item as IntoIterator>::IntoIter>,
}

impl<I: Iterator> Iterator for Flatten<I>
where
    I::Item: IntoIterator,
{
    type Item = <I::Item as IntoIterator>::Item;
    fn next(&mut self) -> Option<<I::Item as IntoIterator>::Item> {
        loop {
            if let Some(inner) = &mut self.front {
                if let Some(x) = inner.next() {
                    return Some(x);
                }
                self.front = None;
            }
            match self.iter.next() {
                Some(x) => self.front = Some(x.into_iter()),
                None => {
                    return match &mut self.back {
                        Some(b) => b.next(),
                        None => None,
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct Fuse<I> {
    iter: Option<I>,
}

impl<I: Iterator> Iterator for Fuse<I> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        let r = match &mut self.iter {
            Some(it) => it.next(),
            None => return None,
        };
        if r.is_none() {
            self.iter = None;
        }
        r
    }
}

#[derive(Clone)]
pub struct Inspect<I, F> {
    iter: I,
    f: F,
}

impl<I: Iterator, F: FnMut(&I::Item)> Iterator for Inspect<I, F> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        let x = self.iter.next()?;
        (self.f)(&x);
        Some(x)
    }
}

#[derive(Clone)]
pub struct Rev<I> {
    iter: I,
}

impl<I: DoubleEndedIterator> Iterator for Rev<I> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        self.iter.next_back()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<I: DoubleEndedIterator> DoubleEndedIterator for Rev<I> {
    fn next_back(&mut self) -> Option<I::Item> {
        self.iter.next()
    }
}

impl<I: DoubleEndedIterator + ExactSizeIterator> ExactSizeIterator for Rev<I> {}

#[derive(Clone)]
pub struct Zip<A, B> {
    a: A,
    b: B,
}

impl<A: Iterator, B: Iterator> Iterator for Zip<A, B> {
    type Item = (A::Item, B::Item);
    fn next(&mut self) -> Option<(A::Item, B::Item)> {
        let x = self.a.next()?;
        let y = self.b.next()?;
        Some((x, y))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (a, _) = self.a.size_hint();
        let (b, _) = self.b.size_hint();
        let n = if a < b { a } else { b };
        (n, Some(n))
    }
}

impl<A: DoubleEndedIterator + ExactSizeIterator, B: DoubleEndedIterator + ExactSizeIterator> DoubleEndedIterator for Zip<A, B> {
    fn next_back(&mut self) -> Option<(A::Item, B::Item)> {
        while self.a.len() > self.b.len() {
            self.a.next_back();
        }
        while self.b.len() > self.a.len() {
            self.b.next_back();
        }
        let x = self.a.next_back()?;
        let y = self.b.next_back()?;
        Some((x, y))
    }
}

impl<A: ExactSizeIterator, B: ExactSizeIterator> ExactSizeIterator for Zip<A, B> {}

pub fn zip<A: IntoIterator, B: IntoIterator>(a: A, b: B) -> Zip<A::IntoIter, B::IntoIter> {
    Zip {
        a: a.into_iter(),
        b: b.into_iter(),
    }
}

#[derive(Clone)]
pub struct Chain<A, B> {
    a: Option<A>,
    b: Option<B>,
}

impl<A: Iterator, B: Iterator<Item = A::Item>> Iterator for Chain<A, B> {
    type Item = A::Item;
    fn next(&mut self) -> Option<A::Item> {
        if let Some(a) = &mut self.a {
            match a.next() {
                Some(x) => return Some(x),
                None => self.a = None,
            }
        }
        match &mut self.b {
            Some(b) => b.next(),
            None => None,
        }
    }
}

impl<A: DoubleEndedIterator, B: DoubleEndedIterator<Item = A::Item>> DoubleEndedIterator for Chain<A, B> {
    fn next_back(&mut self) -> Option<A::Item> {
        if let Some(b) = &mut self.b {
            match b.next_back() {
                Some(x) => return Some(x),
                None => self.b = None,
            }
        }
        match &mut self.a {
            Some(a) => a.next_back(),
            None => None,
        }
    }
}

#[derive(Clone)]
pub struct StepBy<I> {
    iter: I,
    step: usize,
    first: bool,
}

impl<I: Iterator> Iterator for StepBy<I> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        if self.first {
            self.first = false;
            self.iter.next()
        } else {
            self.iter.nth(self.step)
        }
    }
}

#[derive(Clone)]
pub struct Copied<I> {
    iter: I,
}

impl<'a, T: 'a + Copy, I: Iterator<Item = &'a T>> Iterator for Copied<I> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        match self.iter.next() {
            Some(x) => Some(*x),
            None => None,
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a, T: 'a + Copy, I: DoubleEndedIterator<Item = &'a T>> DoubleEndedIterator for Copied<I> {
    fn next_back(&mut self) -> Option<T> {
        match self.iter.next_back() {
            Some(x) => Some(*x),
            None => None,
        }
    }
}

impl<'a, T: 'a + Copy, I: ExactSizeIterator<Item = &'a T>> ExactSizeIterator for Copied<I> {}

#[derive(Clone)]
pub struct Cloned<I> {
    iter: I,
}

impl<'a, T: 'a + Clone, I: Iterator<Item = &'a T>> Iterator for Cloned<I> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        match self.iter.next() {
            Some(x) => Some(x.clone()),
            None => None,
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a, T: 'a + Clone, I: DoubleEndedIterator<Item = &'a T>> DoubleEndedIterator for Cloned<I> {
    fn next_back(&mut self) -> Option<T> {
        match self.iter.next_back() {
            Some(x) => Some(x.clone()),
            None => None,
        }
    }
}

#[derive(Clone)]
pub struct Cycle<I> {
    orig: I,
    iter: I,
}

impl<I: Iterator + Clone> Iterator for Cycle<I> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        match self.iter.next() {
            Some(x) => Some(x),
            None => {
                self.iter = self.orig.clone();
                self.iter.next()
            }
        }
    }
}

// ---- sources ----

#[derive(Clone)]
pub struct Repeat<T> {
    value: T,
}

impl<T: Clone> Iterator for Repeat<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        Some(self.value.clone())
    }
}

pub fn repeat<T: Clone>(value: T) -> Repeat<T> {
    Repeat { value }
}

pub fn repeat_n<T: Clone>(value: T, n: usize) -> Take<Repeat<T>> {
    Repeat { value }.take(n)
}

#[derive(Clone)]
pub struct Once<T> {
    value: Option<T>,
}

impl<T> Iterator for Once<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        self.value.take()
    }
}

impl<T> DoubleEndedIterator for Once<T> {
    fn next_back(&mut self) -> Option<T> {
        self.value.take()
    }
}

pub fn once<T>(value: T) -> Once<T> {
    Once { value: Some(value) }
}

pub struct Empty<T> {
    marker: crate::marker::PhantomData<T>,
}

impl<T> Iterator for Empty<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        None
    }
}

pub fn empty<T>() -> Empty<T> {
    Empty {
        marker: crate::marker::PhantomData,
    }
}

pub struct FromFn<F> {
    f: F,
}

impl<T, F: FnMut() -> Option<T>> Iterator for FromFn<F> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        (self.f)()
    }
}

pub fn from_fn<T, F: FnMut() -> Option<T>>(f: F) -> FromFn<F> {
    FromFn { f }
}

pub struct Successors<T, F> {
    next: Option<T>,
    f: F,
}

impl<T, F: FnMut(&T) -> Option<T>> Iterator for Successors<T, F> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        let item = self.next.take()?;
        self.next = (self.f)(&item);
        Some(item)
    }
}

pub fn successors<T, F: FnMut(&T) -> Option<T>>(first: Option<T>, succ: F) -> Successors<T, F> {
    Successors { next: first, f: succ }
}

// ---- ranges ----

/// Types a range can iterate over.
pub trait Step: Clone + PartialOrd + Sized {
    /// The number of steps from `start` to `end` (`None` if too many).
    fn steps_between(start: &Self, end: &Self) -> usize;
    fn forward(start: Self, n: usize) -> Self;
    fn backward(start: Self, n: usize) -> Self;
}

macro_rules! int_step {
    ($($t:ty)*) => {$(
        impl Step for $t {
            fn steps_between(start: &$t, end: &$t) -> usize {
                if *start < *end {
                    (*end as i64).wrapping_sub(*start as i64) as usize
                } else {
                    0
                }
            }
            fn forward(start: $t, n: usize) -> $t {
                start + n as $t
            }
            fn backward(start: $t, n: usize) -> $t {
                start - n as $t
            }
        }
    )*};
}
int_step!(i8 i16 i32 i64 isize u8 u16 u32 u64 usize);

impl Step for char {
    fn steps_between(start: &char, end: &char) -> usize {
        if *start < *end {
            (*end as u32 - *start as u32) as usize
        } else {
            0
        }
    }
    fn forward(start: char, n: usize) -> char {
        let mut v = start as u32 + n as u32;
        if start as u32 <= 0xD7FF && v > 0xD7FF {
            v += 0x800;
        }
        crate::char::from_u32(v).unwrap()
    }
    fn backward(start: char, n: usize) -> char {
        let mut v = start as u32 - n as u32;
        if start as u32 >= 0xE000 && v < 0xE000 {
            v -= 0x800;
        }
        crate::char::from_u32(v).unwrap()
    }
}

impl<A: Step> Iterator for crate::ops::Range<A> {
    type Item = A;
    fn next(&mut self) -> Option<A> {
        if self.start < self.end {
            let x = self.start.clone();
            self.start = Step::forward(x.clone(), 1);
            Some(x)
        } else {
            None
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = Step::steps_between(&self.start, &self.end);
        (n, Some(n))
    }
    fn nth(&mut self, n: usize) -> Option<A> {
        if n < Step::steps_between(&self.start, &self.end) {
            self.start = Step::forward(self.start.clone(), n);
            self.next()
        } else {
            self.start = self.end.clone();
            None
        }
    }
}

impl<A: Step> DoubleEndedIterator for crate::ops::Range<A> {
    fn next_back(&mut self) -> Option<A> {
        if self.start < self.end {
            self.end = Step::backward(self.end.clone(), 1);
            Some(self.end.clone())
        } else {
            None
        }
    }
}

impl<A: Step> ExactSizeIterator for crate::ops::Range<A> {}

impl<A: Step> Iterator for crate::ops::RangeInclusive<A> {
    type Item = A;
    fn next(&mut self) -> Option<A> {
        if self.exhausted || self.start > self.end {
            return None;
        }
        let x = self.start.clone();
        if self.start < self.end {
            self.start = Step::forward(x.clone(), 1);
        } else {
            self.exhausted = true;
        }
        Some(x)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = if self.exhausted || self.start > self.end { 0 } else { Step::steps_between(&self.start, &self.end) + 1 };
        (n, Some(n))
    }
}

impl<A: Step> DoubleEndedIterator for crate::ops::RangeInclusive<A> {
    fn next_back(&mut self) -> Option<A> {
        if self.exhausted || self.start > self.end {
            return None;
        }
        let x = self.end.clone();
        if self.start < self.end {
            self.end = Step::backward(x.clone(), 1);
        } else {
            self.exhausted = true;
        }
        Some(x)
    }
}

impl<A: Step> ExactSizeIterator for crate::ops::RangeInclusive<A> {}

impl<A: Step> Iterator for crate::ops::RangeFrom<A> {
    type Item = A;
    fn next(&mut self) -> Option<A> {
        let x = self.start.clone();
        self.start = Step::forward(x.clone(), 1);
        Some(x)
    }
}
