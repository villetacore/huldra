//! The syntax tree produced by the parser. Names are not resolved yet;
//! macros (`println!`, `vec!`, ...) are already parsed into their own nodes.

use crate::Span;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

pub type Name = Rc<str>;

#[derive(Clone, Debug)]
pub struct Path {
    pub segs: Vec<PathSeg>,
    /// `::a::b` (unused by Rust 2018+ code, but accepted).
    pub global: bool,
    /// `<T as Trait>::rest`: the qualified self type and trait.
    pub qself: Option<Box<QSelf>>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct QSelf {
    pub ty: Type,
    pub trait_: Option<Path>,
}

#[derive(Clone, Debug)]
pub struct PathSeg {
    pub name: Name,
    pub args: Option<GenericArgs>,
}

#[derive(Clone, Debug, Default)]
pub struct GenericArgs {
    pub types: Vec<Type>,
    /// `Iterator<Item = T>`
    pub bindings: Vec<(Name, Type)>,
    /// `Fn(A, B) -> C`: the arguments become a tuple in `types`, the output
    /// a binding named `Output`.
    pub paren: bool,
}

impl Path {
    pub fn single(name: Name, span: Span) -> Path {
        Path {
            segs: alloc::vec![PathSeg { name, args: None }],
            global: false,
            qself: None,
            span,
        }
    }

    /// The name if this is a plain identifier.
    pub fn as_ident(&self) -> Option<&Name> {
        if self.segs.len() == 1 && self.segs[0].args.is_none() && self.qself.is_none() && !self.global {
            Some(&self.segs[0].name)
        } else {
            None
        }
    }

    pub fn last(&self) -> &Name {
        &self.segs.last().unwrap().name
    }
}

#[derive(Clone, Debug)]
pub struct Type {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TypeKind {
    Path(Path),
    Ref(bool, Box<Type>),
    Ptr(bool, Box<Type>),
    Tuple(Vec<Type>),
    Array(Box<Type>, Box<Expr>),
    Slice(Box<Type>),
    /// `fn(A, B) -> C`
    Fn(Vec<Type>, Box<Type>),
    Never,
    Infer,
    ImplTrait(Vec<Bound>),
    Dyn(Vec<Bound>),
}

/// A trait bound; lifetimes are dropped by the parser.
pub type Bound = Path;

#[derive(Clone, Debug, Default)]
pub struct Generics {
    pub params: Vec<GenericParam>,
    pub where_: Vec<(Type, Vec<Bound>)>,
}

#[derive(Clone, Debug)]
pub struct GenericParam {
    pub name: Name,
    pub bounds: Vec<Bound>,
    pub default: Option<Type>,
    /// `const N: usize`
    pub const_ty: Option<Type>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Attrs {
    pub derives: Vec<Name>,
    /// Other attributes, by name (`test`, `track_caller`, `inline`, ...).
    pub names: Vec<Name>,
}

impl Attrs {
    pub fn has(&self, n: &str) -> bool {
        self.names.iter().any(|a| &**a == n)
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub kind: ItemKind,
    pub attrs: Attrs,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ItemKind {
    Fn(Rc<FnDecl>),
    Struct(Rc<StructDef>),
    Enum(Rc<EnumDef>),
    Impl(Rc<ImplDef>),
    Trait(Rc<TraitDef>),
    Const(Name, Type, Rc<Expr>),
    Static(Name, bool, Type, Rc<Expr>),
    /// `mod name { items }`; `mod name;` is loaded by the parser's caller.
    Mod(Name, Vec<Item>),
    /// An unloaded `mod name;`.
    ModFile(Name),
    Use(UseTree),
    TypeAlias(Name, Generics, Type),
    /// `extern "C" { fn ...; }`; the ABI name is kept.
    Extern(Name, Vec<Rc<FnDecl>>),
    /// `macro_rules! name { ... }`
    MacroRules(Rc<MacroDef>),
}

#[derive(Clone, Debug)]
pub enum UseTree {
    /// `a::b` or `a::b as c`
    Path(Vec<Name>, Option<Name>, Span),
    /// `a::b::*`
    Glob(Vec<Name>, Span),
    /// `a::{x, y::z}`
    Group(Vec<Name>, Vec<UseTree>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfKind {
    Value,
    Ref,
    RefMut,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub pat: Pat,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    pub name: Name,
    pub generics: Generics,
    /// `self`, `&self`, `&mut self`, `mut self`; `self: Box<Self>` is `Value`
    /// with `self_ty` set.
    pub self_param: Option<(SelfKind, bool)>,
    pub self_ty: Option<Type>,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    pub body: Option<Rc<Block>>,
    pub variadic: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum VariantData {
    Named(Vec<(Name, Type)>),
    Tuple(Vec<Type>),
    Unit,
}

#[derive(Clone, Debug)]
pub struct StructDef {
    pub name: Name,
    pub generics: Generics,
    pub data: VariantData,
    /// Which fields are `pub` (in declaration order).
    pub public: Vec<bool>,
}

#[derive(Clone, Debug)]
pub struct Variant {
    pub name: Name,
    pub data: VariantData,
    pub disc: Option<Expr>,
    /// `#[default]` (for `derive(Default)`).
    pub default: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EnumDef {
    pub name: Name,
    pub generics: Generics,
    pub variants: Vec<Variant>,
}

#[derive(Clone, Debug)]
pub enum ImplItem {
    Fn(Rc<FnDecl>, Attrs),
    Const(Name, Type, Rc<Expr>),
    Type(Name, Type),
}

#[derive(Clone, Debug)]
pub struct ImplDef {
    pub generics: Generics,
    pub trait_: Option<Path>,
    /// `impl !Trait`: unsupported but parsed.
    pub negative: bool,
    pub self_ty: Type,
    pub items: Vec<ImplItem>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TraitItem {
    Fn(Rc<FnDecl>, Attrs),
    Type(Name, Vec<Bound>, Option<Type>),
    Const(Name, Type, Option<Rc<Expr>>),
}

#[derive(Clone, Debug)]
pub struct TraitDef {
    pub name: Name,
    pub generics: Generics,
    pub supertraits: Vec<Bound>,
    pub items: Vec<TraitItem>,
}

#[derive(Clone, Debug)]
pub struct MacroDef {
    pub name: Name,
    pub rules: Vec<(Vec<crate::lex::Token>, Vec<crate::lex::Token>)>,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub expr: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let {
        pat: Pat,
        ty: Option<Type>,
        init: Option<Expr>,
        else_: Option<Rc<Block>>,
        span: Span,
    },
    /// An expression statement; `true` when followed by `;`.
    Expr(Expr, bool),
    Item(Item),
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn is_comparison(self) -> bool {
        matches!(self, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
    }

    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    Deref,
}

#[derive(Clone, Debug)]
pub enum Lit {
    Int(u128, Option<Name>),
    Float(Name, Option<Name>),
    Str(Name),
    ByteStr(Rc<[u8]>),
    Char(char),
    Byte(u8),
    Bool(bool),
}

/// A piece of a format string.
#[derive(Clone, Debug)]
pub enum FmtPiece {
    Lit(String),
    Arg(FmtArg),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FmtTrait {
    Display,
    Debug,
    LowerHex,
    UpperHex,
    Octal,
    Binary,
    LowerExp,
    UpperExp,
}

#[derive(Clone, Debug)]
pub enum FmtCount {
    Is(usize),
    /// Index into the macro's argument list.
    Arg(usize),
}

#[derive(Clone, Debug)]
pub struct FmtArg {
    /// Index into the macro's argument list.
    pub arg: usize,
    pub fill: char,
    /// 0 unknown, 1 left, 2 center, 3 right
    pub align: u8,
    pub plus: bool,
    pub alternate: bool,
    pub zero: bool,
    pub width: Option<FmtCount>,
    pub precision: Option<FmtCount>,
    pub trait_: FmtTrait,
}

#[derive(Clone, Debug)]
pub struct Format {
    pub pieces: Vec<FmtPiece>,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacroKind {
    Print,
    Println,
    Eprint,
    Eprintln,
    Format,
    /// `format_args!`, the argument of `write!`.
    Args,
    Panic,
}

#[derive(Clone, Debug)]
pub struct Arm {
    pub pat: Pat,
    pub guard: Option<Expr>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ClosureParam {
    pub pat: Pat,
    pub ty: Option<Type>,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    /// Where the expression starts (postfix expressions carry the span of
    /// their operator or method name).
    pub fn start(&self) -> Span {
        match &self.kind {
            ExprKind::Field(b, _) | ExprKind::TupleField(b, _) | ExprKind::Index(b, _) | ExprKind::Try(b) | ExprKind::Cast(b, _) => b.start(),
            ExprKind::MethodCall { recv, .. } => recv.start(),
            ExprKind::Call(f, _) => f.start(),
            ExprKind::Binary(_, a, _) | ExprKind::Assign(a, _) | ExprKind::AssignOp(_, a, _) => a.start(),
            ExprKind::Range(Some(a), _, _) => a.start(),
            _ => self.span,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Lit(Lit),
    Path(Path),
    Unary(UnOp, Box<Expr>),
    /// `&e` / `&mut e`
    Ref(bool, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    AssignOp(BinOp, Box<Expr>, Box<Expr>),
    Cast(Box<Expr>, Type),
    Call(Box<Expr>, Vec<Expr>),
    MethodCall {
        recv: Box<Expr>,
        name: Name,
        args: Option<GenericArgs>,
        params: Vec<Expr>,
    },
    Field(Box<Expr>, Name),
    /// `t.0`
    TupleField(Box<Expr>, usize),
    Index(Box<Expr>, Box<Expr>),
    Range(Option<Box<Expr>>, Option<Box<Expr>>, bool),
    Tuple(Vec<Expr>),
    Array(Vec<Expr>),
    ArrayRepeat(Box<Expr>, Box<Expr>),
    StructLit {
        path: Path,
        fields: Vec<(Name, Expr, Span)>,
        base: Option<Box<Expr>>,
    },
    Block(Rc<Block>, Option<Name>),
    Unsafe(Rc<Block>),
    If(Box<Expr>, Rc<Block>, Option<Box<Expr>>),
    /// `let pat = e` inside an `if`/`while` condition (possibly in a `&&` chain).
    Let(Box<Pat>, Box<Expr>),
    Match(Box<Expr>, Vec<Arm>),
    While(Box<Expr>, Rc<Block>, Option<Name>),
    Loop(Rc<Block>, Option<Name>),
    For(Box<Pat>, Box<Expr>, Rc<Block>, Option<Name>),
    Break(Option<Name>, Option<Box<Expr>>),
    Continue(Option<Name>),
    Return(Option<Box<Expr>>),
    Try(Box<Expr>),
    Closure {
        params: Vec<ClosureParam>,
        ret: Option<Type>,
        body: Rc<Expr>,
        is_move: bool,
    },
    /// `print!`, `format!`, `panic!` and friends.
    Format(MacroKind, Rc<Format>),
    /// `write!(dst, ...)` / `writeln!` with the formatted arguments.
    Write(Box<Expr>, Rc<Format>),
    /// `vec![a, b]`
    VecLit(Vec<Expr>),
    /// `vec![x; n]`
    VecRepeat(Box<Expr>, Box<Expr>),
    /// `assert!(cond, fmt...)`
    Assert(Box<Expr>, Option<Rc<Format>>),
    /// `assert_eq!` (true) / `assert_ne!` (false)
    AssertCmp(bool, Box<Expr>, Box<Expr>, Option<Rc<Format>>),
    /// `matches!(e, pat)`
    Matches(Box<Expr>, Box<Pat>, Option<Box<Expr>>),
    /// `dbg!(e)`
    Dbg(Box<Expr>, String),
    Paren(Box<Expr>),
    /// An already type-checked expression, by index (used by the checker
    /// when it desugars syntax late).
    Checked(u32),
    /// `&'static core::panic::Location` of this span (the caller's inside
    /// `#[track_caller]` functions).
    Location,
}

#[derive(Clone, Debug)]
pub struct Pat {
    pub kind: PatKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum PatKind {
    Wild,
    /// A binding (`ref`, `mut`, `x @ pat`), or a unit struct/variant/const
    /// when the name resolves to one.
    Ident {
        name: Name,
        by_ref: bool,
        mutable: bool,
        sub: Option<Box<Pat>>,
    },
    Lit(Box<Expr>),
    Range(Option<Box<Expr>>, Option<Box<Expr>>, bool),
    Tuple(Vec<Pat>),
    TupleStruct(Path, Vec<Pat>),
    Struct(Path, Vec<(Name, Pat)>, bool),
    Path(Path),
    Ref(Box<Pat>, bool),
    Or(Vec<Pat>),
    Slice(Vec<Pat>),
    /// `..` inside tuple, tuple-struct and slice patterns.
    Rest,
    /// `name @ ..` inside a slice pattern is `Ident` with this as `sub`.
    Box(Box<Pat>),
}
