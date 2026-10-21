//! Interior mutability: `Cell`, `RefCell` and `OnceCell`.
//!
//! Borrows of a `RefCell` are not tracked (hrc has no destructors to end
//! them), so `borrow_mut` never panics.

use crate::fmt;
use crate::option::Option::{self, None, Some};

pub struct UnsafeCell<T> {
    value: T,
}

impl<T> UnsafeCell<T> {
    pub fn new(value: T) -> UnsafeCell<T> {
        UnsafeCell { value }
    }
    pub fn get(&self) -> *mut T {
        &self.value as *const T as *mut T
    }
    pub fn into_inner(self) -> T {
        self.value
    }
}

pub struct Cell<T> {
    value: UnsafeCell<T>,
}

impl<T> Cell<T> {
    pub fn new(value: T) -> Cell<T> {
        Cell {
            value: UnsafeCell::new(value),
        }
    }
    pub fn set(&self, value: T) {
        unsafe {
            *self.value.get() = value;
        }
    }
    pub fn replace(&self, value: T) -> T {
        unsafe { crate::mem::replace(&mut *self.value.get(), value) }
    }
    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }
    pub fn get_mut(&mut self) -> &mut T {
        unsafe { &mut *self.value.get() }
    }
    pub fn as_ptr(&self) -> *mut T {
        self.value.get()
    }
}

impl<T: Copy> Cell<T> {
    pub fn get(&self) -> T {
        unsafe { *self.value.get() }
    }
    pub fn update<F: FnOnce(T) -> T>(&self, f: F) -> T {
        let new = f(self.get());
        self.set(new);
        new
    }
}

impl<T: Default> Cell<T> {
    pub fn take(&self) -> T {
        self.replace(T::default())
    }
}

impl<T: Copy> Clone for Cell<T> {
    fn clone(&self) -> Cell<T> {
        Cell::new(self.get())
    }
}

impl<T: Default> Default for Cell<T> {
    fn default() -> Cell<T> {
        Cell::new(T::default())
    }
}

impl<T: Copy + PartialEq> PartialEq for Cell<T> {
    fn eq(&self, other: &Cell<T>) -> bool {
        self.get() == other.get()
    }
}

impl<T: Copy + fmt::Debug> fmt::Debug for Cell<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Cell").field("value", &self.get()).finish()
    }
}

pub struct RefCell<T> {
    value: UnsafeCell<T>,
}

pub struct Ref<'b, T> {
    value: &'b T,
}

pub struct RefMut<'b, T> {
    value: &'b mut T,
}

#[derive(Debug)]
pub struct BorrowError;

#[derive(Debug)]
pub struct BorrowMutError;

impl<T> RefCell<T> {
    pub fn new(value: T) -> RefCell<T> {
        RefCell {
            value: UnsafeCell::new(value),
        }
    }
    pub fn borrow(&self) -> Ref<'_, T> {
        Ref {
            value: unsafe { &*self.value.get() },
        }
    }
    pub fn borrow_mut(&self) -> RefMut<'_, T> {
        RefMut {
            value: unsafe { &mut *self.value.get() },
        }
    }
    pub fn try_borrow(&self) -> Result<Ref<'_, T>, BorrowError> {
        Ok(self.borrow())
    }
    pub fn try_borrow_mut(&self) -> Result<RefMut<'_, T>, BorrowMutError> {
        Ok(self.borrow_mut())
    }
    pub fn replace(&self, value: T) -> T {
        unsafe { crate::mem::replace(&mut *self.value.get(), value) }
    }
    pub fn replace_with<F: FnOnce(&mut T) -> T>(&self, f: F) -> T {
        let v = unsafe { &mut *self.value.get() };
        let new = f(v);
        self.replace(new)
    }
    pub fn swap(&self, other: &RefCell<T>) {
        unsafe { crate::mem::swap(&mut *self.value.get(), &mut *other.value.get()) }
    }
    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }
    pub fn get_mut(&mut self) -> &mut T {
        unsafe { &mut *self.value.get() }
    }
    pub fn as_ptr(&self) -> *mut T {
        self.value.get()
    }
}

impl<T: Default> RefCell<T> {
    pub fn take(&self) -> T {
        self.replace(T::default())
    }
}

impl<T: Clone> Clone for RefCell<T> {
    fn clone(&self) -> RefCell<T> {
        RefCell::new(self.borrow().clone())
    }
}

impl<T: Default> Default for RefCell<T> {
    fn default() -> RefCell<T> {
        RefCell::new(T::default())
    }
}

impl<T: PartialEq> PartialEq for RefCell<T> {
    fn eq(&self, other: &RefCell<T>) -> bool {
        *self.borrow() == *other.borrow()
    }
}

impl<T: fmt::Debug> fmt::Debug for RefCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RefCell").field("value", &*self.borrow()).finish()
    }
}

impl<'b, T> Ref<'b, T> {
    pub fn clone(orig: &Ref<'b, T>) -> Ref<'b, T> {
        Ref { value: orig.value }
    }
    pub fn map<U, F: FnOnce(&T) -> &U>(orig: Ref<'b, T>, f: F) -> Ref<'b, U> {
        Ref { value: f(orig.value) }
    }
}

impl<'b, T> RefMut<'b, T> {
    pub fn map<U, F: FnOnce(&mut T) -> &mut U>(orig: RefMut<'b, T>, f: F) -> RefMut<'b, U> {
        RefMut { value: f(orig.value) }
    }
}

impl<'b, T> crate::ops::Deref for Ref<'b, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value
    }
}

impl<'b, T> crate::ops::Deref for RefMut<'b, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value
    }
}

impl<'b, T> crate::ops::DerefMut for RefMut<'b, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value
    }
}

impl<'b, T: fmt::Display> fmt::Display for Ref<'b, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self.value, f)
    }
}

impl<'b, T: fmt::Debug> fmt::Debug for Ref<'b, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self.value, f)
    }
}

impl<'b, T: fmt::Display> fmt::Display for RefMut<'b, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&*self.value, f)
    }
}

impl<'b, T: fmt::Debug> fmt::Debug for RefMut<'b, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&*self.value, f)
    }
}

pub struct OnceCell<T> {
    value: UnsafeCell<Option<T>>,
}

impl<T> OnceCell<T> {
    pub fn new() -> OnceCell<T> {
        OnceCell {
            value: UnsafeCell::new(None),
        }
    }
    pub fn get(&self) -> Option<&T> {
        unsafe { (*self.value.get()).as_ref() }
    }
    pub fn set(&self, value: T) -> Result<(), T> {
        unsafe {
            if (*self.value.get()).is_some() {
                return Err(value);
            }
            *self.value.get() = Some(value);
        }
        Ok(())
    }
    pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> &T {
        unsafe {
            if (*self.value.get()).is_none() {
                *self.value.get() = Some(f());
            }
            (*self.value.get()).as_ref().unwrap()
        }
    }
}
