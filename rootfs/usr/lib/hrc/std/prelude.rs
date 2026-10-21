//! Names every module sees.

pub use crate::borrow::ToOwned;
pub use crate::boxed::Box;
pub use crate::clone::Clone;
pub use crate::cmp::{Eq, Ord, PartialEq, PartialOrd};
pub use crate::convert::{AsMut, AsRef, From, Into, TryFrom, TryInto};
pub use crate::default::Default;
pub use crate::iter::{DoubleEndedIterator, ExactSizeIterator, Extend, FromIterator, IntoIterator, Iterator};
pub use crate::marker::{Copy, Send, Sized, Sync, Unpin};
pub use crate::mem::drop;
pub use crate::ops::{Drop, Fn, FnMut, FnOnce};
pub use crate::option::Option::{self, None, Some};
pub use crate::result::Result::{self, Err, Ok};
pub use crate::string::{String, ToString};
pub use crate::vec::Vec;

pub mod v1 {
    pub use crate::prelude::*;
}

pub mod rust_2021 {
    pub use crate::prelude::*;
}
