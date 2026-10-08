//! C types (x86-64 LP64 layout).

use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;

#[derive(Clone, Debug)]
pub enum Type {
    Void,
    Bool,
    Int {
        size: u8,
        signed: bool,
    },
    Float,
    /// `double` and `long double`.
    Double,
    Ptr(Rc<Type>),
    /// Element type and length (`None` while incomplete: `int a[]`).
    Array(Rc<Type>, Option<usize>),
    Func(Rc<FuncType>),
    Record(Rc<RefCell<Record>>),
}

#[derive(Clone, Debug)]
pub struct FuncType {
    pub ret: Type,
    pub params: Vec<Type>,
    pub variadic: bool,
    /// Declared with a parameter list (not `f()`).
    pub prototype: bool,
}

#[derive(Debug, Default)]
pub struct Record {
    pub members: Vec<Member>,
    pub size: usize,
    pub align: usize,
    pub complete: bool,
    pub union: bool,
}

#[derive(Clone, Debug)]
pub struct Member {
    /// `None` for anonymous struct/union members.
    pub name: Option<Rc<str>>,
    pub ty: Type,
    pub offset: usize,
}

pub const CHAR: Type = Type::Int {
    size: 1,
    signed: true,
};
pub const INT: Type = Type::Int {
    size: 4,
    signed: true,
};
pub const UINT: Type = Type::Int {
    size: 4,
    signed: false,
};
pub const LONG: Type = Type::Int {
    size: 8,
    signed: true,
};
pub const ULONG: Type = Type::Int {
    size: 8,
    signed: false,
};

pub fn align_to(n: usize, a: usize) -> usize {
    n.div_ceil(a.max(1)) * a.max(1)
}

impl Type {
    pub fn ptr(to: Type) -> Type {
        Type::Ptr(Rc::new(to))
    }

    pub fn size(&self) -> usize {
        match self {
            Type::Void | Type::Bool => 1,
            Type::Int { size, .. } => *size as usize,
            Type::Float => 4,
            Type::Double | Type::Ptr(_) => 8,
            Type::Array(e, n) => e.size() * n.unwrap_or(0),
            Type::Func(_) => 1,
            Type::Record(r) => r.borrow().size,
        }
    }

    pub fn align(&self) -> usize {
        match self {
            Type::Array(e, _) => e.align(),
            Type::Record(r) => r.borrow().align.max(1),
            Type::Func(_) | Type::Void => 1,
            _ => self.size(),
        }
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Type::Int { .. } | Type::Bool)
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Type::Float | Type::Double)
    }

    pub fn is_arith(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    /// Pointers and things that decay into pointers.
    pub fn is_pointer_like(&self) -> bool {
        matches!(self, Type::Ptr(_) | Type::Array(..))
    }

    pub fn is_scalar(&self) -> bool {
        self.is_arith() || matches!(self, Type::Ptr(_))
    }

    pub fn is_void(&self) -> bool {
        matches!(self, Type::Void)
    }

    pub fn is_record(&self) -> bool {
        matches!(self, Type::Record(_))
    }

    pub fn is_func(&self) -> bool {
        matches!(self, Type::Func(_))
    }

    pub fn is_unsigned(&self) -> bool {
        matches!(
            self,
            Type::Int { signed: false, .. } | Type::Bool | Type::Ptr(_)
        )
    }

    /// Pointed-to / element type.
    pub fn base(&self) -> Option<&Type> {
        match self {
            Type::Ptr(t) | Type::Array(t, _) => Some(t),
            _ => None,
        }
    }

    pub fn func(&self) -> Option<&FuncType> {
        match self {
            Type::Func(f) => Some(f),
            Type::Ptr(t) => match &**t {
                Type::Func(f) => Some(f),
                _ => None,
            },
            _ => None,
        }
    }

    /// The type an expression of this type has as an rvalue.
    pub fn decay(&self) -> Type {
        match self {
            Type::Array(e, _) => Type::Ptr(e.clone()),
            Type::Func(_) => Type::ptr(self.clone()),
            _ => self.clone(),
        }
    }

    /// Integer promotion.
    pub fn promote(&self) -> Type {
        match self {
            Type::Bool => INT,
            Type::Int { size, .. } if *size < 4 => INT,
            _ => self.clone(),
        }
    }

    pub fn same(&self, other: &Type) -> bool {
        match (self, other) {
            (Type::Void, Type::Void)
            | (Type::Bool, Type::Bool)
            | (Type::Float, Type::Float)
            | (Type::Double, Type::Double) => true,
            (Type::Int { size: a, signed: s }, Type::Int { size: b, signed: t }) => {
                a == b && s == t
            }
            (Type::Ptr(a), Type::Ptr(b)) => a.same(b),
            (Type::Array(a, n), Type::Array(b, m)) => n == m && a.same(b),
            (Type::Record(a), Type::Record(b)) => Rc::ptr_eq(a, b),
            (Type::Func(a), Type::Func(b)) => {
                a.ret.same(&b.ret) && a.params.len() == b.params.len()
            }
            _ => false,
        }
    }

    /// Finds a member, searching anonymous members too; returns its type
    /// and offset from the start of the record.
    pub fn member(&self, name: &str) -> Option<(Type, usize)> {
        let Type::Record(r) = self else { return None };
        for m in &r.borrow().members {
            match &m.name {
                Some(n) if &**n == name => return Some((m.ty.clone(), m.offset)),
                None => {
                    if let Some((t, o)) = m.ty.member(name) {
                        return Some((t, o + m.offset));
                    }
                }
                _ => {}
            }
        }
        None
    }
}

/// The usual arithmetic conversions.
pub fn common(a: &Type, b: &Type) -> Type {
    if a.is_pointer_like() {
        return a.decay();
    }
    if b.is_pointer_like() {
        return b.decay();
    }
    if matches!(a, Type::Double) || matches!(b, Type::Double) {
        return Type::Double;
    }
    if matches!(a, Type::Float) || matches!(b, Type::Float) {
        return Type::Float;
    }
    let (a, b) = (a.promote(), b.promote());
    let (
        Type::Int {
            size: sa,
            signed: ga,
        },
        Type::Int {
            size: sb,
            signed: gb,
        },
    ) = (&a, &b)
    else {
        return LONG;
    };
    let size = (*sa).max(*sb);
    let signed = if sa == sb {
        *ga && *gb
    } else if sa > sb {
        *ga
    } else {
        *gb
    };
    Type::Int { size, signed }
}
