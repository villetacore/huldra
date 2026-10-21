//! Hashing: `Hash`, `Hasher` and the default hasher (FNV-1a, 64 bits).

pub trait Hasher {
    fn finish(&self) -> u64;
    fn write(&mut self, bytes: &[u8]);
    fn write_u8(&mut self, i: u8) {
        self.write(&[i]);
    }
    fn write_u16(&mut self, i: u16) {
        self.write_u64(i as u64);
    }
    fn write_u32(&mut self, i: u32) {
        self.write_u64(i as u64);
    }
    fn write_u64(&mut self, i: u64) {
        self.write(&i.to_le_bytes());
    }
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
    fn write_i8(&mut self, i: i8) {
        self.write_u8(i as u8);
    }
    fn write_i16(&mut self, i: i16) {
        self.write_u64(i as u64);
    }
    fn write_i32(&mut self, i: i32) {
        self.write_u64(i as u64);
    }
    fn write_i64(&mut self, i: i64) {
        self.write_u64(i as u64);
    }
    fn write_isize(&mut self, i: isize) {
        self.write_u64(i as u64);
    }
}

impl<H: Hasher + ?Sized> Hasher for &mut H {
    fn finish(&self) -> u64 {
        (**self).finish()
    }
    fn write(&mut self, bytes: &[u8]) {
        (**self).write(bytes)
    }
}

pub trait Hash {
    fn hash<H: Hasher>(&self, state: &mut H);
    fn hash_slice<H: Hasher>(data: &[Self], state: &mut H)
    where
        Self: Sized,
    {
        for x in data.iter() {
            x.hash(state);
        }
    }
}

pub trait BuildHasher {
    type Hasher: Hasher;
    fn build_hasher(&self) -> Self::Hasher;
    fn hash_one<T: Hash>(&self, x: T) -> u64 {
        let mut h = self.build_hasher();
        x.hash(&mut h);
        h.finish()
    }
}

#[derive(Clone, Debug)]
pub struct DefaultHasher {
    state: u64,
}

impl DefaultHasher {
    pub fn new() -> DefaultHasher {
        DefaultHasher { state: 0xcbf29ce484222325 }
    }
}

impl Default for DefaultHasher {
    fn default() -> DefaultHasher {
        DefaultHasher::new()
    }
}

impl Hasher for DefaultHasher {
    fn finish(&self) -> u64 {
        // A final mix so that the low bits are good bucket indices.
        let mut h = self.state;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51afd7ed558ccd);
        h ^= h >> 33;
        h
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut h = self.state;
        for b in bytes.iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        self.state = h;
    }
    fn write_u64(&mut self, i: u64) {
        let mut h = self.state;
        h ^= i;
        h = h.wrapping_mul(0x100000001b3);
        h ^= h >> 29;
        self.state = h;
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RandomState;

impl RandomState {
    pub fn new() -> RandomState {
        RandomState
    }
}

impl BuildHasher for RandomState {
    type Hasher = DefaultHasher;
    fn build_hasher(&self) -> DefaultHasher {
        DefaultHasher::new()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BuildHasherDefault<H>(crate::marker::PhantomData<H>);

impl<H: Default + Hasher> BuildHasher for BuildHasherDefault<H> {
    type Hasher = H;
    fn build_hasher(&self) -> H {
        H::default()
    }
}

macro_rules! int_hash {
    ($($t:ty)*) => {$(
        impl Hash for $t {
            fn hash<H: Hasher>(&self, state: &mut H) {
                state.write_u64(*self as u64);
            }
        }
    )*};
}
int_hash!(i8 i16 i32 i64 isize u8 u16 u32 u64 usize);

impl Hash for bool {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u8(*self as u8);
    }
}

impl Hash for char {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(*self as u32);
    }
}

impl Hash for str {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(self.as_bytes());
        state.write_u8(0xff);
    }
}

impl Hash for () {
    fn hash<H: Hasher>(&self, _state: &mut H) {}
}

impl<T: Hash + ?Sized> Hash for &T {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

impl<T: Hash + ?Sized> Hash for &mut T {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

impl<T: Hash> Hash for [T] {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_usize(self.len());
        for x in self.iter() {
            x.hash(state);
        }
    }
}

impl<T: Hash, const N: usize> Hash for [T; N] {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Hash::hash(&self[..], state)
    }
}

macro_rules! tuple_hash {
    ($(($($T:ident $i:tt),+))*) => {$(
        impl<$($T: Hash),+> Hash for ($($T,)+) {
            fn hash<HH: Hasher>(&self, state: &mut HH) {
                $(self.$i.hash(state);)+
            }
        }
    )*};
}
tuple_hash! {
    (A 0)
    (A 0, B 1)
    (A 0, B 1, C 2)
    (A 0, B 1, C 2, D 3)
    (A 0, B 1, C 2, D 3, E 4)
    (A 0, B 1, C 2, D 3, E 4, F 5)
}
