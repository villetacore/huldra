//! The typed IR: what the checker produces for each monomorphized
//! function. Every type is concrete; method calls, operators, `for`, `?`
//! and formatting are already lowered to plain calls and matches.

use crate::ast::{BinOp, Name};
use crate::resolve::DefId;
use crate::ty::Ty;
use crate::Span;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

pub type LocalId = u32;
pub type InstId = u32;

#[derive(Clone, Debug)]
pub struct Local {
    pub name: Name,
    pub ty: Ty,
    /// 0 for the function itself, `k + 1` for closure `k`.
    pub owner: u32,
}

#[derive(Clone, Debug)]
pub enum Callee {
    /// Before monomorphization: a function, method or constructor.
    Item(DefId, Rc<[Ty]>),
    /// Before monomorphization: a trait method (substs: Self, trait
    /// arguments, method arguments).
    Trait(DefId, Rc<[Ty]>),
    /// A monomorphized function.
    Inst(InstId),
    /// A builtin operation (compiler intrinsics, builtin trait impls).
    Builtin(Builtin, Rc<[Ty]>),
    /// `extern "C"` function.
    Extern(DefId),
    /// Tuple struct / variant constructor.
    Ctor(DefId, Rc<[Ty]>),
    /// A trait object method: (trait, slot in the vtable); the first
    /// argument is the fat pointer.
    Virtual(DefId, u32),
    /// A function pointer value.
    Value(Box<Expr>),
    /// A closure: the place holding it, and its id.
    Closure(Box<Expr>, u32),
    /// `dyn Fn*` (the first argument is the fat pointer).
    DynFn,
}

#[derive(Clone, Debug)]
pub enum Builtin {
    /// `extern "rust-intrinsic"` by name.
    Intrinsic(Name),
    /// `Clone::clone` of a builtin type: a copy when empty, otherwise the
    /// clone of each element (one for arrays, one per field for tuples).
    Clone(Vec<Callee>),
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Ty,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(u128),
    Float(f64),
    Bool(bool),
    Char(char),
    Str(Rc<str>),
    ByteStr(Rc<[u8]>),
    Unit,
    Local(LocalId),
    /// A captured variable inside a closure body.
    Upvar(u32),
    Static(DefId),
    /// A function item used as a value (zero-sized).
    FnItem,
    /// Turns a function item or capture-less closure into a pointer.
    Reify(Callee),
    Call(Callee, Vec<Expr>),
    Unary(crate::ast::UnOp, Box<Expr>),
    /// Primitive arithmetic, bit operations and comparisons.
    Binary(BinOp, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cast(Box<Expr>),
    AddrOf(bool, Box<Expr>),
    Deref(Box<Expr>),
    Field(Box<Expr>, usize),
    /// Bounds-checked indexing of an array or slice place.
    Index(Box<Expr>, Box<Expr>),
    /// `&x[a..b]` on arrays, slices and `str` (the result is a fat pointer).
    Slice(Box<Expr>, Option<Box<Expr>>, Option<Box<Expr>>),
    Adt(DefId, Vec<Expr>),
    Tuple(Vec<Expr>),
    Array(Vec<Expr>),
    Repeat(Box<Expr>, u64),
    Closure(u32, Vec<Expr>),
    /// Unsizing coercion to `self.ty` (slices and trait objects).
    Unsize(Box<Expr>),
    Block(Vec<Stmt>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Match(Box<Expr>, Vec<Arm>),
    Loop(Box<Expr>, u32),
    LabeledBlock(Box<Expr>, u32),
    Break(u32, Box<Expr>),
    Continue(u32),
    Return(Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    /// Primitive `a op= b`.
    AssignOp(BinOp, Box<Expr>, Box<Expr>),
    /// `&'static Location` of a span (or the caller's in `#[track_caller]`).
    Location(Span),
    /// Where control cannot reach (non-exhaustive match fallthrough).
    Unreachable,
    /// A `let` that has not been initialized (`let x;`).
    Uninit,
}

#[derive(Clone, Debug)]
pub struct Arm {
    pub pat: Pat,
    pub guard: Option<Expr>,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let(Pat, Option<Expr>, Option<Expr>),
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub struct Pat {
    pub kind: PatKind,
    pub ty: Ty,
}

#[derive(Clone, Debug)]
pub enum PatKind {
    Wild,
    /// A binding; `by_ref` is `Some(mutable)` for `ref`/default-ref bindings.
    Bind(LocalId, Option<bool>, Option<Box<Pat>>),
    /// A constant (integer, char, bool, float or string).
    Const(Box<Expr>),
    Range(Option<Box<Expr>>, Option<Box<Expr>>, bool),
    /// Struct, variant or tuple fields by index. `def` is the struct or
    /// variant (`None` for tuples).
    Adt(Option<DefId>, Vec<(usize, Pat)>),
    Deref(Box<Pat>),
    Or(Vec<Pat>),
    /// Prefix, optional rest (bound or wildcard), suffix.
    Slice(Vec<Pat>, Option<Box<Pat>>, Vec<Pat>),
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<LocalId>,
    pub locals: Vec<Local>,
    pub body: Expr,
    pub ret: Ty,
    pub track_caller: bool,
}

#[derive(Clone, Debug)]
pub struct Closure {
    pub params: Vec<LocalId>,
    pub ret: Ty,
    /// Captured values: their types (a reference type for by-ref captures)
    /// and whether they are references.
    pub captures: Vec<(Ty, bool)>,
    pub body: Expr,
    /// The function whose locals the closure body uses.
    pub func: InstId,
}

#[derive(Clone, Debug)]
pub struct Static {
    pub def: DefId,
    pub ty: Ty,
    pub init: Expr,
    pub locals: Vec<Local>,
}

#[derive(Clone, Debug)]
pub struct Vtable {
    pub trait_: DefId,
    pub ty: Ty,
    /// Methods in slot order (`None` for methods that cannot be called
    /// through the object).
    pub methods: Vec<Option<InstId>>,
    /// For `dyn Fn` from a function item: the function.
    pub fn_inst: Option<InstId>,
}

/// The layout of a struct or enum: field types per variant (with generic
/// parameters) and the discriminant of each variant.
#[derive(Clone, Debug)]
pub struct Layout {
    pub variants: Vec<Vec<Ty>>,
    pub discs: Vec<i128>,
}

#[derive(Debug)]
pub struct Program {
    pub fns: Vec<Function>,
    pub closures: Vec<Closure>,
    pub statics: Vec<Static>,
    pub vtables: Vec<Vtable>,
    pub main: InstId,
    /// `main` returns `Result<(), E>`: the instance that reports an error.
    pub main_result: Option<InstId>,
    pub layouts: alloc::collections::BTreeMap<DefId, Layout>,
    /// Signatures of `extern "C"` functions.
    pub externs: alloc::collections::BTreeMap<DefId, (Vec<Ty>, Ty)>,
    /// Field types of ADT instances whose declared fields have associated
    /// types (`Option<U::IntoIter>`), normalized.
    pub adt_fields: alloc::collections::BTreeMap<Ty, Vec<Vec<Ty>>>,
}
