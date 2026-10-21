//! Collections: `HashMap`, `HashSet`, `BTreeMap`, `BTreeSet`, `VecDeque`
//! and `BinaryHeap`.
//!
//! `HashMap` keeps entries in insertion order with an open-addressing
//! index; `BTreeMap` is a sorted vector (fine for the sizes programs here
//! use).

pub use self::binary_heap::BinaryHeap;
pub use self::btree_map::BTreeMap;
pub use self::btree_set::BTreeSet;
pub use self::hash_map::HashMap;
pub use self::hash_set::HashSet;
pub use self::vec_deque::VecDeque;

pub mod hash_map {
    use crate::borrow::Borrow;
    use crate::fmt;
    use crate::hash::{BuildHasher, Hash, Hasher, RandomState};
    use crate::option::Option::{self, None, Some};
    use crate::vec::Vec;

    const EMPTY: usize = usize::MAX;
    const DELETED: usize = usize::MAX - 1;

    #[derive(Clone)]
    struct Bucket<K, V> {
        hash: u64,
        key: K,
        value: V,
    }

    /// The entries and the index; independent of the hasher.
    #[derive(Clone)]
    struct Raw<K, V> {
        entries: Vec<Bucket<K, V>>,
        table: Vec<usize>,
        used: usize,
    }

    impl<K, V> Raw<K, V> {
        fn new() -> Raw<K, V> {
            Raw {
                entries: Vec::new(),
                table: Vec::new(),
                used: 0,
            }
        }

        fn find<Q: ?Sized + Eq>(&self, hash: u64, key: &Q) -> Option<(usize, usize)>
        where
            K: Borrow<Q>,
        {
            if self.table.is_empty() {
                return None;
            }
            let mask = self.table.len() - 1;
            let mut slot = (hash as usize) & mask;
            loop {
                let e = self.table[slot];
                if e == EMPTY {
                    return None;
                }
                if e != DELETED {
                    let b = &self.entries[e];
                    if b.hash == hash && <K as Borrow<Q>>::borrow(&b.key) == key {
                        return Some((slot, e));
                    }
                }
                slot = (slot + 1) & mask;
            }
        }

        fn rebuild(&mut self, size: usize) {
            self.table = crate::vec::from_elem(EMPTY, size);
            let mask = size - 1;
            let mut i = 0;
            while i < self.entries.len() {
                let mut slot = (self.entries[i].hash as usize) & mask;
                while self.table[slot] != EMPTY {
                    slot = (slot + 1) & mask;
                }
                self.table[slot] = i;
                i += 1;
            }
            self.used = self.entries.len();
        }

        fn insert_new(&mut self, hash: u64, key: K, value: V) -> usize {
            if (self.used + 1) * 2 > self.table.len() {
                let mut size = 8;
                while size < (self.entries.len() + 1) * 4 {
                    size *= 2;
                }
                self.rebuild(size);
            }
            let mask = self.table.len() - 1;
            let mut slot = (hash as usize) & mask;
            while self.table[slot] != EMPTY && self.table[slot] != DELETED {
                slot = (slot + 1) & mask;
            }
            if self.table[slot] == EMPTY {
                self.used += 1;
            }
            let idx = self.entries.len();
            self.table[slot] = idx;
            self.entries.push(Bucket { hash, key, value });
            idx
        }

        /// Removes the entry at `idx` (found in `slot`), keeping the
        /// order of the other entries.
        fn remove_at(&mut self, slot: usize, idx: usize) -> (K, V) {
            self.table[slot] = DELETED;
            let b = self.entries.remove(idx);
            // Entries after `idx` moved down by one.
            let mut s = 0;
            while s < self.table.len() {
                let e = self.table[s];
                if e != EMPTY && e != DELETED && e > idx {
                    self.table[s] = e - 1;
                }
                s += 1;
            }
            (b.key, b.value)
        }
    }

    pub struct HashMap<K, V, S = RandomState> {
        raw: Raw<K, V>,
        hasher: S,
    }

    impl<K, V> HashMap<K, V, RandomState> {
        pub fn new() -> HashMap<K, V, RandomState> {
            HashMap {
                raw: Raw::new(),
                hasher: RandomState,
            }
        }
        pub fn with_capacity(_n: usize) -> HashMap<K, V, RandomState> {
            HashMap::new()
        }
    }

    impl<K, V, S> HashMap<K, V, S> {
        pub fn with_hasher(hasher: S) -> HashMap<K, V, S> {
            HashMap { raw: Raw::new(), hasher }
        }
        pub fn with_capacity_and_hasher(_n: usize, hasher: S) -> HashMap<K, V, S> {
            HashMap { raw: Raw::new(), hasher }
        }
        pub fn len(&self) -> usize {
            self.raw.entries.len()
        }
        pub fn is_empty(&self) -> bool {
            self.raw.entries.is_empty()
        }
        pub fn capacity(&self) -> usize {
            self.raw.table.len() / 2
        }
        pub fn clear(&mut self) {
            self.raw = Raw::new();
        }
        pub fn iter(&self) -> Iter<'_, K, V> {
            Iter {
                inner: self.raw.entries.iter(),
            }
        }
        pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
            IterMut {
                inner: self.raw.entries.iter_mut(),
            }
        }
        pub fn keys(&self) -> Keys<'_, K, V> {
            Keys { inner: self.iter() }
        }
        pub fn values(&self) -> Values<'_, K, V> {
            Values { inner: self.iter() }
        }
        pub fn values_mut(&mut self) -> ValuesMut<'_, K, V> {
            ValuesMut { inner: self.iter_mut() }
        }
        pub fn into_keys(self) -> crate::vec::IntoIter<K> {
            let mut v = Vec::new();
            for b in self.raw.entries {
                v.push(b.key);
            }
            v.into_iter()
        }
        pub fn into_values(self) -> crate::vec::IntoIter<V> {
            let mut v = Vec::new();
            for b in self.raw.entries {
                v.push(b.value);
            }
            v.into_iter()
        }
        pub fn drain(&mut self) -> IntoIter<K, V> {
            let raw = crate::mem::replace(&mut self.raw, Raw::new());
            IntoIter {
                inner: raw.entries.into_iter(),
            }
        }
        pub fn retain<F: FnMut(&K, &mut V) -> bool>(&mut self, mut f: F) {
            let old = crate::mem::replace(&mut self.raw.entries, Vec::new());
            for mut b in old {
                if f(&b.key, &mut b.value) {
                    self.raw.entries.push(b);
                }
            }
            let mut size = 8;
            while size < (self.raw.entries.len() + 1) * 4 {
                size *= 2;
            }
            self.raw.rebuild(size);
        }
        pub fn hasher(&self) -> &S {
            &self.hasher
        }
    }

    impl<K: Hash + Eq, V, S: BuildHasher> HashMap<K, V, S> {
        fn hash_of<Q: ?Sized + Hash>(&self, k: &Q) -> u64 {
            let mut h = self.hasher.build_hasher();
            k.hash(&mut h);
            h.finish()
        }
        pub fn insert(&mut self, k: K, v: V) -> Option<V> {
            let hash = self.hash_of(&k);
            match self.raw.find(hash, &k) {
                Some((_, i)) => Some(crate::mem::replace(&mut self.raw.entries[i].value, v)),
                None => {
                    self.raw.insert_new(hash, k, v);
                    None
                }
            }
        }
        pub fn get<Q: ?Sized + Hash + Eq>(&self, k: &Q) -> Option<&V>
        where
            K: Borrow<Q>,
        {
            let hash = self.hash_of(k);
            match self.raw.find(hash, k) {
                Some((_, i)) => Some(&self.raw.entries[i].value),
                None => None,
            }
        }
        pub fn get_key_value<Q: ?Sized + Hash + Eq>(&self, k: &Q) -> Option<(&K, &V)>
        where
            K: Borrow<Q>,
        {
            let hash = self.hash_of(k);
            match self.raw.find(hash, k) {
                Some((_, i)) => {
                    let b = &self.raw.entries[i];
                    Some((&b.key, &b.value))
                }
                None => None,
            }
        }
        pub fn get_mut<Q: ?Sized + Hash + Eq>(&mut self, k: &Q) -> Option<&mut V>
        where
            K: Borrow<Q>,
        {
            let hash = self.hash_of(k);
            match self.raw.find(hash, k) {
                Some((_, i)) => Some(&mut self.raw.entries[i].value),
                None => None,
            }
        }
        pub fn contains_key<Q: ?Sized + Hash + Eq>(&self, k: &Q) -> bool
        where
            K: Borrow<Q>,
        {
            let hash = self.hash_of(k);
            self.raw.find(hash, k).is_some()
        }
        pub fn remove<Q: ?Sized + Hash + Eq>(&mut self, k: &Q) -> Option<V>
        where
            K: Borrow<Q>,
        {
            let hash = self.hash_of(k);
            match self.raw.find(hash, k) {
                Some((slot, i)) => {
                    let (_, v) = self.raw.remove_at(slot, i);
                    Some(v)
                }
                None => None,
            }
        }
        pub fn remove_entry<Q: ?Sized + Hash + Eq>(&mut self, k: &Q) -> Option<(K, V)>
        where
            K: Borrow<Q>,
        {
            let hash = self.hash_of(k);
            match self.raw.find(hash, k) {
                Some((slot, i)) => Some(self.raw.remove_at(slot, i)),
                None => None,
            }
        }
        pub fn entry(&mut self, key: K) -> Entry<'_, K, V> {
            let hash = self.hash_of(&key);
            match self.raw.find(hash, &key) {
                Some((_, i)) => Entry::Occupied(OccupiedEntry {
                    raw: &mut self.raw,
                    index: i,
                    key,
                }),
                None => Entry::Vacant(VacantEntry {
                    raw: &mut self.raw,
                    hash,
                    key,
                }),
            }
        }
        pub fn reserve(&mut self, _n: usize) {}
        pub fn shrink_to_fit(&mut self) {}
    }

    pub enum Entry<'a, K, V> {
        Occupied(OccupiedEntry<'a, K, V>),
        Vacant(VacantEntry<'a, K, V>),
    }

    pub struct OccupiedEntry<'a, K, V> {
        raw: &'a mut Raw<K, V>,
        index: usize,
        key: K,
    }

    pub struct VacantEntry<'a, K, V> {
        raw: &'a mut Raw<K, V>,
        hash: u64,
        key: K,
    }

    impl<'a, K, V> Entry<'a, K, V> {
        pub fn or_insert(self, default: V) -> &'a mut V {
            match self {
                Entry::Occupied(o) => o.into_mut(),
                Entry::Vacant(v) => v.insert(default),
            }
        }
        pub fn or_insert_with<F: FnOnce() -> V>(self, f: F) -> &'a mut V {
            match self {
                Entry::Occupied(o) => o.into_mut(),
                Entry::Vacant(v) => v.insert(f()),
            }
        }
        pub fn or_insert_with_key<F: FnOnce(&K) -> V>(self, f: F) -> &'a mut V {
            match self {
                Entry::Occupied(o) => o.into_mut(),
                Entry::Vacant(v) => {
                    let value = f(&v.key);
                    v.insert(value)
                }
            }
        }
        pub fn or_default(self) -> &'a mut V
        where
            V: Default,
        {
            match self {
                Entry::Occupied(o) => o.into_mut(),
                Entry::Vacant(v) => v.insert(V::default()),
            }
        }
        pub fn and_modify<F: FnOnce(&mut V)>(self, f: F) -> Entry<'a, K, V> {
            match self {
                Entry::Occupied(mut o) => {
                    f(o.get_mut());
                    Entry::Occupied(o)
                }
                Entry::Vacant(v) => Entry::Vacant(v),
            }
        }
        pub fn key(&self) -> &K {
            match self {
                Entry::Occupied(o) => o.key(),
                Entry::Vacant(v) => &v.key,
            }
        }
    }

    impl<'a, K, V> OccupiedEntry<'a, K, V> {
        pub fn key(&self) -> &K {
            &self.raw.entries[self.index].key
        }
        pub fn get(&self) -> &V {
            &self.raw.entries[self.index].value
        }
        pub fn get_mut(&mut self) -> &mut V {
            &mut self.raw.entries[self.index].value
        }
        pub fn into_mut(self) -> &'a mut V {
            &mut self.raw.entries[self.index].value
        }
        pub fn insert(&mut self, value: V) -> V {
            crate::mem::replace(&mut self.raw.entries[self.index].value, value)
        }
        pub fn remove(self) -> V {
            self.remove_entry().1
        }
        pub fn remove_entry(self) -> (K, V) {
            let mut slot = 0;
            while self.raw.table[slot] != self.index {
                slot += 1;
            }
            let _ = &self.key;
            self.raw.remove_at(slot, self.index)
        }
    }

    impl<'a, K, V> VacantEntry<'a, K, V> {
        pub fn key(&self) -> &K {
            &self.key
        }
        pub fn into_key(self) -> K {
            self.key
        }
        pub fn insert(self, value: V) -> &'a mut V {
            let i = self.raw.insert_new(self.hash, self.key, value);
            &mut self.raw.entries[i].value
        }
    }

    pub struct Iter<'a, K, V> {
        inner: crate::slice::Iter<'a, Bucket<K, V>>,
    }

    impl<'a, K, V> Clone for Iter<'a, K, V> {
        fn clone(&self) -> Iter<'a, K, V> {
            Iter { inner: self.inner.clone() }
        }
    }

    impl<'a, K, V> Iterator for Iter<'a, K, V> {
        type Item = (&'a K, &'a V);
        fn next(&mut self) -> Option<(&'a K, &'a V)> {
            match self.inner.next() {
                Some(b) => Some((&b.key, &b.value)),
                None => None,
            }
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            self.inner.size_hint()
        }
    }

    impl<'a, K, V> ExactSizeIterator for Iter<'a, K, V> {}

    pub struct IterMut<'a, K, V> {
        inner: crate::slice::IterMut<'a, Bucket<K, V>>,
    }

    impl<'a, K, V> Iterator for IterMut<'a, K, V> {
        type Item = (&'a K, &'a mut V);
        fn next(&mut self) -> Option<(&'a K, &'a mut V)> {
            match self.inner.next() {
                Some(b) => Some((&b.key, &mut b.value)),
                None => None,
            }
        }
    }

    pub struct Keys<'a, K, V> {
        inner: Iter<'a, K, V>,
    }

    impl<'a, K, V> Iterator for Keys<'a, K, V> {
        type Item = &'a K;
        fn next(&mut self) -> Option<&'a K> {
            match self.inner.next() {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            self.inner.size_hint()
        }
    }

    impl<'a, K, V> ExactSizeIterator for Keys<'a, K, V> {}

    pub struct Values<'a, K, V> {
        inner: Iter<'a, K, V>,
    }

    impl<'a, K, V> Iterator for Values<'a, K, V> {
        type Item = &'a V;
        fn next(&mut self) -> Option<&'a V> {
            match self.inner.next() {
                Some((_, v)) => Some(v),
                None => None,
            }
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            self.inner.size_hint()
        }
    }

    impl<'a, K, V> ExactSizeIterator for Values<'a, K, V> {}

    pub struct ValuesMut<'a, K, V> {
        inner: IterMut<'a, K, V>,
    }

    impl<'a, K, V> Iterator for ValuesMut<'a, K, V> {
        type Item = &'a mut V;
        fn next(&mut self) -> Option<&'a mut V> {
            match self.inner.next() {
                Some((_, v)) => Some(v),
                None => None,
            }
        }
    }

    pub struct IntoIter<K, V> {
        inner: crate::vec::IntoIter<Bucket<K, V>>,
    }

    impl<K, V> Iterator for IntoIter<K, V> {
        type Item = (K, V);
        fn next(&mut self) -> Option<(K, V)> {
            match self.inner.next() {
                Some(b) => Some((b.key, b.value)),
                None => None,
            }
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            self.inner.size_hint()
        }
    }

    impl<K, V, S> IntoIterator for HashMap<K, V, S> {
        type Item = (K, V);
        type IntoIter = IntoIter<K, V>;
        fn into_iter(self) -> IntoIter<K, V> {
            IntoIter {
                inner: self.raw.entries.into_iter(),
            }
        }
    }

    impl<'a, K, V, S> IntoIterator for &'a HashMap<K, V, S> {
        type Item = (&'a K, &'a V);
        type IntoIter = Iter<'a, K, V>;
        fn into_iter(self) -> Iter<'a, K, V> {
            self.iter()
        }
    }

    impl<'a, K, V, S> IntoIterator for &'a mut HashMap<K, V, S> {
        type Item = (&'a K, &'a mut V);
        type IntoIter = IterMut<'a, K, V>;
        fn into_iter(self) -> IterMut<'a, K, V> {
            self.iter_mut()
        }
    }

    impl<K: Hash + Eq, V, S: BuildHasher + Default> crate::iter::FromIterator<(K, V)> for HashMap<K, V, S> {
        fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> HashMap<K, V, S> {
            let mut m = HashMap::with_hasher(S::default());
            for (k, v) in iter {
                m.insert(k, v);
            }
            m
        }
    }

    impl<K: Hash + Eq, V, S: BuildHasher> crate::iter::Extend<(K, V)> for HashMap<K, V, S> {
        fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
            for (k, v) in iter {
                self.insert(k, v);
            }
        }
    }

    impl<'a, K: Hash + Eq + Copy, V: Copy, S: BuildHasher> crate::iter::Extend<(&'a K, &'a V)> for HashMap<K, V, S> {
        fn extend<I: IntoIterator<Item = (&'a K, &'a V)>>(&mut self, iter: I) {
            for (k, v) in iter {
                self.insert(*k, *v);
            }
        }
    }

    impl<K: Hash + Eq, V, const N: usize> From<[(K, V); N]> for HashMap<K, V, RandomState> {
        fn from(a: [(K, V); N]) -> HashMap<K, V, RandomState> {
            let mut m = HashMap::new();
            for (k, v) in a {
                m.insert(k, v);
            }
            m
        }
    }

    impl<K: Clone, V: Clone, S: Clone> Clone for HashMap<K, V, S> {
        fn clone(&self) -> HashMap<K, V, S> {
            HashMap {
                raw: self.raw.clone(),
                hasher: self.hasher.clone(),
            }
        }
    }

    impl<K, V, S: Default> Default for HashMap<K, V, S> {
        fn default() -> HashMap<K, V, S> {
            HashMap::with_hasher(S::default())
        }
    }

    impl<K: Hash + Eq, V: PartialEq, S: BuildHasher> PartialEq for HashMap<K, V, S> {
        fn eq(&self, other: &HashMap<K, V, S>) -> bool {
            if self.len() != other.len() {
                return false;
            }
            for (k, v) in self.iter() {
                match other.get(k) {
                    Some(w) => {
                        if *v != *w {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            true
        }
    }

    impl<K: Hash + Eq, V: Eq, S: BuildHasher> Eq for HashMap<K, V, S> {}

    impl<K: fmt::Debug, V: fmt::Debug, S> fmt::Debug for HashMap<K, V, S> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let mut m = f.debug_map();
            for (k, v) in self.iter() {
                m.entry(k, v);
            }
            m.finish()
        }
    }

    impl<'b, K: Hash + Eq + Borrow<Q>, Q: ?Sized + Hash + Eq, V, S: BuildHasher> crate::ops::Index<&'b Q> for HashMap<K, V, S> {
        type Output = V;
        #[track_caller]
        fn index(&self, key: &Q) -> &V {
            match self.get(key) {
                Some(v) => v,
                None => crate::rt::begin_panic_str("key not found in map", crate::intrinsics::caller_location()),
            }
        }
    }
}

pub mod hash_set {
    use crate::borrow::Borrow;
    use crate::collections::hash_map::{self, HashMap};
    use crate::fmt;
    use crate::hash::{BuildHasher, Hash, RandomState};
    use crate::option::Option::{self, None, Some};
    use crate::vec::Vec;

    pub struct HashSet<T, S = RandomState> {
        map: HashMap<T, (), S>,
    }

    impl<T> HashSet<T, RandomState> {
        pub fn new() -> HashSet<T, RandomState> {
            HashSet { map: HashMap::new() }
        }
        pub fn with_capacity(_n: usize) -> HashSet<T, RandomState> {
            HashSet::new()
        }
    }

    impl<T, S> HashSet<T, S> {
        pub fn with_hasher(hasher: S) -> HashSet<T, S> {
            HashSet {
                map: HashMap::with_hasher(hasher),
            }
        }
        pub fn len(&self) -> usize {
            self.map.len()
        }
        pub fn is_empty(&self) -> bool {
            self.map.is_empty()
        }
        pub fn clear(&mut self) {
            self.map.clear();
        }
        pub fn iter(&self) -> Iter<'_, T> {
            Iter { inner: self.map.keys() }
        }
        pub fn drain(&mut self) -> crate::vec::IntoIter<T> {
            let mut v = Vec::new();
            for (k, _) in self.map.drain() {
                v.push(k);
            }
            v.into_iter()
        }
        pub fn retain<F: FnMut(&T) -> bool>(&mut self, mut f: F) {
            self.map.retain(|k, _| f(k));
        }
    }

    impl<T: Hash + Eq, S: BuildHasher> HashSet<T, S> {
        pub fn insert(&mut self, value: T) -> bool {
            if self.map.contains_key(&value) {
                false
            } else {
                self.map.insert(value, ());
                true
            }
        }
        pub fn replace(&mut self, value: T) -> Option<T> {
            let old = self.map.remove_entry(&value);
            self.map.insert(value, ());
            match old {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
        pub fn contains<Q: ?Sized + Hash + Eq>(&self, value: &Q) -> bool
        where
            T: Borrow<Q>,
        {
            self.map.contains_key(value)
        }
        pub fn get<Q: ?Sized + Hash + Eq>(&self, value: &Q) -> Option<&T>
        where
            T: Borrow<Q>,
        {
            match self.map.get_key_value(value) {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
        pub fn remove<Q: ?Sized + Hash + Eq>(&mut self, value: &Q) -> bool
        where
            T: Borrow<Q>,
        {
            self.map.remove(value).is_some()
        }
        pub fn take<Q: ?Sized + Hash + Eq>(&mut self, value: &Q) -> Option<T>
        where
            T: Borrow<Q>,
        {
            match self.map.remove_entry(value) {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
        pub fn is_subset(&self, other: &HashSet<T, S>) -> bool {
            self.iter().all(|x| other.contains(x))
        }
        pub fn is_superset(&self, other: &HashSet<T, S>) -> bool {
            other.is_subset(self)
        }
        pub fn is_disjoint(&self, other: &HashSet<T, S>) -> bool {
            self.iter().all(|x| !other.contains(x))
        }
        pub fn union<'a>(&'a self, other: &'a HashSet<T, S>) -> crate::vec::IntoIter<&'a T> {
            let mut v: Vec<&'a T> = self.iter().collect();
            for x in other.iter() {
                if !self.contains(x) {
                    v.push(x);
                }
            }
            v.into_iter()
        }
        pub fn intersection<'a>(&'a self, other: &'a HashSet<T, S>) -> crate::vec::IntoIter<&'a T> {
            let mut v = Vec::new();
            for x in self.iter() {
                if other.contains(x) {
                    v.push(x);
                }
            }
            v.into_iter()
        }
        pub fn difference<'a>(&'a self, other: &'a HashSet<T, S>) -> crate::vec::IntoIter<&'a T> {
            let mut v = Vec::new();
            for x in self.iter() {
                if !other.contains(x) {
                    v.push(x);
                }
            }
            v.into_iter()
        }
        pub fn symmetric_difference<'a>(&'a self, other: &'a HashSet<T, S>) -> crate::vec::IntoIter<&'a T> {
            let mut v = Vec::new();
            for x in self.iter() {
                if !other.contains(x) {
                    v.push(x);
                }
            }
            for x in other.iter() {
                if !self.contains(x) {
                    v.push(x);
                }
            }
            v.into_iter()
        }
    }

    pub struct Iter<'a, T> {
        inner: hash_map::Keys<'a, T, ()>,
    }

    impl<'a, T> Iterator for Iter<'a, T> {
        type Item = &'a T;
        fn next(&mut self) -> Option<&'a T> {
            self.inner.next()
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            self.inner.size_hint()
        }
    }

    impl<'a, T> ExactSizeIterator for Iter<'a, T> {}

    pub struct IntoIter<T> {
        inner: hash_map::IntoIter<T, ()>,
    }

    impl<T> Iterator for IntoIter<T> {
        type Item = T;
        fn next(&mut self) -> Option<T> {
            match self.inner.next() {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
    }

    impl<T, S> IntoIterator for HashSet<T, S> {
        type Item = T;
        type IntoIter = IntoIter<T>;
        fn into_iter(self) -> IntoIter<T> {
            IntoIter {
                inner: self.map.into_iter(),
            }
        }
    }

    impl<'a, T, S> IntoIterator for &'a HashSet<T, S> {
        type Item = &'a T;
        type IntoIter = Iter<'a, T>;
        fn into_iter(self) -> Iter<'a, T> {
            self.iter()
        }
    }

    impl<T: Hash + Eq, S: BuildHasher + Default> crate::iter::FromIterator<T> for HashSet<T, S> {
        fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> HashSet<T, S> {
            let mut s = HashSet::with_hasher(S::default());
            for x in iter {
                s.insert(x);
            }
            s
        }
    }

    impl<T: Hash + Eq, S: BuildHasher> crate::iter::Extend<T> for HashSet<T, S> {
        fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
            for x in iter {
                self.insert(x);
            }
        }
    }

    impl<'a, T: Hash + Eq + Copy, S: BuildHasher> crate::iter::Extend<&'a T> for HashSet<T, S> {
        fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
            for x in iter {
                self.insert(*x);
            }
        }
    }

    impl<T: Hash + Eq, const N: usize> From<[T; N]> for HashSet<T, RandomState> {
        fn from(a: [T; N]) -> HashSet<T, RandomState> {
            let mut s = HashSet::new();
            for x in a {
                s.insert(x);
            }
            s
        }
    }

    impl<T: Clone, S: Clone> Clone for HashSet<T, S> {
        fn clone(&self) -> HashSet<T, S> {
            HashSet { map: self.map.clone() }
        }
    }

    impl<T, S: Default> Default for HashSet<T, S> {
        fn default() -> HashSet<T, S> {
            HashSet::with_hasher(S::default())
        }
    }

    impl<T: Hash + Eq, S: BuildHasher> PartialEq for HashSet<T, S> {
        fn eq(&self, other: &HashSet<T, S>) -> bool {
            self.len() == other.len() && self.is_subset(other)
        }
    }

    impl<T: Hash + Eq, S: BuildHasher> Eq for HashSet<T, S> {}

    impl<T: fmt::Debug, S> fmt::Debug for HashSet<T, S> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let mut s = f.debug_set();
            for x in self.iter() {
                s.entry(x);
            }
            s.finish()
        }
    }
}

pub mod btree_map {
    use crate::borrow::Borrow;
    use crate::cmp::Ordering;
    use crate::fmt;
    use crate::option::Option::{self, None, Some};
    use crate::vec::Vec;

    #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct BTreeMap<K, V> {
        entries: Vec<(K, V)>,
    }

    impl<K, V> BTreeMap<K, V> {
        pub fn new() -> BTreeMap<K, V> {
            BTreeMap { entries: Vec::new() }
        }
        pub fn len(&self) -> usize {
            self.entries.len()
        }
        pub fn is_empty(&self) -> bool {
            self.entries.is_empty()
        }
        pub fn clear(&mut self) {
            self.entries.clear();
        }
        pub fn iter(&self) -> Iter<'_, K, V> {
            Iter {
                inner: self.entries.iter(),
            }
        }
        pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
            IterMut {
                inner: self.entries.iter_mut(),
            }
        }
        pub fn keys(&self) -> Keys<'_, K, V> {
            Keys { inner: self.iter() }
        }
        pub fn values(&self) -> Values<'_, K, V> {
            Values { inner: self.iter() }
        }
        pub fn values_mut(&mut self) -> ValuesMut<'_, K, V> {
            ValuesMut { inner: self.iter_mut() }
        }
        pub fn into_keys(self) -> crate::vec::IntoIter<K> {
            let mut v = Vec::new();
            for (k, _) in self.entries {
                v.push(k);
            }
            v.into_iter()
        }
        pub fn into_values(self) -> crate::vec::IntoIter<V> {
            let mut v = Vec::new();
            for (_, x) in self.entries {
                v.push(x);
            }
            v.into_iter()
        }
        pub fn first_key_value(&self) -> Option<(&K, &V)> {
            match self.entries.first() {
                Some((k, v)) => Some((k, v)),
                None => None,
            }
        }
        pub fn last_key_value(&self) -> Option<(&K, &V)> {
            match self.entries.last() {
                Some((k, v)) => Some((k, v)),
                None => None,
            }
        }
        pub fn pop_first(&mut self) -> Option<(K, V)> {
            if self.entries.is_empty() {
                None
            } else {
                Some(self.entries.remove(0))
            }
        }
        pub fn pop_last(&mut self) -> Option<(K, V)> {
            self.entries.pop()
        }
        pub fn retain<F: FnMut(&K, &mut V) -> bool>(&mut self, mut f: F) {
            self.entries.retain_mut(|e| f(&e.0, &mut e.1));
        }
    }

    impl<K: Ord, V> BTreeMap<K, V> {
        fn search<Q: ?Sized + Ord>(&self, key: &Q) -> Result<usize, usize>
        where
            K: Borrow<Q>,
        {
            let mut lo = 0;
            let mut hi = self.entries.len();
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                match <K as Borrow<Q>>::borrow(&self.entries[mid].0).cmp(key) {
                    Ordering::Less => lo = mid + 1,
                    Ordering::Greater => hi = mid,
                    Ordering::Equal => return Ok(mid),
                }
            }
            Err(lo)
        }
        pub fn insert(&mut self, k: K, v: V) -> Option<V> {
            match self.search(&k) {
                Ok(i) => Some(crate::mem::replace(&mut self.entries[i].1, v)),
                Err(i) => {
                    self.entries.insert(i, (k, v));
                    None
                }
            }
        }
        pub fn get<Q: ?Sized + Ord>(&self, k: &Q) -> Option<&V>
        where
            K: Borrow<Q>,
        {
            match self.search(k) {
                Ok(i) => Some(&self.entries[i].1),
                Err(_) => None,
            }
        }
        pub fn get_key_value<Q: ?Sized + Ord>(&self, k: &Q) -> Option<(&K, &V)>
        where
            K: Borrow<Q>,
        {
            match self.search(k) {
                Ok(i) => {
                    let e = &self.entries[i];
                    Some((&e.0, &e.1))
                }
                Err(_) => None,
            }
        }
        pub fn get_mut<Q: ?Sized + Ord>(&mut self, k: &Q) -> Option<&mut V>
        where
            K: Borrow<Q>,
        {
            match self.search(k) {
                Ok(i) => Some(&mut self.entries[i].1),
                Err(_) => None,
            }
        }
        pub fn contains_key<Q: ?Sized + Ord>(&self, k: &Q) -> bool
        where
            K: Borrow<Q>,
        {
            self.search(k).is_ok()
        }
        pub fn remove<Q: ?Sized + Ord>(&mut self, k: &Q) -> Option<V>
        where
            K: Borrow<Q>,
        {
            match self.search(k) {
                Ok(i) => Some(self.entries.remove(i).1),
                Err(_) => None,
            }
        }
        pub fn remove_entry<Q: ?Sized + Ord>(&mut self, k: &Q) -> Option<(K, V)>
        where
            K: Borrow<Q>,
        {
            match self.search(k) {
                Ok(i) => Some(self.entries.remove(i)),
                Err(_) => None,
            }
        }
        pub fn entry(&mut self, key: K) -> Entry<'_, K, V> {
            match self.search(&key) {
                Ok(i) => Entry::Occupied(OccupiedEntry { map: self, index: i }),
                Err(i) => Entry::Vacant(VacantEntry { map: self, index: i, key }),
            }
        }
        pub fn range<R: crate::collections::btree_map::RangeBounds<K>>(&self, r: R) -> crate::vec::IntoIter<(&K, &V)> {
            let mut v = Vec::new();
            for (k, x) in self.entries.iter() {
                if r.contains_key(k) {
                    v.push((k, x));
                }
            }
            v.into_iter()
        }
        pub fn split_off(&mut self, key: &K) -> BTreeMap<K, V> {
            let i = match self.search(key) {
                Ok(i) => i,
                Err(i) => i,
            };
            BTreeMap {
                entries: self.entries.split_off(i),
            }
        }
        pub fn append(&mut self, other: &mut BTreeMap<K, V>) {
            for (k, v) in crate::mem::replace(&mut other.entries, Vec::new()) {
                self.insert(k, v);
            }
        }
    }

    /// The range forms accepted by `BTreeMap::range`.
    pub trait RangeBounds<K> {
        fn contains_key(&self, k: &K) -> bool;
    }
    impl<K: Ord> RangeBounds<K> for crate::ops::Range<K> {
        fn contains_key(&self, k: &K) -> bool {
            *k >= self.start && *k < self.end
        }
    }
    impl<K: Ord> RangeBounds<K> for crate::ops::RangeInclusive<K> {
        fn contains_key(&self, k: &K) -> bool {
            *k >= self.start && *k <= self.end
        }
    }
    impl<K: Ord> RangeBounds<K> for crate::ops::RangeFrom<K> {
        fn contains_key(&self, k: &K) -> bool {
            *k >= self.start
        }
    }
    impl<K: Ord> RangeBounds<K> for crate::ops::RangeTo<K> {
        fn contains_key(&self, k: &K) -> bool {
            *k < self.end
        }
    }
    impl<K: Ord> RangeBounds<K> for crate::ops::RangeToInclusive<K> {
        fn contains_key(&self, k: &K) -> bool {
            *k <= self.end
        }
    }
    impl<K: Ord> RangeBounds<K> for crate::ops::RangeFull {
        fn contains_key(&self, _k: &K) -> bool {
            true
        }
    }

    pub enum Entry<'a, K, V> {
        Occupied(OccupiedEntry<'a, K, V>),
        Vacant(VacantEntry<'a, K, V>),
    }

    pub struct OccupiedEntry<'a, K, V> {
        map: &'a mut BTreeMap<K, V>,
        index: usize,
    }

    pub struct VacantEntry<'a, K, V> {
        map: &'a mut BTreeMap<K, V>,
        index: usize,
        key: K,
    }

    impl<'a, K, V> Entry<'a, K, V> {
        pub fn or_insert(self, default: V) -> &'a mut V {
            match self {
                Entry::Occupied(o) => o.into_mut(),
                Entry::Vacant(v) => v.insert(default),
            }
        }
        pub fn or_insert_with<F: FnOnce() -> V>(self, f: F) -> &'a mut V {
            match self {
                Entry::Occupied(o) => o.into_mut(),
                Entry::Vacant(v) => v.insert(f()),
            }
        }
        pub fn or_default(self) -> &'a mut V
        where
            V: Default,
        {
            match self {
                Entry::Occupied(o) => o.into_mut(),
                Entry::Vacant(v) => v.insert(V::default()),
            }
        }
        pub fn and_modify<F: FnOnce(&mut V)>(self, f: F) -> Entry<'a, K, V> {
            match self {
                Entry::Occupied(mut o) => {
                    f(o.get_mut());
                    Entry::Occupied(o)
                }
                Entry::Vacant(v) => Entry::Vacant(v),
            }
        }
        pub fn key(&self) -> &K {
            match self {
                Entry::Occupied(o) => &o.map.entries[o.index].0,
                Entry::Vacant(v) => &v.key,
            }
        }
    }

    impl<'a, K, V> OccupiedEntry<'a, K, V> {
        pub fn get(&self) -> &V {
            &self.map.entries[self.index].1
        }
        pub fn get_mut(&mut self) -> &mut V {
            &mut self.map.entries[self.index].1
        }
        pub fn into_mut(self) -> &'a mut V {
            &mut self.map.entries[self.index].1
        }
        pub fn insert(&mut self, value: V) -> V {
            crate::mem::replace(&mut self.map.entries[self.index].1, value)
        }
        pub fn remove(self) -> V {
            self.map.entries.remove(self.index).1
        }
    }

    impl<'a, K, V> VacantEntry<'a, K, V> {
        pub fn insert(self, value: V) -> &'a mut V {
            self.map.entries.insert(self.index, (self.key, value));
            &mut self.map.entries[self.index].1
        }
    }

    pub struct Iter<'a, K, V> {
        inner: crate::slice::Iter<'a, (K, V)>,
    }

    impl<'a, K, V> Iterator for Iter<'a, K, V> {
        type Item = (&'a K, &'a V);
        fn next(&mut self) -> Option<(&'a K, &'a V)> {
            match self.inner.next() {
                Some(e) => Some((&e.0, &e.1)),
                None => None,
            }
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            self.inner.size_hint()
        }
    }

    impl<'a, K, V> DoubleEndedIterator for Iter<'a, K, V> {
        fn next_back(&mut self) -> Option<(&'a K, &'a V)> {
            match self.inner.next_back() {
                Some(e) => Some((&e.0, &e.1)),
                None => None,
            }
        }
    }

    impl<'a, K, V> ExactSizeIterator for Iter<'a, K, V> {}

    pub struct IterMut<'a, K, V> {
        inner: crate::slice::IterMut<'a, (K, V)>,
    }

    impl<'a, K, V> Iterator for IterMut<'a, K, V> {
        type Item = (&'a K, &'a mut V);
        fn next(&mut self) -> Option<(&'a K, &'a mut V)> {
            match self.inner.next() {
                Some(e) => Some((&e.0, &mut e.1)),
                None => None,
            }
        }
    }

    pub struct Keys<'a, K, V> {
        inner: Iter<'a, K, V>,
    }

    impl<'a, K, V> Iterator for Keys<'a, K, V> {
        type Item = &'a K;
        fn next(&mut self) -> Option<&'a K> {
            match self.inner.next() {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
    }

    impl<'a, K, V> DoubleEndedIterator for Keys<'a, K, V> {
        fn next_back(&mut self) -> Option<&'a K> {
            match self.inner.next_back() {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
    }

    pub struct Values<'a, K, V> {
        inner: Iter<'a, K, V>,
    }

    impl<'a, K, V> Iterator for Values<'a, K, V> {
        type Item = &'a V;
        fn next(&mut self) -> Option<&'a V> {
            match self.inner.next() {
                Some((_, v)) => Some(v),
                None => None,
            }
        }
    }

    impl<'a, K, V> DoubleEndedIterator for Values<'a, K, V> {
        fn next_back(&mut self) -> Option<&'a V> {
            match self.inner.next_back() {
                Some((_, v)) => Some(v),
                None => None,
            }
        }
    }

    pub struct ValuesMut<'a, K, V> {
        inner: IterMut<'a, K, V>,
    }

    impl<'a, K, V> Iterator for ValuesMut<'a, K, V> {
        type Item = &'a mut V;
        fn next(&mut self) -> Option<&'a mut V> {
            match self.inner.next() {
                Some((_, v)) => Some(v),
                None => None,
            }
        }
    }

    impl<K, V> IntoIterator for BTreeMap<K, V> {
        type Item = (K, V);
        type IntoIter = crate::vec::IntoIter<(K, V)>;
        fn into_iter(self) -> crate::vec::IntoIter<(K, V)> {
            self.entries.into_iter()
        }
    }

    impl<'a, K, V> IntoIterator for &'a BTreeMap<K, V> {
        type Item = (&'a K, &'a V);
        type IntoIter = Iter<'a, K, V>;
        fn into_iter(self) -> Iter<'a, K, V> {
            self.iter()
        }
    }

    impl<'a, K, V> IntoIterator for &'a mut BTreeMap<K, V> {
        type Item = (&'a K, &'a mut V);
        type IntoIter = IterMut<'a, K, V>;
        fn into_iter(self) -> IterMut<'a, K, V> {
            self.iter_mut()
        }
    }

    impl<K: Ord, V> crate::iter::FromIterator<(K, V)> for BTreeMap<K, V> {
        fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> BTreeMap<K, V> {
            let mut m = BTreeMap::new();
            for (k, v) in iter {
                m.insert(k, v);
            }
            m
        }
    }

    impl<K: Ord, V> crate::iter::Extend<(K, V)> for BTreeMap<K, V> {
        fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
            for (k, v) in iter {
                self.insert(k, v);
            }
        }
    }

    impl<K: Ord, V, const N: usize> From<[(K, V); N]> for BTreeMap<K, V> {
        fn from(a: [(K, V); N]) -> BTreeMap<K, V> {
            let mut m = BTreeMap::new();
            for (k, v) in a {
                m.insert(k, v);
            }
            m
        }
    }

    impl<K, V> Default for BTreeMap<K, V> {
        fn default() -> BTreeMap<K, V> {
            BTreeMap::new()
        }
    }

    impl<K: fmt::Debug, V: fmt::Debug> fmt::Debug for BTreeMap<K, V> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let mut m = f.debug_map();
            for (k, v) in self.iter() {
                m.entry(k, v);
            }
            m.finish()
        }
    }

    impl<'b, K: Ord + Borrow<Q>, Q: ?Sized + Ord, V> crate::ops::Index<&'b Q> for BTreeMap<K, V> {
        type Output = V;
        #[track_caller]
        fn index(&self, key: &Q) -> &V {
            match self.get(key) {
                Some(v) => v,
                None => crate::rt::begin_panic_str("key not found", crate::intrinsics::caller_location()),
            }
        }
    }
}

pub mod btree_set {
    use crate::borrow::Borrow;
    use crate::collections::btree_map::{self, BTreeMap};
    use crate::fmt;
    use crate::option::Option::{self, None, Some};
    use crate::vec::Vec;

    #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
    pub struct BTreeSet<T> {
        map: BTreeMap<T, ()>,
    }

    impl<T> BTreeSet<T> {
        pub fn new() -> BTreeSet<T> {
            BTreeSet { map: BTreeMap::new() }
        }
        pub fn len(&self) -> usize {
            self.map.len()
        }
        pub fn is_empty(&self) -> bool {
            self.map.is_empty()
        }
        pub fn clear(&mut self) {
            self.map.clear();
        }
        pub fn iter(&self) -> btree_map::Keys<'_, T, ()> {
            self.map.keys()
        }
        pub fn first(&self) -> Option<&T> {
            match self.map.first_key_value() {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
        pub fn last(&self) -> Option<&T> {
            match self.map.last_key_value() {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
        pub fn pop_first(&mut self) -> Option<T> {
            match self.map.pop_first() {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
        pub fn pop_last(&mut self) -> Option<T> {
            match self.map.pop_last() {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
        pub fn retain<F: FnMut(&T) -> bool>(&mut self, mut f: F) {
            self.map.retain(|k, _| f(k));
        }
    }

    impl<T: Ord> BTreeSet<T> {
        pub fn insert(&mut self, value: T) -> bool {
            if self.map.contains_key(&value) {
                false
            } else {
                self.map.insert(value, ());
                true
            }
        }
        pub fn contains<Q: ?Sized + Ord>(&self, value: &Q) -> bool
        where
            T: Borrow<Q>,
        {
            self.map.contains_key(value)
        }
        pub fn get<Q: ?Sized + Ord>(&self, value: &Q) -> Option<&T>
        where
            T: Borrow<Q>,
        {
            match self.map.get_key_value(value) {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
        pub fn remove<Q: ?Sized + Ord>(&mut self, value: &Q) -> bool
        where
            T: Borrow<Q>,
        {
            self.map.remove(value).is_some()
        }
        pub fn take<Q: ?Sized + Ord>(&mut self, value: &Q) -> Option<T>
        where
            T: Borrow<Q>,
        {
            match self.map.remove_entry(value) {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
        pub fn range<R: btree_map::RangeBounds<T>>(&self, r: R) -> crate::vec::IntoIter<&T> {
            let mut v = Vec::new();
            for (k, _) in self.map.range(r) {
                v.push(k);
            }
            v.into_iter()
        }
        pub fn is_subset(&self, other: &BTreeSet<T>) -> bool {
            self.iter().all(|x| other.contains(x))
        }
        pub fn is_superset(&self, other: &BTreeSet<T>) -> bool {
            other.is_subset(self)
        }
        pub fn is_disjoint(&self, other: &BTreeSet<T>) -> bool {
            self.iter().all(|x| !other.contains(x))
        }
        pub fn union<'a>(&'a self, other: &'a BTreeSet<T>) -> crate::vec::IntoIter<&'a T> {
            let mut v: Vec<&'a T> = self.iter().collect();
            for x in other.iter() {
                if !self.contains(x) {
                    v.push(x);
                }
            }
            v.sort();
            v.into_iter()
        }
        pub fn intersection<'a>(&'a self, other: &'a BTreeSet<T>) -> crate::vec::IntoIter<&'a T> {
            let mut v = Vec::new();
            for x in self.iter() {
                if other.contains(x) {
                    v.push(x);
                }
            }
            v.into_iter()
        }
        pub fn difference<'a>(&'a self, other: &'a BTreeSet<T>) -> crate::vec::IntoIter<&'a T> {
            let mut v = Vec::new();
            for x in self.iter() {
                if !other.contains(x) {
                    v.push(x);
                }
            }
            v.into_iter()
        }
        pub fn symmetric_difference<'a>(&'a self, other: &'a BTreeSet<T>) -> crate::vec::IntoIter<&'a T> {
            let mut v = Vec::new();
            for x in self.iter() {
                if !other.contains(x) {
                    v.push(x);
                }
            }
            for x in other.iter() {
                if !self.contains(x) {
                    v.push(x);
                }
            }
            v.sort();
            v.into_iter()
        }
        pub fn append(&mut self, other: &mut BTreeSet<T>) {
            self.map.append(&mut other.map);
        }
    }

    pub struct IntoIter<T> {
        inner: crate::vec::IntoIter<(T, ())>,
    }

    impl<T> Iterator for IntoIter<T> {
        type Item = T;
        fn next(&mut self) -> Option<T> {
            match self.inner.next() {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
    }

    impl<T> DoubleEndedIterator for IntoIter<T> {
        fn next_back(&mut self) -> Option<T> {
            match self.inner.next_back() {
                Some((k, _)) => Some(k),
                None => None,
            }
        }
    }

    impl<T> IntoIterator for BTreeSet<T> {
        type Item = T;
        type IntoIter = IntoIter<T>;
        fn into_iter(self) -> IntoIter<T> {
            IntoIter {
                inner: self.map.into_iter(),
            }
        }
    }

    impl<'a, T> IntoIterator for &'a BTreeSet<T> {
        type Item = &'a T;
        type IntoIter = btree_map::Keys<'a, T, ()>;
        fn into_iter(self) -> btree_map::Keys<'a, T, ()> {
            self.iter()
        }
    }

    impl<T: Ord> crate::iter::FromIterator<T> for BTreeSet<T> {
        fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> BTreeSet<T> {
            let mut s = BTreeSet::new();
            for x in iter {
                s.insert(x);
            }
            s
        }
    }

    impl<T: Ord> crate::iter::Extend<T> for BTreeSet<T> {
        fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
            for x in iter {
                self.insert(x);
            }
        }
    }

    impl<T: Ord, const N: usize> From<[T; N]> for BTreeSet<T> {
        fn from(a: [T; N]) -> BTreeSet<T> {
            let mut s = BTreeSet::new();
            for x in a {
                s.insert(x);
            }
            s
        }
    }

    impl<T: fmt::Debug> fmt::Debug for BTreeSet<T> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let mut s = f.debug_set();
            for x in self.iter() {
                s.entry(x);
            }
            s.finish()
        }
    }
}

pub mod vec_deque {
    use crate::fmt;
    use crate::option::Option::{self, None, Some};
    use crate::vec::Vec;

    /// A ring buffer.
    pub struct VecDeque<T> {
        buf: Vec<Option<T>>,
        head: usize,
        len: usize,
    }

    impl<T> VecDeque<T> {
        pub fn new() -> VecDeque<T> {
            VecDeque {
                buf: Vec::new(),
                head: 0,
                len: 0,
            }
        }
        pub fn with_capacity(_n: usize) -> VecDeque<T> {
            VecDeque::new()
        }
        pub fn len(&self) -> usize {
            self.len
        }
        pub fn is_empty(&self) -> bool {
            self.len == 0
        }
        fn idx(&self, i: usize) -> usize {
            (self.head + i) % self.buf.len()
        }
        fn grow(&mut self) {
            if self.len < self.buf.len() {
                return;
            }
            let n = if self.buf.is_empty() { 8 } else { self.buf.len() * 2 };
            let mut nb: Vec<Option<T>> = Vec::with_capacity(n);
            let mut i = 0;
            while i < self.len {
                let k = self.idx(i);
                nb.push(self.buf[k].take());
                i += 1;
            }
            while nb.len() < n {
                nb.push(None);
            }
            self.buf = nb;
            self.head = 0;
        }
        pub fn push_back(&mut self, x: T) {
            self.grow();
            let k = self.idx(self.len);
            self.buf[k] = Some(x);
            self.len += 1;
        }
        pub fn push_front(&mut self, x: T) {
            self.grow();
            self.head = (self.head + self.buf.len() - 1) % self.buf.len();
            let h = self.head;
            self.buf[h] = Some(x);
            self.len += 1;
        }
        pub fn pop_back(&mut self) -> Option<T> {
            if self.len == 0 {
                return None;
            }
            self.len -= 1;
            let k = self.idx(self.len);
            self.buf[k].take()
        }
        pub fn pop_front(&mut self) -> Option<T> {
            if self.len == 0 {
                return None;
            }
            let h = self.head;
            let x = self.buf[h].take();
            self.head = (self.head + 1) % self.buf.len();
            self.len -= 1;
            x
        }
        pub fn get(&self, i: usize) -> Option<&T> {
            if i < self.len {
                self.buf[self.idx(i)].as_ref()
            } else {
                None
            }
        }
        pub fn get_mut(&mut self, i: usize) -> Option<&mut T> {
            if i < self.len {
                let k = self.idx(i);
                self.buf[k].as_mut()
            } else {
                None
            }
        }
        pub fn front(&self) -> Option<&T> {
            self.get(0)
        }
        pub fn back(&self) -> Option<&T> {
            if self.len == 0 {
                None
            } else {
                self.get(self.len - 1)
            }
        }
        pub fn front_mut(&mut self) -> Option<&mut T> {
            self.get_mut(0)
        }
        pub fn back_mut(&mut self) -> Option<&mut T> {
            if self.len == 0 {
                None
            } else {
                let n = self.len - 1;
                self.get_mut(n)
            }
        }
        pub fn clear(&mut self) {
            *self = VecDeque::new();
        }
        pub fn iter(&self) -> Iter<'_, T> {
            Iter { d: self, i: 0, j: self.len }
        }
        pub fn iter_mut(&mut self) -> crate::vec::IntoIter<&mut T> {
            let mut v = Vec::new();
            let n = self.buf.len();
            let mut i = 0;
            while i < self.len {
                let k = (self.head + i) % n;
                let p: *mut Option<T> = &mut self.buf[k];
                unsafe {
                    if let Some(x) = &mut *p {
                        v.push(x);
                    }
                }
                i += 1;
            }
            v.into_iter()
        }
        pub fn contains(&self, x: &T) -> bool
        where
            T: PartialEq,
        {
            self.iter().any(|y| *y == *x)
        }
        pub fn swap(&mut self, a: usize, b: usize) {
            let (ka, kb) = (self.idx(a), self.idx(b));
            self.buf.swap(ka, kb);
        }
        pub fn insert(&mut self, index: usize, x: T) {
            self.push_back(x);
            let mut i = self.len - 1;
            while i > index {
                self.swap(i, i - 1);
                i -= 1;
            }
        }
        pub fn remove(&mut self, index: usize) -> Option<T> {
            if index >= self.len {
                return None;
            }
            let mut i = index;
            while i + 1 < self.len {
                self.swap(i, i + 1);
                i += 1;
            }
            self.pop_back()
        }
        pub fn truncate(&mut self, n: usize) {
            while self.len > n {
                self.pop_back();
            }
        }
        pub fn drain<R: crate::vec::RangeArg>(&mut self, range: R) -> crate::vec::IntoIter<T> {
            let (lo, hi) = range.bounds(self.len);
            let mut all = Vec::new();
            while let Some(x) = self.pop_front() {
                all.push(x);
            }
            let mut out = Vec::new();
            for (i, x) in all.into_iter().enumerate() {
                if i >= lo && i < hi {
                    out.push(x);
                } else {
                    self.push_back(x);
                }
            }
            out.into_iter()
        }
        pub fn rotate_left(&mut self, n: usize) {
            let mut i = 0;
            while i < n {
                if let Some(x) = self.pop_front() {
                    self.push_back(x);
                }
                i += 1;
            }
        }
        pub fn rotate_right(&mut self, n: usize) {
            let mut i = 0;
            while i < n {
                if let Some(x) = self.pop_back() {
                    self.push_front(x);
                }
                i += 1;
            }
        }
        pub fn retain<F: FnMut(&T) -> bool>(&mut self, mut f: F) {
            let mut v = Vec::new();
            while let Some(x) = self.pop_front() {
                if f(&x) {
                    v.push(x);
                }
            }
            for x in v {
                self.push_back(x);
            }
        }
        pub fn extend_from_vec(&mut self, v: Vec<T>) {
            for x in v {
                self.push_back(x);
            }
        }
    }

    pub struct Iter<'a, T> {
        d: &'a VecDeque<T>,
        i: usize,
        j: usize,
    }

    impl<'a, T> Iterator for Iter<'a, T> {
        type Item = &'a T;
        fn next(&mut self) -> Option<&'a T> {
            if self.i < self.j {
                let x = self.d.get(self.i);
                self.i += 1;
                x
            } else {
                None
            }
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            (self.j - self.i, Some(self.j - self.i))
        }
    }

    impl<'a, T> DoubleEndedIterator for Iter<'a, T> {
        fn next_back(&mut self) -> Option<&'a T> {
            if self.i < self.j {
                self.j -= 1;
                self.d.get(self.j)
            } else {
                None
            }
        }
    }

    impl<'a, T> ExactSizeIterator for Iter<'a, T> {}

    pub struct IntoIter<T> {
        d: VecDeque<T>,
    }

    impl<T> Iterator for IntoIter<T> {
        type Item = T;
        fn next(&mut self) -> Option<T> {
            self.d.pop_front()
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            (self.d.len(), Some(self.d.len()))
        }
    }

    impl<T> DoubleEndedIterator for IntoIter<T> {
        fn next_back(&mut self) -> Option<T> {
            self.d.pop_back()
        }
    }

    impl<T> IntoIterator for VecDeque<T> {
        type Item = T;
        type IntoIter = IntoIter<T>;
        fn into_iter(self) -> IntoIter<T> {
            IntoIter { d: self }
        }
    }

    impl<'a, T> IntoIterator for &'a VecDeque<T> {
        type Item = &'a T;
        type IntoIter = Iter<'a, T>;
        fn into_iter(self) -> Iter<'a, T> {
            self.iter()
        }
    }

    impl<T> crate::iter::FromIterator<T> for VecDeque<T> {
        fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> VecDeque<T> {
            let mut d = VecDeque::new();
            for x in iter {
                d.push_back(x);
            }
            d
        }
    }

    impl<T> crate::iter::Extend<T> for VecDeque<T> {
        fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
            for x in iter {
                self.push_back(x);
            }
        }
    }

    impl<T> From<Vec<T>> for VecDeque<T> {
        fn from(v: Vec<T>) -> VecDeque<T> {
            let mut d = VecDeque::new();
            for x in v {
                d.push_back(x);
            }
            d
        }
    }

    impl<T> From<VecDeque<T>> for Vec<T> {
        fn from(d: VecDeque<T>) -> Vec<T> {
            d.into_iter().collect()
        }
    }

    impl<T, const N: usize> From<[T; N]> for VecDeque<T> {
        fn from(a: [T; N]) -> VecDeque<T> {
            let mut d = VecDeque::new();
            for x in a {
                d.push_back(x);
            }
            d
        }
    }

    impl<T: Clone> Clone for VecDeque<T> {
        fn clone(&self) -> VecDeque<T> {
            self.iter().cloned().collect()
        }
    }

    impl<T> Default for VecDeque<T> {
        fn default() -> VecDeque<T> {
            VecDeque::new()
        }
    }

    impl<T: PartialEq> PartialEq for VecDeque<T> {
        fn eq(&self, other: &VecDeque<T>) -> bool {
            self.len() == other.len() && self.iter().zip(other.iter()).all(|(a, b)| *a == *b)
        }
    }

    impl<T: Eq> Eq for VecDeque<T> {}

    impl<T: fmt::Debug> fmt::Debug for VecDeque<T> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let mut l = f.debug_list();
            for x in self.iter() {
                l.entry(x);
            }
            l.finish()
        }
    }

    impl<T> crate::ops::Index<usize> for VecDeque<T> {
        type Output = T;
        #[track_caller]
        fn index(&self, i: usize) -> &T {
            match self.get(i) {
                Some(x) => x,
                None => crate::rt::begin_panic_str("Out of bounds access", crate::intrinsics::caller_location()),
            }
        }
    }

    impl<T> crate::ops::IndexMut<usize> for VecDeque<T> {
        #[track_caller]
        fn index_mut(&mut self, i: usize) -> &mut T {
            match self.get_mut(i) {
                Some(x) => x,
                None => crate::rt::begin_panic_str("Out of bounds access", crate::intrinsics::caller_location()),
            }
        }
    }
}

pub mod binary_heap {
    use crate::fmt;
    use crate::option::Option::{self, None, Some};
    use crate::vec::Vec;

    /// A max-heap.
    #[derive(Clone)]
    pub struct BinaryHeap<T> {
        data: Vec<T>,
    }

    impl<T> BinaryHeap<T> {
        pub fn len(&self) -> usize {
            self.data.len()
        }
        pub fn is_empty(&self) -> bool {
            self.data.is_empty()
        }
        pub fn peek(&self) -> Option<&T> {
            self.data.first()
        }
        pub fn clear(&mut self) {
            self.data.clear();
        }
        pub fn iter(&self) -> crate::slice::Iter<'_, T> {
            self.data.iter()
        }
        pub fn into_vec(self) -> Vec<T> {
            self.data
        }
    }

    impl<T: Ord> BinaryHeap<T> {
        pub fn new() -> BinaryHeap<T> {
            BinaryHeap { data: Vec::new() }
        }
        pub fn with_capacity(n: usize) -> BinaryHeap<T> {
            BinaryHeap {
                data: Vec::with_capacity(n),
            }
        }
        pub fn push(&mut self, x: T) {
            self.data.push(x);
            let mut i = self.data.len() - 1;
            while i > 0 {
                let p = (i - 1) / 2;
                if self.data[i] > self.data[p] {
                    self.data.swap(i, p);
                    i = p;
                } else {
                    break;
                }
            }
        }
        pub fn pop(&mut self) -> Option<T> {
            let n = self.data.len();
            if n == 0 {
                return None;
            }
            self.data.swap(0, n - 1);
            let top = self.data.pop();
            let n = self.data.len();
            let mut i = 0;
            loop {
                let l = 2 * i + 1;
                let r = l + 1;
                let mut m = i;
                if l < n && self.data[l] > self.data[m] {
                    m = l;
                }
                if r < n && self.data[r] > self.data[m] {
                    m = r;
                }
                if m == i {
                    break;
                }
                self.data.swap(i, m);
                i = m;
            }
            top
        }
        pub fn into_sorted_vec(self) -> Vec<T> {
            let mut v = self.data;
            v.sort();
            v
        }
        pub fn peek_mut(&mut self) -> Option<&mut T> {
            self.data.first_mut()
        }
        pub fn append(&mut self, other: &mut BinaryHeap<T>) {
            while let Some(x) = other.pop() {
                self.push(x);
            }
        }
        pub fn drain(&mut self) -> crate::vec::IntoIter<T> {
            crate::mem::replace(&mut self.data, Vec::new()).into_iter()
        }
    }

    impl<T: Ord> Default for BinaryHeap<T> {
        fn default() -> BinaryHeap<T> {
            BinaryHeap::new()
        }
    }

    impl<T: Ord> From<Vec<T>> for BinaryHeap<T> {
        fn from(v: Vec<T>) -> BinaryHeap<T> {
            let mut h = BinaryHeap::new();
            for x in v {
                h.push(x);
            }
            h
        }
    }

    impl<T: Ord, const N: usize> From<[T; N]> for BinaryHeap<T> {
        fn from(a: [T; N]) -> BinaryHeap<T> {
            let mut h = BinaryHeap::new();
            for x in a {
                h.push(x);
            }
            h
        }
    }

    impl<T: Ord> crate::iter::FromIterator<T> for BinaryHeap<T> {
        fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> BinaryHeap<T> {
            let mut h = BinaryHeap::new();
            for x in iter {
                h.push(x);
            }
            h
        }
    }

    impl<T: Ord> crate::iter::Extend<T> for BinaryHeap<T> {
        fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
            for x in iter {
                self.push(x);
            }
        }
    }

    impl<T> IntoIterator for BinaryHeap<T> {
        type Item = T;
        type IntoIter = crate::vec::IntoIter<T>;
        fn into_iter(self) -> crate::vec::IntoIter<T> {
            self.data.into_iter()
        }
    }

    impl<'a, T> IntoIterator for &'a BinaryHeap<T> {
        type Item = &'a T;
        type IntoIter = crate::slice::Iter<'a, T>;
        fn into_iter(self) -> crate::slice::Iter<'a, T> {
            self.data.iter()
        }
    }

    impl<T: fmt::Debug> fmt::Debug for BinaryHeap<T> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            fmt::Debug::fmt(&self.data, f)
        }
    }
}
