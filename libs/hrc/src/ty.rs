//! Semantic types. Generic parameters are `Param(i)` (an index into the
//! item's generics); inference variables exist only while a function is
//! being checked.

use crate::ast::Name;
use crate::resolve::DefId;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IntTy {
    I8,
    I16,
    I32,
    I64,
    I128,
    Isize,
    U8,
    U16,
    U32,
    U64,
    U128,
    Usize,
}

impl IntTy {
    pub fn name(self) -> &'static str {
        match self {
            IntTy::I8 => "i8",
            IntTy::I16 => "i16",
            IntTy::I32 => "i32",
            IntTy::I64 => "i64",
            IntTy::I128 => "i128",
            IntTy::Isize => "isize",
            IntTy::U8 => "u8",
            IntTy::U16 => "u16",
            IntTy::U32 => "u32",
            IntTy::U64 => "u64",
            IntTy::U128 => "u128",
            IntTy::Usize => "usize",
        }
    }

    pub fn from_name(s: &str) -> Option<IntTy> {
        Some(match s {
            "i8" => IntTy::I8,
            "i16" => IntTy::I16,
            "i32" => IntTy::I32,
            "i64" => IntTy::I64,
            "i128" => IntTy::I128,
            "isize" => IntTy::Isize,
            "u8" => IntTy::U8,
            "u16" => IntTy::U16,
            "u32" => IntTy::U32,
            "u64" => IntTy::U64,
            "u128" => IntTy::U128,
            "usize" => IntTy::Usize,
            _ => return None,
        })
    }

    pub fn signed(self) -> bool {
        matches!(self, IntTy::I8 | IntTy::I16 | IntTy::I32 | IntTy::I64 | IntTy::I128 | IntTy::Isize)
    }

    pub fn bits(self) -> u32 {
        match self {
            IntTy::I8 | IntTy::U8 => 8,
            IntTy::I16 | IntTy::U16 => 16,
            IntTy::I32 | IntTy::U32 => 32,
            IntTy::I64 | IntTy::U64 | IntTy::Isize | IntTy::Usize => 64,
            IntTy::I128 | IntTy::U128 => 128,
        }
    }

    pub fn max_value(self) -> u128 {
        if self.signed() {
            (1u128 << (self.bits() - 1)) - 1
        } else if self.bits() == 128 {
            u128::MAX
        } else {
            (1u128 << self.bits()) - 1
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FloatTy {
    F32,
    F64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ty {
    Int(IntTy),
    Float(FloatTy),
    Bool,
    Char,
    /// `str` (unsized; behind a reference it is a fat pointer).
    Str,
    Never,
    /// `()` is the empty tuple.
    Tuple(Rc<[Ty]>),
    Ref(bool, Rc<Ty>),
    Ptr(bool, Rc<Ty>),
    /// The length is `Const(n)`, a const parameter or a variable.
    Array(Rc<Ty>, Rc<Ty>),
    Slice(Rc<Ty>),
    Adt(DefId, Rc<[Ty]>),
    FnPtr(Rc<[Ty]>, Rc<Ty>),
    /// A function item (zero-sized); calling it is a direct call.
    FnDef(DefId, Rc<[Ty]>),
    /// A closure, by its index in the program's closure table.
    Closure(u32),
    /// `dyn Trait<args, Assoc = T>` (unsized).
    Dyn(Rc<DynTy>),
    Param(u32),
    /// A value of a const generic parameter (array lengths).
    Const(u64),
    Infer(u32),
    IntVar(u32),
    FloatVar(u32),
    /// `<Self as Trait<args>>::Name`, with Self first in the list.
    Proj(DefId, Rc<[Ty]>, Name),
    /// The hidden type of an `impl Trait` return: (function, its substs).
    Opaque(DefId, Rc<[Ty]>),
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DynTy {
    pub trait_: DefId,
    /// Trait arguments without Self.
    pub args: Rc<[Ty]>,
    pub bindings: Rc<[(Name, Ty)]>,
}

pub fn unit() -> Ty {
    Ty::Tuple(Rc::from([]))
}

impl Ty {
    pub fn is_unit(&self) -> bool {
        matches!(self, Ty::Tuple(t) if t.is_empty())
    }

    pub fn reference(m: bool, t: Ty) -> Ty {
        Ty::Ref(m, Rc::new(t))
    }

    pub fn array(elem: Ty, n: u64) -> Ty {
        Ty::Array(Rc::new(elem), Rc::new(Ty::Const(n)))
    }

    /// The length of an array type, when known.
    pub fn array_len(&self) -> Option<u64> {
        match self {
            Ty::Array(_, n) => match &**n {
                Ty::Const(n) => Some(*n),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn usize() -> Ty {
        Ty::Int(IntTy::Usize)
    }

    pub fn is_integral(&self) -> bool {
        matches!(self, Ty::Int(_) | Ty::IntVar(_))
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Ty::Float(_) | Ty::FloatVar(_))
    }

    pub fn is_numeric(&self) -> bool {
        self.is_integral() || self.is_float()
    }

    pub fn is_unsized(&self) -> bool {
        matches!(self, Ty::Str | Ty::Slice(_) | Ty::Dyn(_))
    }

    pub fn is_var(&self) -> bool {
        matches!(self, Ty::Infer(_) | Ty::IntVar(_) | Ty::FloatVar(_))
    }

    /// Whether the type has inference variables or projections left.
    pub fn needs_infer(&self) -> bool {
        let mut found = false;
        self.walk(&mut |t| {
            if matches!(t, Ty::Infer(_) | Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Proj(..) | Ty::Opaque(..)) {
                found = true;
            }
        });
        found
    }

    pub fn has_params(&self) -> bool {
        let mut found = false;
        self.walk(&mut |t| {
            if matches!(t, Ty::Param(_)) {
                found = true;
            }
        });
        found
    }

    /// Calls `f` on this type and every type inside it.
    pub fn walk(&self, f: &mut dyn FnMut(&Ty)) {
        f(self);
        match self {
            Ty::Tuple(ts) | Ty::Adt(_, ts) | Ty::FnDef(_, ts) | Ty::Proj(_, ts, _) | Ty::Opaque(_, ts) => {
                ts.iter().for_each(|t| t.walk(f))
            }
            Ty::Ref(_, t) | Ty::Ptr(_, t) | Ty::Slice(t) => t.walk(f),
            Ty::Array(t, n) => {
                t.walk(f);
                n.walk(f);
            }
            Ty::FnPtr(ps, r) => {
                ps.iter().for_each(|t| t.walk(f));
                r.walk(f);
            }
            Ty::Dyn(d) => {
                d.args.iter().for_each(|t| t.walk(f));
                d.bindings.iter().for_each(|(_, t)| t.walk(f));
            }
            _ => {}
        }
    }

    /// Rebuilds the type bottom-up, replacing nodes for which `f` returns
    /// `Some` (children of replaced nodes are not visited).
    pub fn map(&self, f: &mut dyn FnMut(&Ty) -> Option<Ty>) -> Ty {
        if let Some(t) = f(self) {
            return t;
        }
        let list = |ts: &Rc<[Ty]>, f: &mut dyn FnMut(&Ty) -> Option<Ty>| -> Rc<[Ty]> {
            ts.iter().map(|t| t.map(f)).collect::<Vec<_>>().into()
        };
        match self {
            Ty::Tuple(ts) => Ty::Tuple(list(ts, f)),
            Ty::Adt(d, ts) => Ty::Adt(*d, list(ts, f)),
            Ty::FnDef(d, ts) => Ty::FnDef(*d, list(ts, f)),
            Ty::Opaque(d, ts) => Ty::Opaque(*d, list(ts, f)),
            Ty::Proj(d, ts, n) => Ty::Proj(*d, list(ts, f), n.clone()),
            Ty::Ref(m, t) => Ty::Ref(*m, Rc::new(t.map(f))),
            Ty::Ptr(m, t) => Ty::Ptr(*m, Rc::new(t.map(f))),
            Ty::Array(t, n) => Ty::Array(Rc::new(t.map(f)), Rc::new(n.map(f))),
            Ty::Slice(t) => Ty::Slice(Rc::new(t.map(f))),
            Ty::FnPtr(ps, r) => Ty::FnPtr(list(ps, f), Rc::new(r.map(f))),
            Ty::Dyn(d) => Ty::Dyn(Rc::new(DynTy {
                trait_: d.trait_,
                args: list(&d.args, f),
                bindings: d.bindings.iter().map(|(n, t)| (n.clone(), t.map(f))).collect::<Vec<_>>().into(),
            })),
            t => t.clone(),
        }
    }

    /// Replaces `Param(i)` with `substs[i]`.
    pub fn subst(&self, substs: &[Ty]) -> Ty {
        if substs.is_empty() {
            return self.clone();
        }
        self.map(&mut |t| match t {
            Ty::Param(i) => Some(substs.get(*i as usize).cloned().unwrap_or(Ty::Error)),
            _ => None,
        })
    }
}

pub fn subst_list(ts: &[Ty], substs: &[Ty]) -> Rc<[Ty]> {
    ts.iter().map(|t| t.subst(substs)).collect::<Vec<_>>().into()
}

/// Names for diagnostics; items are named by a callback.
pub fn display(t: &Ty, name: &dyn Fn(DefId) -> String) -> String {
    let list = |ts: &[Ty]| -> String { ts.iter().map(|t| display(t, name)).collect::<Vec<_>>().join(", ") };
    match t {
        Ty::Int(i) => String::from(i.name()),
        Ty::Float(FloatTy::F32) => String::from("f32"),
        Ty::Float(FloatTy::F64) => String::from("f64"),
        Ty::Bool => String::from("bool"),
        Ty::Char => String::from("char"),
        Ty::Str => String::from("str"),
        Ty::Never => String::from("!"),
        Ty::Tuple(ts) if ts.len() == 1 => format!("({},)", display(&ts[0], name)),
        Ty::Tuple(ts) => format!("({})", list(ts)),
        Ty::Ref(m, t) => format!("&{}{}", if *m { "mut " } else { "" }, display(t, name)),
        Ty::Ptr(m, t) => format!("*{} {}", if *m { "mut" } else { "const" }, display(t, name)),
        Ty::Array(t, n) => format!("[{}; {}]", display(t, name), display(n, name)),
        Ty::Const(n) => format!("{}", n),
        Ty::Slice(t) => format!("[{}]", display(t, name)),
        Ty::Adt(d, ts) if ts.is_empty() => name(*d),
        Ty::Adt(d, ts) => format!("{}<{}>", name(*d), list(ts)),
        Ty::FnPtr(ps, r) if r.is_unit() => format!("fn({})", list(ps)),
        Ty::FnPtr(ps, r) => format!("fn({}) -> {}", list(ps), display(r, name)),
        Ty::FnDef(d, _) => format!("fn item {{{}}}", name(*d)),
        Ty::Closure(_) => String::from("{closure}"),
        Ty::Dyn(d) => {
            let mut s = format!("dyn {}", name(d.trait_));
            let mut parts: Vec<String> = d.args.iter().map(|t| display(t, name)).collect();
            parts.extend(d.bindings.iter().map(|(n, t)| format!("{} = {}", n, display(t, name))));
            if !parts.is_empty() {
                s.push('<');
                s.push_str(&parts.join(", "));
                s.push('>');
            }
            s
        }
        Ty::Param(i) => format!("T{}", i),
        Ty::Infer(_) => String::from("_"),
        Ty::IntVar(_) => String::from("{integer}"),
        Ty::FloatVar(_) => String::from("{float}"),
        Ty::Proj(d, ts, n) => format!("<{} as {}>::{}", display(&ts[0], name), name(*d), n),
        Ty::Opaque(..) => String::from("impl Trait"),
        Ty::Error => String::from("{error}"),
    }
}
