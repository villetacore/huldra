//! `Borrow`, `ToOwned` and `Cow`.

use crate::fmt;
use crate::string::String;
use crate::vec::Vec;

pub trait Borrow<B: ?Sized> {
    fn borrow(&self) -> &B;
}

pub trait BorrowMut<B: ?Sized>: Borrow<B> {
    fn borrow_mut(&mut self) -> &mut B;
}

impl<T: ?Sized> Borrow<T> for T {
    fn borrow(&self) -> &T {
        self
    }
}

impl<T: ?Sized> Borrow<T> for &T {
    fn borrow(&self) -> &T {
        &**self
    }
}

impl<T: ?Sized> Borrow<T> for &mut T {
    fn borrow(&self) -> &T {
        &**self
    }
}

pub trait ToOwned {
    type Owned;
    fn to_owned(&self) -> Self::Owned;
}

impl<T: Clone> ToOwned for T {
    type Owned = T;
    fn to_owned(&self) -> T {
        self.clone()
    }
}

impl ToOwned for str {
    type Owned = String;
    fn to_owned(&self) -> String {
        String::from(self)
    }
}

impl<T: Clone> ToOwned for [T] {
    type Owned = Vec<T>;
    fn to_owned(&self) -> Vec<T> {
        self.to_vec()
    }
}

/// A borrowed or owned string.
pub enum Cow<'a, B: ?Sized + ToOwned> {
    Borrowed(&'a B),
    Owned(B::Owned),
}

impl<'a> Cow<'a, str> {
    pub fn into_owned(self) -> String {
        match self {
            Cow::Borrowed(s) => String::from(s),
            Cow::Owned(s) => s,
        }
    }
    pub fn is_borrowed(&self) -> bool {
        match self {
            Cow::Borrowed(_) => true,
            Cow::Owned(_) => false,
        }
    }
    pub fn to_mut(&mut self) -> &mut String {
        if let Cow::Borrowed(s) = self {
            *self = Cow::Owned(String::from(*s));
        }
        match self {
            Cow::Owned(s) => s,
            Cow::Borrowed(_) => crate::intrinsics::abort(),
        }
    }
}

impl<'a> crate::ops::Deref for Cow<'a, str> {
    type Target = str;
    fn deref(&self) -> &str {
        match self {
            Cow::Borrowed(s) => s,
            Cow::Owned(s) => s.as_str(),
        }
    }
}

impl<'a> fmt::Display for Cow<'a, str> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.pad(&**self)
    }
}

impl<'a> fmt::Debug for Cow<'a, str> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<'a> From<&'a str> for Cow<'a, str> {
    fn from(s: &'a str) -> Cow<'a, str> {
        Cow::Borrowed(s)
    }
}

impl<'a> From<String> for Cow<'a, str> {
    fn from(s: String) -> Cow<'a, str> {
        Cow::Owned(s)
    }
}
