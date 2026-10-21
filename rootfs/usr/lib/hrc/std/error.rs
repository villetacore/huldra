//! The `Error` trait.

use crate::boxed::Box;
use crate::fmt;
use crate::option::Option::{self, None};
use crate::string::String;

pub trait Error: fmt::Debug + fmt::Display {
    fn source(&self) -> Option<&dyn Error> {
        None
    }
    fn description(&self) -> &str {
        "description() is deprecated; use Display"
    }
    fn cause(&self) -> Option<&dyn Error> {
        self.source()
    }
}

impl<'a, E: Error + 'a> From<E> for Box<dyn Error + 'a> {
    fn from(err: E) -> Box<dyn Error + 'a> {
        Box::new(err)
    }
}

struct StringError(String);

impl fmt::Display for StringError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for StringError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl Error for StringError {}

impl<'a> From<String> for Box<dyn Error + 'a> {
    fn from(s: String) -> Box<dyn Error + 'a> {
        Box::new(StringError(s))
    }
}

impl<'a, 'b> From<&'b str> for Box<dyn Error + 'a> {
    fn from(s: &'b str) -> Box<dyn Error + 'a> {
        Box::new(StringError(String::from(s)))
    }
}

impl Error for fmt::Error {}
impl Error for crate::str::ParseIntError {}
impl Error for crate::str::ParseFloatError {}
impl Error for crate::str::ParseBoolError {}
impl Error for crate::str::ParseCharError {}
impl Error for crate::str::Utf8Error {}
impl Error for crate::string::FromUtf8Error {}
impl Error for crate::num::TryFromIntError {}
impl Error for crate::char::CharTryFromError {}
impl Error for crate::env::VarError {}
