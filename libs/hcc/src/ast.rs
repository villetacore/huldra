//! The typed syntax tree produced by the parser.

use crate::ty::{FuncType, Type};
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    /// Index into the current function's locals.
    Local(usize),
    /// A function or global variable by its link name.
    Global(Rc<str>),
    /// Arithmetic on operands already converted to a common type; for
    /// comparisons the operand type is the left operand's.
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    BitNot(Box<Expr>),
    LogAnd(Box<Expr>, Box<Expr>),
    LogOr(Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Comma(Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    Deref(Box<Expr>),
    Addr(Box<Expr>),
    /// Member at a byte offset.
    Member(Box<Expr>, usize),
    Cast(Box<Expr>),
    /// `ret` is the local receiving a returned struct.
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
        ret: Option<usize>,
    },
    /// `__syscall(nr, args...)`
    Syscall(Vec<Expr>),
    /// `__builtin_sqrt(x)`
    Sqrt(Box<Expr>),
    /// Zero-fills a local (before its initializer runs).
    Zero(usize),
    /// GNU statement expression: the value of the last expression statement.
    Stmts(Vec<Stmt>, Option<Box<Expr>>),
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Expr(Expr),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    /// `for`, `while` and `do`-`while`.
    Loop {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        step: Option<Expr>,
        body: Box<Stmt>,
        brk: usize,
        cont: usize,
        post_test: bool,
    },
    Switch {
        cond: Expr,
        body: Box<Stmt>,
        cases: Vec<(i64, i64, usize)>,
        default: Option<usize>,
        brk: usize,
    },
    Label(usize),
    Goto(usize),
    Return(Option<Expr>),
}

#[derive(Clone, Debug)]
pub struct Local {
    pub ty: Type,
}

#[derive(Debug)]
pub struct Function {
    pub name: Rc<str>,
    pub ty: Rc<FuncType>,
    pub locals: Vec<Local>,
    /// Locals holding the parameters, in order.
    pub params: Vec<usize>,
    pub body: Vec<Stmt>,
}

/// A pointer stored in initialized data.
#[derive(Clone, Debug)]
pub struct Reloc {
    pub offset: usize,
    pub target: Rc<str>,
    pub addend: i64,
}

#[derive(Debug)]
pub struct Global {
    pub name: Rc<str>,
    pub ty: Type,
    /// Initial contents; `None` means zero-filled (bss).
    pub data: Option<Vec<u8>>,
    pub relocs: Vec<Reloc>,
}

#[derive(Default, Debug)]
pub struct Program {
    pub functions: Vec<Function>,
    pub globals: Vec<Global>,
    /// Number of code labels allocated by the parser.
    pub labels: usize,
    /// Counter for unique names of string literals and static locals.
    pub anon: usize,
}
