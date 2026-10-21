//! The standard library of hrc, the Huldra Rust compiler. It is compiled
//! together with every program; only what a program uses is generated.
//! The compiler knows some items by path (`option::Option`, `ops::Add`,
//! ...); low-level operations are `extern "rust-intrinsic"` functions
//! implemented by the code generator, and the rest of the runtime is C
//! (`rt.c`).

pub mod intrinsics {
    extern "rust-intrinsic" {
        pub fn size_of<T>() -> usize;
        pub fn ptr_read<T>(p: *const T) -> T;
        pub fn ptr_write<T>(p: *mut T, v: T);
        pub fn ptr_add<T>(p: *mut T, n: usize) -> *mut T;
        pub fn ptr_sub<T>(p: *mut T, n: usize) -> *mut T;
        pub fn ptr_diff<T>(a: *const T, b: *const T) -> isize;
        pub fn ptr_null<T>() -> *mut T;
        pub fn ptr_copy<T>(src: *const T, dst: *mut T, count: usize);
        pub fn str_from_raw(p: *const u8, len: usize) -> &'static str;
        pub fn str_ptr(s: &str) -> *const u8;
        pub fn str_len(s: &str) -> usize;
        pub fn slice_from_raw<T>(p: *const T, len: usize) -> &'static [T];
        pub fn slice_from_raw_mut<T>(p: *mut T, len: usize) -> &'static mut [T];
        pub fn slice_ptr<T>(s: &[T]) -> *const T;
        pub fn slice_len<T>(s: &[T]) -> usize;
        pub fn discriminant<T>(v: &T) -> i64;
        pub fn abort() -> !;
        pub fn caller_location() -> &'static crate::panic::Location;
        pub fn wrapping_add<T>(a: T, b: T) -> T;
        pub fn wrapping_sub<T>(a: T, b: T) -> T;
        pub fn wrapping_mul<T>(a: T, b: T) -> T;
        pub fn overflowing_add<T>(a: T, b: T) -> (T, bool);
        pub fn overflowing_sub<T>(a: T, b: T) -> (T, bool);
        pub fn overflowing_mul<T>(a: T, b: T) -> (T, bool);
        pub fn f64_to_bits(x: f64) -> u64;
        pub fn f32_to_bits(x: f32) -> u32;
        pub fn f64_from_bits(x: u64) -> f64;
        pub fn f32_from_bits(x: u32) -> f32;
        pub fn type_name<T>() -> &'static str;
    }
}

/// The C runtime and C library.
pub mod sys {
    extern "C" {
        pub fn malloc(n: usize) -> *mut u8;
        pub fn realloc(p: *mut u8, n: usize) -> *mut u8;
        pub fn free(p: *mut u8);
        pub fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32;
        pub fn read(fd: i32, buf: *mut u8, n: usize) -> isize;
        pub fn write(fd: i32, buf: *const u8, n: usize) -> isize;
        pub fn hrc_write_stdout(p: *const u8, n: usize);
        pub fn hrc_write_stderr(p: *const u8, n: usize);
        pub fn hrc_flush_stdout();
        pub fn hrc_exit(code: i32) -> !;
        pub fn hrc_panic_start(loc: &crate::panic::Location);
        pub fn hrc_panic_end() -> !;
        pub fn hrc_arg_count() -> usize;
        pub fn hrc_arg(i: usize) -> &'static str;
        pub fn hrc_fmt_float(v: f64, is_f32: i32, kind: i32, prec: i64, out: *mut u8) -> usize;
        pub fn hrc_parse_float(s: *const u8, len: usize, is_f32: i32, out: *mut f64) -> i32;
    }
}

pub mod marker {
    pub trait Sized {}
    pub trait Copy: Clone {}
    pub trait Send {}
    pub trait Sync {}
    pub trait Unpin {}

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
    pub struct PhantomData<T>;
}

pub mod clone {
    pub trait Clone {
        fn clone(&self) -> Self;
        fn clone_from(&mut self, source: &Self) {
            *self = source.clone();
        }
    }
}

pub mod default {
    pub trait Default {
        fn default() -> Self;
    }

    impl Default for bool {
        fn default() -> bool {
            false
        }
    }

    impl Default for () {
        fn default() -> () {}
    }

    impl<T: Default, const N: usize> Default for [T; N] {
        fn default() -> [T; N] {
            crate::array::from_fn(|_| T::default())
        }
    }

    macro_rules! tuple_default {
        ($(($($T:ident),+))*) => {$(
            impl<$($T: Default),+> Default for ($($T,)+) {
                fn default() -> ($($T,)+) {
                    ($($T::default(),)+)
                }
            }
        )*};
    }
    tuple_default! {
        (A)
        (A, B)
        (A, B, C)
        (A, B, C, D)
        (A, B, C, D, E)
    }
}

pub mod ops {
    pub trait Add<Rhs = Self> {
        type Output;
        fn add(self, rhs: Rhs) -> Self::Output;
    }
    pub trait Sub<Rhs = Self> {
        type Output;
        fn sub(self, rhs: Rhs) -> Self::Output;
    }
    pub trait Mul<Rhs = Self> {
        type Output;
        fn mul(self, rhs: Rhs) -> Self::Output;
    }
    pub trait Div<Rhs = Self> {
        type Output;
        fn div(self, rhs: Rhs) -> Self::Output;
    }
    pub trait Rem<Rhs = Self> {
        type Output;
        fn rem(self, rhs: Rhs) -> Self::Output;
    }
    pub trait BitAnd<Rhs = Self> {
        type Output;
        fn bitand(self, rhs: Rhs) -> Self::Output;
    }
    pub trait BitOr<Rhs = Self> {
        type Output;
        fn bitor(self, rhs: Rhs) -> Self::Output;
    }
    pub trait BitXor<Rhs = Self> {
        type Output;
        fn bitxor(self, rhs: Rhs) -> Self::Output;
    }
    pub trait Shl<Rhs = Self> {
        type Output;
        fn shl(self, rhs: Rhs) -> Self::Output;
    }
    pub trait Shr<Rhs = Self> {
        type Output;
        fn shr(self, rhs: Rhs) -> Self::Output;
    }
    pub trait Neg {
        type Output;
        fn neg(self) -> Self::Output;
    }
    pub trait Not {
        type Output;
        fn not(self) -> Self::Output;
    }
    pub trait AddAssign<Rhs = Self> {
        fn add_assign(&mut self, rhs: Rhs);
    }
    pub trait SubAssign<Rhs = Self> {
        fn sub_assign(&mut self, rhs: Rhs);
    }
    pub trait MulAssign<Rhs = Self> {
        fn mul_assign(&mut self, rhs: Rhs);
    }
    pub trait DivAssign<Rhs = Self> {
        fn div_assign(&mut self, rhs: Rhs);
    }
    pub trait RemAssign<Rhs = Self> {
        fn rem_assign(&mut self, rhs: Rhs);
    }
    pub trait BitAndAssign<Rhs = Self> {
        fn bitand_assign(&mut self, rhs: Rhs);
    }
    pub trait BitOrAssign<Rhs = Self> {
        fn bitor_assign(&mut self, rhs: Rhs);
    }
    pub trait BitXorAssign<Rhs = Self> {
        fn bitxor_assign(&mut self, rhs: Rhs);
    }
    pub trait ShlAssign<Rhs = Self> {
        fn shl_assign(&mut self, rhs: Rhs);
    }
    pub trait ShrAssign<Rhs = Self> {
        fn shr_assign(&mut self, rhs: Rhs);
    }

    pub trait Index<Idx> {
        type Output;
        fn index(&self, index: Idx) -> &Self::Output;
    }
    pub trait IndexMut<Idx>: Index<Idx> {
        fn index_mut(&mut self, index: Idx) -> &mut Self::Output;
    }

    pub trait Deref {
        type Target;
        fn deref(&self) -> &Self::Target;
    }
    pub trait DerefMut: Deref {
        fn deref_mut(&mut self) -> &mut Self::Target;
    }

    pub trait Drop {
        fn drop(&mut self);
    }

    pub trait FnOnce<Args> {
        type Output;
    }
    pub trait FnMut<Args>: FnOnce<Args> {}
    pub trait Fn<Args>: FnMut<Args> {}

    #[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct Range<Idx> {
        pub start: Idx,
        pub end: Idx,
    }

    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    pub struct RangeInclusive<Idx> {
        pub start: Idx,
        pub end: Idx,
        pub exhausted: bool,
    }

    impl<Idx> RangeInclusive<Idx> {
        pub fn new(start: Idx, end: Idx) -> RangeInclusive<Idx> {
            RangeInclusive { start, end, exhausted: false }
        }
        pub fn start(&self) -> &Idx {
            &self.start
        }
        pub fn end(&self) -> &Idx {
            &self.end
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    pub struct RangeFrom<Idx> {
        pub start: Idx,
    }

    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    pub struct RangeTo<Idx> {
        pub end: Idx,
    }

    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    pub struct RangeToInclusive<Idx> {
        pub end: Idx,
    }

    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    pub struct RangeFull;

    impl<Idx: PartialOrd> Range<Idx> {
        pub fn contains(&self, item: &Idx) -> bool {
            *item >= self.start && *item < self.end
        }
        pub fn is_empty(&self) -> bool {
            !(self.start < self.end)
        }
    }

    impl<Idx: PartialOrd> RangeInclusive<Idx> {
        pub fn contains(&self, item: &Idx) -> bool {
            *item >= self.start && *item <= self.end
        }
        pub fn is_empty(&self) -> bool {
            self.exhausted || !(self.start <= self.end)
        }
    }

    impl<Idx: PartialOrd> RangeFrom<Idx> {
        pub fn contains(&self, item: &Idx) -> bool {
            *item >= self.start
        }
    }

    impl<Idx: PartialOrd> RangeTo<Idx> {
        pub fn contains(&self, item: &Idx) -> bool {
            *item < self.end
        }
    }

    impl<Idx: crate::fmt::Debug> crate::fmt::Debug for Range<Idx> {
        fn fmt(&self, f: &mut crate::fmt::Formatter) -> crate::fmt::Result {
            crate::fmt::Debug::fmt(&self.start, f)?;
            f.write_str("..")?;
            crate::fmt::Debug::fmt(&self.end, f)
        }
    }

    impl<Idx: crate::fmt::Debug> crate::fmt::Debug for RangeInclusive<Idx> {
        fn fmt(&self, f: &mut crate::fmt::Formatter) -> crate::fmt::Result {
            crate::fmt::Debug::fmt(&self.start, f)?;
            f.write_str("..=")?;
            crate::fmt::Debug::fmt(&self.end, f)
        }
    }

    pub enum ControlFlow<B, C = ()> {
        Continue(C),
        Break(B),
    }

    // Operators on primitive types (used through generic bounds; direct
    // uses are compiled as builtin operations).
    macro_rules! binop_impls {
        ($tr:ident $m:ident $op:tt $tra:ident $ma:ident; $($t:ty)*) => {$(
            impl $tr for $t {
                type Output = $t;
                fn $m(self, o: $t) -> $t { self $op o }
            }
            impl<'a> $tr<&'a $t> for $t {
                type Output = $t;
                fn $m(self, o: &'a $t) -> $t { self $op *o }
            }
            impl<'a> $tr<$t> for &'a $t {
                type Output = $t;
                fn $m(self, o: $t) -> $t { *self $op o }
            }
            impl<'a, 'b> $tr<&'a $t> for &'b $t {
                type Output = $t;
                fn $m(self, o: &'a $t) -> $t { *self $op *o }
            }
            impl $tra for $t {
                fn $ma(&mut self, o: $t) { *self = *self $op o; }
            }
            impl<'a> $tra<&'a $t> for $t {
                fn $ma(&mut self, o: &'a $t) { *self = *self $op *o; }
            }
        )*};
    }
    binop_impls!(Add add + AddAssign add_assign; i8 i16 i32 i64 isize u8 u16 u32 u64 usize f32 f64);
    binop_impls!(Sub sub - SubAssign sub_assign; i8 i16 i32 i64 isize u8 u16 u32 u64 usize f32 f64);
    binop_impls!(Mul mul * MulAssign mul_assign; i8 i16 i32 i64 isize u8 u16 u32 u64 usize f32 f64);
    binop_impls!(Div div / DivAssign div_assign; i8 i16 i32 i64 isize u8 u16 u32 u64 usize f32 f64);
    binop_impls!(Rem rem % RemAssign rem_assign; i8 i16 i32 i64 isize u8 u16 u32 u64 usize f32 f64);
    binop_impls!(BitAnd bitand & BitAndAssign bitand_assign; i8 i16 i32 i64 isize u8 u16 u32 u64 usize bool);
    binop_impls!(BitOr bitor | BitOrAssign bitor_assign; i8 i16 i32 i64 isize u8 u16 u32 u64 usize bool);
    binop_impls!(BitXor bitxor ^ BitXorAssign bitxor_assign; i8 i16 i32 i64 isize u8 u16 u32 u64 usize bool);
    binop_impls!(Shl shl << ShlAssign shl_assign; i8 i16 i32 i64 isize u8 u16 u32 u64 usize);
    binop_impls!(Shr shr >> ShrAssign shr_assign; i8 i16 i32 i64 isize u8 u16 u32 u64 usize);

    macro_rules! neg_impls {
        ($($t:ty)*) => {$(
            impl Neg for $t {
                type Output = $t;
                fn neg(self) -> $t { -self }
            }
        )*};
    }
    neg_impls!(i8 i16 i32 i64 isize f32 f64);

    macro_rules! not_impls {
        ($($t:ty)*) => {$(
            impl Not for $t {
                type Output = $t;
                fn not(self) -> $t { !self }
            }
            impl<'a> Not for &'a $t {
                type Output = $t;
                fn not(self) -> $t { !*self }
            }
        )*};
    }
    not_impls!(i8 i16 i32 i64 isize u8 u16 u32 u64 usize bool);

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Bound<T> {
        Included(T),
        Excluded(T),
        Unbounded,
    }
}

pub mod cmp {
    use crate::option::Option::{self, None, Some};

    pub trait PartialEq<Rhs = Self> {
        fn eq(&self, other: &Rhs) -> bool;
        fn ne(&self, other: &Rhs) -> bool {
            !self.eq(other)
        }
    }

    pub trait Eq: PartialEq {}

    #[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
    pub enum Ordering {
        Less = -1,
        Equal = 0,
        Greater = 1,
    }

    impl Ordering {
        pub fn is_eq(self) -> bool {
            match self {
                Ordering::Equal => true,
                _ => false,
            }
        }
        pub fn is_ne(self) -> bool {
            !self.is_eq()
        }
        pub fn is_lt(self) -> bool {
            match self {
                Ordering::Less => true,
                _ => false,
            }
        }
        pub fn is_gt(self) -> bool {
            match self {
                Ordering::Greater => true,
                _ => false,
            }
        }
        pub fn is_le(self) -> bool {
            !self.is_gt()
        }
        pub fn is_ge(self) -> bool {
            !self.is_lt()
        }
        pub fn reverse(self) -> Ordering {
            match self {
                Ordering::Less => Ordering::Greater,
                Ordering::Equal => Ordering::Equal,
                Ordering::Greater => Ordering::Less,
            }
        }
        pub fn then(self, other: Ordering) -> Ordering {
            match self {
                Ordering::Equal => other,
                _ => self,
            }
        }
        pub fn then_with<F: FnOnce() -> Ordering>(self, f: F) -> Ordering {
            match self {
                Ordering::Equal => f(),
                _ => self,
            }
        }
    }

    impl PartialOrd for Ordering {
        fn partial_cmp(&self, other: &Ordering) -> Option<Ordering> {
            (*self as i32).partial_cmp(&(*other as i32))
        }
    }

    impl Ord for Ordering {
        fn cmp(&self, other: &Ordering) -> Ordering {
            (*self as i32).cmp(&(*other as i32))
        }
    }

    pub trait PartialOrd<Rhs = Self>: PartialEq<Rhs> {
        fn partial_cmp(&self, other: &Rhs) -> Option<Ordering>;
        fn lt(&self, other: &Rhs) -> bool {
            match self.partial_cmp(other) {
                Some(Ordering::Less) => true,
                _ => false,
            }
        }
        fn le(&self, other: &Rhs) -> bool {
            match self.partial_cmp(other) {
                Some(Ordering::Less) | Some(Ordering::Equal) => true,
                _ => false,
            }
        }
        fn gt(&self, other: &Rhs) -> bool {
            match self.partial_cmp(other) {
                Some(Ordering::Greater) => true,
                _ => false,
            }
        }
        fn ge(&self, other: &Rhs) -> bool {
            match self.partial_cmp(other) {
                Some(Ordering::Greater) | Some(Ordering::Equal) => true,
                _ => false,
            }
        }
    }

    pub trait Ord: Eq + PartialOrd {
        fn cmp(&self, other: &Self) -> Ordering;
        fn max(self, other: Self) -> Self
        where
            Self: Sized,
        {
            match self.cmp(&other) {
                Ordering::Greater => self,
                _ => other,
            }
        }
        fn min(self, other: Self) -> Self
        where
            Self: Sized,
        {
            match self.cmp(&other) {
                Ordering::Greater => other,
                _ => self,
            }
        }
        fn clamp(self, min: Self, max: Self) -> Self
        where
            Self: Sized,
        {
            if self.cmp(&min) == Ordering::Less {
                min
            } else if self.cmp(&max) == Ordering::Greater {
                max
            } else {
                self
            }
        }
    }

    pub fn max<T: Ord>(a: T, b: T) -> T {
        a.max(b)
    }

    pub fn min<T: Ord>(a: T, b: T) -> T {
        a.min(b)
    }

    pub fn max_by_key<T, K: Ord, F: FnMut(&T) -> K>(a: T, b: T, mut f: F) -> T {
        if f(&b) >= f(&a) {
            b
        } else {
            a
        }
    }

    pub fn min_by_key<T, K: Ord, F: FnMut(&T) -> K>(a: T, b: T, mut f: F) -> T {
        if f(&b) < f(&a) {
            b
        } else {
            a
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
    pub struct Reverse<T>(pub T);

    impl<T: PartialOrd> PartialOrd for Reverse<T> {
        fn partial_cmp(&self, other: &Reverse<T>) -> Option<Ordering> {
            other.0.partial_cmp(&self.0)
        }
    }

    impl<T: Ord> Ord for Reverse<T> {
        fn cmp(&self, other: &Reverse<T>) -> Ordering {
            other.0.cmp(&self.0)
        }
    }

    // References compare their referents.
    impl<'a, A: PartialEq<B> + ?Sized, B: ?Sized> PartialEq<&'a B> for &'a A {
        fn eq(&self, other: &&B) -> bool {
            PartialEq::eq(*self, *other)
        }
    }
    impl<'a, A: PartialEq<B> + ?Sized, B: ?Sized> PartialEq<&'a mut B> for &'a mut A {
        fn eq(&self, other: &&mut B) -> bool {
            PartialEq::eq(&**self, &**other)
        }
    }
    impl<'a, A: Eq + ?Sized> Eq for &'a A {}
    impl<'a, A: PartialOrd + ?Sized> PartialOrd for &'a A {
        fn partial_cmp(&self, other: &&A) -> Option<Ordering> {
            PartialOrd::partial_cmp(*self, *other)
        }
    }
    impl<'a, A: Ord + ?Sized> Ord for &'a A {
        fn cmp(&self, other: &&A) -> Ordering {
            Ord::cmp(*self, *other)
        }
    }

    impl PartialEq for () {
        fn eq(&self, _other: &()) -> bool {
            true
        }
    }
    impl Eq for () {}
    impl PartialOrd for () {
        fn partial_cmp(&self, _other: &()) -> Option<Ordering> {
            Some(Ordering::Equal)
        }
    }
    impl Ord for () {
        fn cmp(&self, _other: &()) -> Ordering {
            Ordering::Equal
        }
    }

    impl PartialEq for bool {
        fn eq(&self, other: &bool) -> bool {
            *self == *other
        }
    }
    impl Eq for bool {}
    impl PartialOrd for bool {
        fn partial_cmp(&self, other: &bool) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for bool {
        fn cmp(&self, other: &bool) -> Ordering {
            (*self as u8).cmp(&(*other as u8))
        }
    }

    impl PartialEq for char {
        fn eq(&self, other: &char) -> bool {
            *self == *other
        }
    }
    impl Eq for char {}
    impl PartialOrd for char {
        fn partial_cmp(&self, other: &char) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for char {
        fn cmp(&self, other: &char) -> Ordering {
            (*self as u32).cmp(&(*other as u32))
        }
    }

    macro_rules! int_cmp {
        ($($t:ty)*) => {$(
            impl PartialEq for $t {
                fn eq(&self, other: &$t) -> bool {
                    *self == *other
                }
            }
            impl Eq for $t {}
            impl PartialOrd for $t {
                fn partial_cmp(&self, other: &$t) -> Option<Ordering> {
                    Some(self.cmp(other))
                }
                fn lt(&self, other: &$t) -> bool { *self < *other }
                fn le(&self, other: &$t) -> bool { *self <= *other }
                fn gt(&self, other: &$t) -> bool { *self > *other }
                fn ge(&self, other: &$t) -> bool { *self >= *other }
            }
            impl Ord for $t {
                fn cmp(&self, other: &$t) -> Ordering {
                    if *self < *other {
                        Ordering::Less
                    } else if *self == *other {
                        Ordering::Equal
                    } else {
                        Ordering::Greater
                    }
                }
            }
        )*};
    }
    int_cmp!(i8 i16 i32 i64 isize u8 u16 u32 u64 usize);

    macro_rules! float_cmp {
        ($($t:ty)*) => {$(
            impl PartialEq for $t {
                fn eq(&self, other: &$t) -> bool {
                    *self == *other
                }
            }
            impl PartialOrd for $t {
                fn partial_cmp(&self, other: &$t) -> Option<Ordering> {
                    if *self < *other {
                        Some(Ordering::Less)
                    } else if *self > *other {
                        Some(Ordering::Greater)
                    } else if *self == *other {
                        Some(Ordering::Equal)
                    } else {
                        None
                    }
                }
                fn lt(&self, other: &$t) -> bool { *self < *other }
                fn le(&self, other: &$t) -> bool { *self <= *other }
                fn gt(&self, other: &$t) -> bool { *self > *other }
                fn ge(&self, other: &$t) -> bool { *self >= *other }
            }
        )*};
    }
    float_cmp!(f32 f64);

    macro_rules! tuple_cmp {
        ($(($($T:ident $i:tt),+))*) => {$(
            impl<$($T: PartialEq),+> PartialEq for ($($T,)+) {
                fn eq(&self, other: &($($T,)+)) -> bool {
                    $(self.$i == other.$i)&&+
                }
            }
            impl<$($T: Eq),+> Eq for ($($T,)+) {}
            impl<$($T: PartialOrd),+> PartialOrd for ($($T,)+) {
                fn partial_cmp(&self, other: &($($T,)+)) -> Option<Ordering> {
                    $(
                        match self.$i.partial_cmp(&other.$i) {
                            Some(Ordering::Equal) => {}
                            c => return c,
                        }
                    )+
                    Some(Ordering::Equal)
                }
            }
            impl<$($T: Ord),+> Ord for ($($T,)+) {
                fn cmp(&self, other: &($($T,)+)) -> Ordering {
                    $(
                        match self.$i.cmp(&other.$i) {
                            Ordering::Equal => {}
                            c => return c,
                        }
                    )+
                    Ordering::Equal
                }
            }
        )*};
    }
    tuple_cmp! {
        (A 0)
        (A 0, B 1)
        (A 0, B 1, C 2)
        (A 0, B 1, C 2, D 3)
        (A 0, B 1, C 2, D 3, E 4)
        (A 0, B 1, C 2, D 3, E 4, F 5)
    }

    impl<T: PartialEq<U>, U, const N: usize> PartialEq<[U; N]> for [T; N] {
        fn eq(&self, other: &[U; N]) -> bool {
            let mut i = 0;
            while i < N {
                if self[i] != other[i] {
                    return false;
                }
                i += 1;
            }
            true
        }
    }
    impl<T: Eq, const N: usize> Eq for [T; N] {}
    impl<T: PartialOrd, const N: usize> PartialOrd for [T; N] {
        fn partial_cmp(&self, other: &[T; N]) -> Option<Ordering> {
            PartialOrd::partial_cmp(&self[..], &other[..])
        }
    }
    impl<T: Ord, const N: usize> Ord for [T; N] {
        fn cmp(&self, other: &[T; N]) -> Ordering {
            Ord::cmp(&self[..], &other[..])
        }
    }

    impl<T: PartialEq<U>, U> PartialEq<[U]> for [T] {
        fn eq(&self, other: &[U]) -> bool {
            if self.len() != other.len() {
                return false;
            }
            let mut i = 0;
            while i < self.len() {
                if self[i] != other[i] {
                    return false;
                }
                i += 1;
            }
            true
        }
    }
    impl<T: Eq> Eq for [T] {}
    impl<T: PartialOrd> PartialOrd for [T] {
        fn partial_cmp(&self, other: &[T]) -> Option<Ordering> {
            let n = if self.len() < other.len() { self.len() } else { other.len() };
            let mut i = 0;
            while i < n {
                match self[i].partial_cmp(&other[i]) {
                    Some(Ordering::Equal) => {}
                    c => return c,
                }
                i += 1;
            }
            self.len().partial_cmp(&other.len())
        }
    }
    impl<T: Ord> Ord for [T] {
        fn cmp(&self, other: &[T]) -> Ordering {
            let n = if self.len() < other.len() { self.len() } else { other.len() };
            let mut i = 0;
            while i < n {
                match self[i].cmp(&other[i]) {
                    Ordering::Equal => {}
                    c => return c,
                }
                i += 1;
            }
            self.len().cmp(&other.len())
        }
    }
    impl<T: PartialEq<U>, U, const N: usize> PartialEq<[U; N]> for [T] {
        fn eq(&self, other: &[U; N]) -> bool {
            PartialEq::eq(self, &other[..])
        }
    }

    impl PartialEq for str {
        fn eq(&self, other: &str) -> bool {
            self.as_bytes() == other.as_bytes()
        }
    }
    impl Eq for str {}
    impl PartialOrd for str {
        fn partial_cmp(&self, other: &str) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for str {
        fn cmp(&self, other: &str) -> Ordering {
            Ord::cmp(self.as_bytes(), other.as_bytes())
        }
    }
}

pub mod convert {
    pub trait From<T> {
        fn from(value: T) -> Self;
    }

    pub trait Into<T> {
        fn into(self) -> T;
    }

    impl<T, U: From<T>> Into<U> for T {
        fn into(self) -> U {
            U::from(self)
        }
    }

    impl<T> From<T> for T {
        fn from(t: T) -> T {
            t
        }
    }

    pub trait TryFrom<T>: Sized {
        type Error;
        fn try_from(value: T) -> Result<Self, Self::Error>;
    }

    pub trait TryInto<T> {
        type Error;
        fn try_into(self) -> Result<T, Self::Error>;
    }

    impl<T, U: TryFrom<T>> TryInto<U> for T {
        type Error = U::Error;
        fn try_into(self) -> Result<U, U::Error> {
            U::try_from(self)
        }
    }

    pub trait AsRef<T: ?Sized> {
        fn as_ref(&self) -> &T;
    }

    pub trait AsMut<T: ?Sized> {
        fn as_mut(&mut self) -> &mut T;
    }

    impl<'a, T: ?Sized + AsRef<U>, U: ?Sized> AsRef<U> for &'a T {
        fn as_ref(&self) -> &U {
            <T as AsRef<U>>::as_ref(*self)
        }
    }

    impl<'a, T: ?Sized + AsRef<U>, U: ?Sized> AsRef<U> for &'a mut T {
        fn as_ref(&self) -> &U {
            <T as AsRef<U>>::as_ref(&**self)
        }
    }

    impl<T> AsRef<[T]> for [T] {
        fn as_ref(&self) -> &[T] {
            self
        }
    }

    impl<T, const N: usize> AsRef<[T]> for [T; N] {
        fn as_ref(&self) -> &[T] {
            &self[..]
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Infallible {}
}

pub mod option {
    use crate::result::Result::{self, Err, Ok};

    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
    pub enum Option<T> {
        None,
        Some(T),
    }

    pub use self::Option::{None, Some};

    impl<T> Option<T> {
        pub fn is_some(&self) -> bool {
            match self {
                Some(_) => true,
                None => false,
            }
        }
        pub fn is_none(&self) -> bool {
            !self.is_some()
        }
        pub fn is_some_and<F: FnOnce(T) -> bool>(self, f: F) -> bool {
            match self {
                Some(x) => f(x),
                None => false,
            }
        }
        pub fn is_none_or<F: FnOnce(T) -> bool>(self, f: F) -> bool {
            match self {
                Some(x) => f(x),
                None => true,
            }
        }
        pub fn as_ref(&self) -> Option<&T> {
            match self {
                Some(x) => Some(x),
                None => None,
            }
        }
        pub fn as_mut(&mut self) -> Option<&mut T> {
            match self {
                Some(x) => Some(x),
                None => None,
            }
        }
        #[track_caller]
        pub fn unwrap(self) -> T {
            match self {
                Some(x) => x,
                None => crate::rt::begin_panic_str("called `Option::unwrap()` on a `None` value", crate::intrinsics::caller_location()),
            }
        }
        #[track_caller]
        pub fn expect(self, msg: &str) -> T {
            match self {
                Some(x) => x,
                None => crate::rt::begin_panic_str(msg, crate::intrinsics::caller_location()),
            }
        }
        pub fn unwrap_or(self, default: T) -> T {
            match self {
                Some(x) => x,
                None => default,
            }
        }
        pub fn unwrap_or_else<F: FnOnce() -> T>(self, f: F) -> T {
            match self {
                Some(x) => x,
                None => f(),
            }
        }
        pub fn unwrap_or_default(self) -> T
        where
            T: Default,
        {
            match self {
                Some(x) => x,
                None => T::default(),
            }
        }
        pub unsafe fn unwrap_unchecked(self) -> T {
            self.unwrap()
        }
        pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Option<U> {
            match self {
                Some(x) => Some(f(x)),
                None => None,
            }
        }
        pub fn map_or<U, F: FnOnce(T) -> U>(self, default: U, f: F) -> U {
            match self {
                Some(x) => f(x),
                None => default,
            }
        }
        pub fn map_or_else<U, D: FnOnce() -> U, F: FnOnce(T) -> U>(self, default: D, f: F) -> U {
            match self {
                Some(x) => f(x),
                None => default(),
            }
        }
        pub fn inspect<F: FnOnce(&T)>(self, f: F) -> Option<T> {
            if let Some(x) = &self {
                f(x);
            }
            self
        }
        pub fn ok_or<E>(self, err: E) -> Result<T, E> {
            match self {
                Some(x) => Ok(x),
                None => Err(err),
            }
        }
        pub fn ok_or_else<E, F: FnOnce() -> E>(self, f: F) -> Result<T, E> {
            match self {
                Some(x) => Ok(x),
                None => Err(f()),
            }
        }
        pub fn and<U>(self, other: Option<U>) -> Option<U> {
            match self {
                Some(_) => other,
                None => None,
            }
        }
        pub fn and_then<U, F: FnOnce(T) -> Option<U>>(self, f: F) -> Option<U> {
            match self {
                Some(x) => f(x),
                None => None,
            }
        }
        pub fn filter<P: FnOnce(&T) -> bool>(self, predicate: P) -> Option<T> {
            if let Some(x) = self {
                if predicate(&x) {
                    return Some(x);
                }
            }
            None
        }
        pub fn or(self, other: Option<T>) -> Option<T> {
            match self {
                Some(x) => Some(x),
                None => other,
            }
        }
        pub fn or_else<F: FnOnce() -> Option<T>>(self, f: F) -> Option<T> {
            match self {
                Some(x) => Some(x),
                None => f(),
            }
        }
        pub fn xor(self, other: Option<T>) -> Option<T> {
            match (self, other) {
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                _ => None,
            }
        }
        pub fn take(&mut self) -> Option<T> {
            crate::mem::replace(self, None)
        }
        pub fn replace(&mut self, value: T) -> Option<T> {
            crate::mem::replace(self, Some(value))
        }
        pub fn insert(&mut self, value: T) -> &mut T {
            *self = Some(value);
            match self {
                Some(x) => x,
                None => crate::intrinsics::abort(),
            }
        }
        pub fn get_or_insert(&mut self, value: T) -> &mut T {
            if self.is_none() {
                *self = Some(value);
            }
            match self {
                Some(x) => x,
                None => crate::intrinsics::abort(),
            }
        }
        pub fn get_or_insert_with<F: FnOnce() -> T>(&mut self, f: F) -> &mut T {
            if self.is_none() {
                *self = Some(f());
            }
            match self {
                Some(x) => x,
                None => crate::intrinsics::abort(),
            }
        }
        pub fn zip<U>(self, other: Option<U>) -> Option<(T, U)> {
            match (self, other) {
                (Some(a), Some(b)) => Some((a, b)),
                _ => None,
            }
        }
        pub fn iter(&self) -> crate::option::Iter<'_, T> {
            Iter { inner: self.as_ref() }
        }
        pub fn into_iter_inner(self) -> IntoIter<T> {
            IntoIter { inner: self }
        }
    }

    impl<T: Clone> Option<&T> {
        pub fn cloned(self) -> Option<T> {
            match self {
                Some(x) => Some(x.clone()),
                None => None,
            }
        }
    }

    impl<T: Copy> Option<&T> {
        pub fn copied(self) -> Option<T> {
            match self {
                Some(x) => Some(*x),
                None => None,
            }
        }
    }

    impl<T: Clone> Option<&mut T> {
        pub fn cloned(self) -> Option<T> {
            match self {
                Some(x) => Some(x.clone()),
                None => None,
            }
        }
    }

    impl<T: crate::ops::Deref> Option<T> {
        pub fn as_deref(&self) -> Option<&T::Target> {
            match self {
                Some(x) => Some(x.deref()),
                None => None,
            }
        }
    }

    impl<T> Option<Option<T>> {
        pub fn flatten(self) -> Option<T> {
            match self {
                Some(x) => x,
                None => None,
            }
        }
    }

    impl<T, E> Option<Result<T, E>> {
        pub fn transpose(self) -> Result<Option<T>, E> {
            match self {
                Some(Ok(x)) => Ok(Some(x)),
                Some(Err(e)) => Err(e),
                None => Ok(None),
            }
        }
    }

    impl<T> Default for Option<T> {
        fn default() -> Option<T> {
            None
        }
    }

    impl<T> From<T> for Option<T> {
        fn from(t: T) -> Option<T> {
            Some(t)
        }
    }

    pub struct Iter<'a, T> {
        inner: Option<&'a T>,
    }

    impl<'a, T> Iterator for Iter<'a, T> {
        type Item = &'a T;
        fn next(&mut self) -> Option<&'a T> {
            self.inner.take()
        }
    }

    pub struct IntoIter<T> {
        inner: Option<T>,
    }

    impl<T> Iterator for IntoIter<T> {
        type Item = T;
        fn next(&mut self) -> Option<T> {
            self.inner.take()
        }
    }

    impl<T> IntoIterator for Option<T> {
        type Item = T;
        type IntoIter = IntoIter<T>;
        fn into_iter(self) -> IntoIter<T> {
            IntoIter { inner: self }
        }
    }

    impl<'a, T> IntoIterator for &'a Option<T> {
        type Item = &'a T;
        type IntoIter = Iter<'a, T>;
        fn into_iter(self) -> Iter<'a, T> {
            self.iter()
        }
    }

    impl<A, V: crate::iter::FromIterator<A>> crate::iter::FromIterator<Option<A>> for Option<V> {
        fn from_iter<I: IntoIterator<Item = Option<A>>>(iter: I) -> Option<V> {
            let mut failed = false;
            let mut items = Vec::new();
            for x in iter {
                match x {
                    Some(v) => items.push(v),
                    None => {
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                None
            } else {
                Some(items.into_iter().collect())
            }
        }
    }

}

pub mod result {
    use crate::fmt;
    use crate::option::Option::{self, None, Some};

    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
    pub enum Result<T, E> {
        Ok(T),
        Err(E),
    }

    pub use self::Result::{Err, Ok};

    impl<T, E> Result<T, E> {
        pub fn is_ok(&self) -> bool {
            match self {
                Ok(_) => true,
                Err(_) => false,
            }
        }
        pub fn is_err(&self) -> bool {
            !self.is_ok()
        }
        pub fn is_ok_and<F: FnOnce(T) -> bool>(self, f: F) -> bool {
            match self {
                Ok(x) => f(x),
                Err(_) => false,
            }
        }
        pub fn is_err_and<F: FnOnce(E) -> bool>(self, f: F) -> bool {
            match self {
                Ok(_) => false,
                Err(e) => f(e),
            }
        }
        pub fn ok(self) -> Option<T> {
            match self {
                Ok(x) => Some(x),
                Err(_) => None,
            }
        }
        pub fn err(self) -> Option<E> {
            match self {
                Ok(_) => None,
                Err(e) => Some(e),
            }
        }
        pub fn as_ref(&self) -> Result<&T, &E> {
            match self {
                Ok(x) => Ok(x),
                Err(e) => Err(e),
            }
        }
        pub fn as_mut(&mut self) -> Result<&mut T, &mut E> {
            match self {
                Ok(x) => Ok(x),
                Err(e) => Err(e),
            }
        }
        pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Result<U, E> {
            match self {
                Ok(x) => Ok(f(x)),
                Err(e) => Err(e),
            }
        }
        pub fn map_err<G, F: FnOnce(E) -> G>(self, f: F) -> Result<T, G> {
            match self {
                Ok(x) => Ok(x),
                Err(e) => Err(f(e)),
            }
        }
        pub fn map_or<U, F: FnOnce(T) -> U>(self, default: U, f: F) -> U {
            match self {
                Ok(x) => f(x),
                Err(_) => default,
            }
        }
        pub fn map_or_else<U, D: FnOnce(E) -> U, F: FnOnce(T) -> U>(self, default: D, f: F) -> U {
            match self {
                Ok(x) => f(x),
                Err(e) => default(e),
            }
        }
        pub fn and_then<U, F: FnOnce(T) -> Result<U, E>>(self, f: F) -> Result<U, E> {
            match self {
                Ok(x) => f(x),
                Err(e) => Err(e),
            }
        }
        pub fn and<U>(self, other: Result<U, E>) -> Result<U, E> {
            match self {
                Ok(_) => other,
                Err(e) => Err(e),
            }
        }
        pub fn or<F>(self, other: Result<T, F>) -> Result<T, F> {
            match self {
                Ok(x) => Ok(x),
                Err(_) => other,
            }
        }
        pub fn or_else<F, O: FnOnce(E) -> Result<T, F>>(self, f: O) -> Result<T, F> {
            match self {
                Ok(x) => Ok(x),
                Err(e) => f(e),
            }
        }
        pub fn unwrap_or(self, default: T) -> T {
            match self {
                Ok(x) => x,
                Err(_) => default,
            }
        }
        pub fn unwrap_or_else<F: FnOnce(E) -> T>(self, f: F) -> T {
            match self {
                Ok(x) => x,
                Err(e) => f(e),
            }
        }
        pub fn unwrap_or_default(self) -> T
        where
            T: Default,
        {
            match self {
                Ok(x) => x,
                Err(_) => T::default(),
            }
        }
        pub fn iter(&self) -> crate::option::IntoIter<&T> {
            self.as_ref().ok().into_iter()
        }
    }

    impl<T, E: fmt::Debug> Result<T, E> {
        #[track_caller]
        pub fn unwrap(self) -> T {
            match self {
                Ok(x) => x,
                Err(e) => crate::rt::unwrap_failed("called `Result::unwrap()` on an `Err` value", &e, crate::intrinsics::caller_location()),
            }
        }
        #[track_caller]
        pub fn expect(self, msg: &str) -> T {
            match self {
                Ok(x) => x,
                Err(e) => crate::rt::unwrap_failed(msg, &e, crate::intrinsics::caller_location()),
            }
        }
    }

    impl<T: fmt::Debug, E> Result<T, E> {
        #[track_caller]
        pub fn unwrap_err(self) -> E {
            match self {
                Ok(x) => crate::rt::unwrap_failed("called `Result::unwrap_err()` on an `Ok` value", &x, crate::intrinsics::caller_location()),
                Err(e) => e,
            }
        }
        #[track_caller]
        pub fn expect_err(self, msg: &str) -> E {
            match self {
                Ok(x) => crate::rt::unwrap_failed(msg, &x, crate::intrinsics::caller_location()),
                Err(e) => e,
            }
        }
    }

    impl<T, E> Result<Option<T>, E> {
        pub fn transpose(self) -> Option<Result<T, E>> {
            match self {
                Ok(Some(x)) => Some(Ok(x)),
                Ok(None) => None,
                Err(e) => Some(Err(e)),
            }
        }
    }

    impl<T, E> IntoIterator for Result<T, E> {
        type Item = T;
        type IntoIter = crate::option::IntoIter<T>;
        fn into_iter(self) -> crate::option::IntoIter<T> {
            self.ok().into_iter()
        }
    }

    impl<A, E, V: crate::iter::FromIterator<A>> crate::iter::FromIterator<Result<A, E>> for Result<V, E> {
        fn from_iter<I: IntoIterator<Item = Result<A, E>>>(iter: I) -> Result<V, E> {
            let mut items = Vec::new();
            for x in iter {
                match x {
                    Ok(v) => items.push(v),
                    Err(e) => return Err(e),
                }
            }
            Ok(items.into_iter().collect())
        }
    }
}

pub mod mem {
    pub fn swap<T>(a: &mut T, b: &mut T) {
        unsafe {
            let t = crate::intrinsics::ptr_read(a as *const T);
            crate::intrinsics::ptr_write(a as *mut T, crate::intrinsics::ptr_read(b as *const T));
            crate::intrinsics::ptr_write(b as *mut T, t);
        }
    }

    pub fn replace<T>(dest: &mut T, src: T) -> T {
        unsafe {
            let old = crate::intrinsics::ptr_read(dest as *const T);
            crate::intrinsics::ptr_write(dest as *mut T, src);
            old
        }
    }

    pub fn take<T: Default>(dest: &mut T) -> T {
        replace(dest, T::default())
    }

    pub fn size_of<T>() -> usize {
        crate::intrinsics::size_of::<T>()
    }

    pub fn drop<T>(_x: T) {}

    pub fn forget<T>(_x: T) {}

    pub fn discriminant<T>(v: &T) -> Discriminant {
        Discriminant(crate::intrinsics::discriminant(v))
    }

    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct Discriminant(i64);
}

pub mod ptr {
    pub fn null<T>() -> *const T {
        crate::intrinsics::ptr_null::<T>() as *const T
    }

    pub fn null_mut<T>() -> *mut T {
        crate::intrinsics::ptr_null::<T>()
    }

    pub unsafe fn read<T>(p: *const T) -> T {
        crate::intrinsics::ptr_read(p)
    }

    pub unsafe fn write<T>(p: *mut T, v: T) {
        crate::intrinsics::ptr_write(p, v)
    }

    pub fn eq<T>(a: *const T, b: *const T) -> bool {
        a == b
    }

    impl<T> *const T {
        pub fn is_null(self) -> bool {
            self as usize == 0
        }
        pub unsafe fn add(self, n: usize) -> *const T {
            crate::intrinsics::ptr_add(self as *mut T, n) as *const T
        }
        pub unsafe fn sub(self, n: usize) -> *const T {
            crate::intrinsics::ptr_sub(self as *mut T, n) as *const T
        }
        pub unsafe fn offset(self, n: isize) -> *const T {
            if n >= 0 {
                self.add(n as usize)
            } else {
                self.sub((-n) as usize)
            }
        }
        pub unsafe fn read(self) -> T {
            crate::intrinsics::ptr_read(self)
        }
        pub fn cast<U>(self) -> *const U {
            self as *const U
        }
        pub unsafe fn offset_from(self, origin: *const T) -> isize {
            crate::intrinsics::ptr_diff(self, origin)
        }
    }

    impl<T> *mut T {
        pub fn is_null(self) -> bool {
            self as usize == 0
        }
        pub unsafe fn add(self, n: usize) -> *mut T {
            crate::intrinsics::ptr_add(self, n)
        }
        pub unsafe fn sub(self, n: usize) -> *mut T {
            crate::intrinsics::ptr_sub(self, n)
        }
        pub unsafe fn offset(self, n: isize) -> *mut T {
            if n >= 0 {
                self.add(n as usize)
            } else {
                self.sub((-n) as usize)
            }
        }
        pub unsafe fn read(self) -> T {
            crate::intrinsics::ptr_read(self as *const T)
        }
        pub unsafe fn write(self, v: T) {
            crate::intrinsics::ptr_write(self, v)
        }
        pub fn cast<U>(self) -> *mut U {
            self as *mut U
        }
    }
}

pub mod alloc {
    pub unsafe fn alloc_array<T>(n: usize) -> *mut T {
        let size = crate::intrinsics::size_of::<T>() * n;
        crate::sys::malloc(if size == 0 { 1 } else { size }) as *mut T
    }

    pub unsafe fn realloc_array<T>(p: *mut T, n: usize) -> *mut T {
        let size = crate::intrinsics::size_of::<T>() * n;
        crate::sys::realloc(p as *mut u8, if size == 0 { 1 } else { size }) as *mut T
    }
}

pub mod boxed {
    use crate::fmt;

    /// A pointer to a heap value (the compiler represents it as a plain or
    /// fat pointer and dereferences it itself).
    pub struct Box<T: ?Sized> {
        ptr: *mut T,
    }

    impl<T> Box<T> {
        pub fn new(x: T) -> Box<T> {
            unsafe {
                let p = crate::alloc::alloc_array::<T>(1);
                crate::intrinsics::ptr_write(p, x);
                crate::boxed::from_raw(p)
            }
        }
        pub fn into_inner(b: Box<T>) -> T {
            *b
        }
        pub fn leak<'a>(b: Box<T>) -> &'a mut T {
            unsafe { &mut *crate::boxed::into_raw(b) }
        }
    }

    extern "rust-intrinsic" {
        pub fn from_raw<T>(p: *mut T) -> Box<T>;
        pub fn into_raw<T>(b: Box<T>) -> *mut T;
    }

    impl<T: Clone> Clone for Box<T> {
        fn clone(&self) -> Box<T> {
            Box::new((**self).clone())
        }
    }

    impl<T: ?Sized + PartialEq> PartialEq for Box<T> {
        fn eq(&self, other: &Box<T>) -> bool {
            PartialEq::eq(&**self, &**other)
        }
    }
    impl<T: ?Sized + Eq> Eq for Box<T> {}
    impl<T: ?Sized + PartialOrd> PartialOrd for Box<T> {
        fn partial_cmp(&self, other: &Box<T>) -> Option<crate::cmp::Ordering> {
            PartialOrd::partial_cmp(&**self, &**other)
        }
    }
    impl<T: ?Sized + Ord> Ord for Box<T> {
        fn cmp(&self, other: &Box<T>) -> crate::cmp::Ordering {
            Ord::cmp(&**self, &**other)
        }
    }

    impl<T: ?Sized + fmt::Display> fmt::Display for Box<T> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            fmt::Display::fmt(&**self, f)
        }
    }

    impl<T: ?Sized + fmt::Debug> fmt::Debug for Box<T> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            fmt::Debug::fmt(&**self, f)
        }
    }

    impl<T: Default> Default for Box<T> {
        fn default() -> Box<T> {
            Box::new(T::default())
        }
    }

    impl<T> From<T> for Box<T> {
        fn from(t: T) -> Box<T> {
            Box::new(t)
        }
    }

    impl<T: ?Sized + crate::hash::Hash> crate::hash::Hash for Box<T> {
        fn hash<H: crate::hash::Hasher>(&self, state: &mut H) {
            crate::hash::Hash::hash(&**self, state)
        }
    }

    impl<T: ?Sized> AsRef<T> for Box<T> {
        fn as_ref(&self) -> &T {
            &**self
        }
    }

    impl<I: Iterator + ?Sized> Iterator for Box<I> {
        type Item = I::Item;
        fn next(&mut self) -> Option<I::Item> {
            (**self).next()
        }
    }
}

pub mod panic {
    #[derive(Clone, Copy, Debug)]
    pub struct Location {
        file: &'static str,
        line: u32,
        col: u32,
    }

    impl Location {
        #[track_caller]
        pub fn caller() -> &'static Location {
            crate::intrinsics::caller_location()
        }
        pub fn file(&self) -> &str {
            self.file
        }
        pub fn line(&self) -> u32 {
            self.line
        }
        pub fn column(&self) -> u32 {
            self.col
        }
    }

    impl crate::fmt::Display for Location {
        fn fmt(&self, f: &mut crate::fmt::Formatter) -> crate::fmt::Result {
            write!(f, "{}:{}:{}", self.file, self.line, self.col)
        }
    }
}

/// Panics and other support for compiled code.
pub mod rt {
    use crate::fmt;
    use crate::panic::Location;

    pub fn begin_panic(msg: String, loc: &Location) -> ! {
        unsafe {
            crate::sys::hrc_panic_start(loc);
            crate::sys::hrc_write_stderr(msg.as_ptr(), msg.len());
            crate::sys::hrc_panic_end()
        }
    }

    pub fn begin_panic_str(msg: &str, loc: &Location) -> ! {
        unsafe {
            crate::sys::hrc_panic_start(loc);
            crate::sys::hrc_write_stderr(msg.as_ptr(), msg.len());
            crate::sys::hrc_panic_end()
        }
    }

    pub fn unwrap_failed(msg: &str, e: &dyn fmt::Debug, loc: &Location) -> ! {
        begin_panic(format!("{}: {:?}", msg, e), loc)
    }

    pub fn assert_failed<T: fmt::Debug + ?Sized, U: fmt::Debug + ?Sized>(eq: bool, left: &T, right: &U, args: Option<String>, loc: &Location) -> ! {
        let op = if eq { "==" } else { "!=" };
        let msg = match args {
            Some(a) => format!("assertion `left {} right` failed: {}\n  left: {:?}\n right: {:?}", op, a, left, right),
            None => format!("assertion `left {} right` failed\n  left: {:?}\n right: {:?}", op, left, right),
        };
        begin_panic(msg, loc)
    }

    pub fn dbg_print<T: fmt::Debug>(loc: &Location, text: &str, value: &T) {
        crate::io::_eprint(&format!("[{}:{}:{}] {} = {:#?}\n", loc.file(), loc.line(), loc.column(), text, value));
    }

    pub fn report_main_error<T, E: fmt::Debug>(r: Result<T, E>) -> i32 {
        match r {
            Ok(_) => 0,
            Err(e) => {
                crate::io::_eprint(&format!("Error: {:?}\n", e));
                1
            }
        }
    }
}

pub mod borrow;
pub mod cell;
pub mod char;
pub mod collections;
pub mod env;
pub mod error;
pub mod fmt;
pub mod fs;
pub mod hash;
pub mod io;
pub mod iter;
pub mod num;
pub mod prelude;
pub mod process;
pub mod rc;
pub mod slice;
pub mod str;
pub mod string;
pub mod time;
pub mod vec;

pub mod thread {
    pub fn sleep(d: crate::time::Duration) {
        crate::time::sleep(d)
    }
    pub fn yield_now() {}
}

pub mod array {
    pub fn from_fn<T, F: FnMut(usize) -> T, const N: usize>(mut f: F) -> [T; N] {
        let mut v = crate::vec::Vec::with_capacity(N);
        let mut i = 0;
        while i < N {
            v.push(f(i));
            i += 1;
        }
        unsafe { crate::intrinsics::ptr_read(v.as_ptr() as *const [T; N]) }
    }
}

pub mod sync {
    pub use crate::rc::Rc as Arc;
    pub use crate::rc::Weak;
}

/// `std::f64::consts::PI`, `std::i32::MAX`, ...
pub use crate::num::f32;
pub use crate::num::f64;
pub use crate::num::int_mods::{i16, i32, i64, i8, isize, u16, u32, u64, u8, usize};
