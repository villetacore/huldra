//! `Rc<T>` (reference counting) and `Weak<T>`.
//!
//! hrc does not run destructors, so values are never freed and counts
//! only go up when clones are made.

use crate::fmt;
use crate::option::Option::{self, None, Some};

struct RcBox<T> {
    strong: usize,
    weak: usize,
    value: T,
}

pub struct Rc<T> {
    ptr: *mut RcBox<T>,
}

pub struct Weak<T> {
    ptr: *mut RcBox<T>,
}

impl<T> Rc<T> {
    pub fn new(value: T) -> Rc<T> {
        unsafe {
            let p = crate::alloc::alloc_array::<RcBox<T>>(1);
            crate::intrinsics::ptr_write(p, RcBox { strong: 1, weak: 0, value });
            Rc { ptr: p }
        }
    }
    pub fn strong_count(this: &Rc<T>) -> usize {
        unsafe { (*this.ptr).strong }
    }
    pub fn weak_count(this: &Rc<T>) -> usize {
        unsafe { (*this.ptr).weak }
    }
    pub fn ptr_eq(a: &Rc<T>, b: &Rc<T>) -> bool {
        a.ptr == b.ptr
    }
    pub fn as_ptr(this: &Rc<T>) -> *const T {
        unsafe { &(*this.ptr).value as *const T }
    }
    pub fn downgrade(this: &Rc<T>) -> Weak<T> {
        unsafe {
            (*this.ptr).weak += 1;
        }
        Weak { ptr: this.ptr }
    }
    pub fn get_mut(this: &mut Rc<T>) -> Option<&mut T> {
        unsafe {
            if (*this.ptr).strong == 1 {
                Some(&mut (*this.ptr).value)
            } else {
                None
            }
        }
    }
    pub fn try_unwrap(this: Rc<T>) -> Result<T, Rc<T>> {
        unsafe {
            if (*this.ptr).strong == 1 {
                (*this.ptr).strong = 0;
                Ok(crate::intrinsics::ptr_read(&(*this.ptr).value as *const T))
            } else {
                Err(this)
            }
        }
    }
    pub fn into_inner(this: Rc<T>) -> Option<T> {
        Rc::try_unwrap(this).ok()
    }
    pub fn unwrap_or_clone(this: Rc<T>) -> T
    where
        T: Clone,
    {
        (*this).clone()
    }
}

impl<T: Clone> Rc<T> {
    pub fn make_mut(this: &mut Rc<T>) -> &mut T {
        unsafe {
            if (*this.ptr).strong != 1 {
                *this = Rc::new((*this.ptr).value.clone());
            }
            &mut (*this.ptr).value
        }
    }
}

impl<T> Clone for Rc<T> {
    fn clone(&self) -> Rc<T> {
        unsafe {
            (*self.ptr).strong += 1;
        }
        Rc { ptr: self.ptr }
    }
}

impl<T> crate::ops::Deref for Rc<T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &(*self.ptr).value }
    }
}

impl<T> AsRef<T> for Rc<T> {
    fn as_ref(&self) -> &T {
        unsafe { &(*self.ptr).value }
    }
}

impl<T> crate::borrow::Borrow<T> for Rc<T> {
    fn borrow(&self) -> &T {
        unsafe { &(*self.ptr).value }
    }
}

impl<T: Default> Default for Rc<T> {
    fn default() -> Rc<T> {
        Rc::new(T::default())
    }
}

impl<T> From<T> for Rc<T> {
    fn from(t: T) -> Rc<T> {
        Rc::new(t)
    }
}

impl<T: PartialEq> PartialEq for Rc<T> {
    fn eq(&self, other: &Rc<T>) -> bool {
        **self == **other
    }
}
impl<T: Eq> Eq for Rc<T> {}
impl<T: PartialOrd> PartialOrd for Rc<T> {
    fn partial_cmp(&self, other: &Rc<T>) -> Option<crate::cmp::Ordering> {
        (**self).partial_cmp(&**other)
    }
}
impl<T: Ord> Ord for Rc<T> {
    fn cmp(&self, other: &Rc<T>) -> crate::cmp::Ordering {
        (**self).cmp(&**other)
    }
}
impl<T: crate::hash::Hash> crate::hash::Hash for Rc<T> {
    fn hash<H: crate::hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state)
    }
}

impl<T: fmt::Display> fmt::Display for Rc<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: fmt::Debug> fmt::Debug for Rc<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T> Weak<T> {
    pub fn new() -> Weak<T> {
        Weak {
            ptr: crate::intrinsics::ptr_null(),
        }
    }
    pub fn upgrade(&self) -> Option<Rc<T>> {
        if self.ptr.is_null() {
            return None;
        }
        unsafe {
            if (*self.ptr).strong == 0 {
                return None;
            }
            (*self.ptr).strong += 1;
        }
        Some(Rc { ptr: self.ptr })
    }
    pub fn strong_count(&self) -> usize {
        if self.ptr.is_null() {
            0
        } else {
            unsafe { (*self.ptr).strong }
        }
    }
    pub fn ptr_eq(&self, other: &Weak<T>) -> bool {
        self.ptr == other.ptr
    }
}

impl<T> Clone for Weak<T> {
    fn clone(&self) -> Weak<T> {
        Weak { ptr: self.ptr }
    }
}

impl<T> Default for Weak<T> {
    fn default() -> Weak<T> {
        Weak::new()
    }
}

impl<T> fmt::Debug for Weak<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("(Weak)")
    }
}
