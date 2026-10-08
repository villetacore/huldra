//! Recursive-descent parser for C: declarations, statements and
//! expressions, with type checking and implicit conversions applied as
//! the tree is built.

use crate::ast::*;
use crate::lex::{Tok, Token};
use crate::pp::parse_int_literal;
use crate::ty::{align_to, common, FuncType, Member, Record, Type, CHAR, INT, LONG, UINT, ULONG};
use crate::Error;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

#[derive(Clone)]
enum Sym {
    Local(usize, Type),
    Global(Rc<str>, Type),
    Typedef(Type),
    Enum(i64),
}

#[derive(Default)]
struct Scope {
    syms: BTreeMap<Rc<str>, Sym>,
    tags: BTreeMap<Rc<str>, Type>,
}

#[derive(Default, Clone, Copy)]
struct Storage {
    typedef: bool,
    is_static: bool,
    is_extern: bool,
}

struct FnState {
    locals: Vec<Local>,
    ret: Type,
    labels: BTreeMap<Rc<str>, usize>,
    defined_labels: Vec<Rc<str>>,
}

struct SwitchState {
    cases: Vec<(i64, i64, usize)>,
    default: Option<usize>,
}

/// Leaf of an initializer: an expression stored at a byte offset.
struct InitLeaf {
    offset: usize,
    ty: Type,
    expr: Expr,
}

struct Parser<'p> {
    toks: Vec<Token>,
    pos: usize,
    files: Vec<String>,
    unit: usize,
    prog: &'p mut Program,
    scopes: Vec<Scope>,
    func: Option<FnState>,
    brk: Vec<usize>,
    cont: Vec<usize>,
    switches: Vec<SwitchState>,
    /// Parameters of the most recently parsed function declarator.
    last_params: Vec<(Option<Rc<str>>, Type)>,
    /// Global definitions seen in this unit, by link name.
    defined: BTreeMap<Rc<str>, usize>,
}

pub fn parse(
    toks: Vec<Token>,
    files: Vec<String>,
    unit: usize,
    prog: &mut Program,
) -> Result<(), Error> {
    let mut p = Parser {
        toks,
        pos: 0,
        files,
        unit,
        prog,
        scopes: vec![Scope::default()],
        func: None,
        brk: Vec::new(),
        cont: Vec::new(),
        switches: Vec::new(),
        last_params: Vec::new(),
        defined: BTreeMap::new(),
    };
    for g in p.prog.globals.iter().enumerate() {
        p.defined.insert(g.1.name.clone(), g.0);
    }
    while !p.at_eof() {
        p.top_level()?;
    }
    Ok(())
}

fn expr(kind: ExprKind, ty: Type) -> Expr {
    Expr { kind, ty }
}

fn int(v: i64) -> Expr {
    expr(ExprKind::Int(v), INT)
}

fn long(v: i64) -> Expr {
    expr(ExprKind::Int(v), LONG)
}

fn b(e: Expr) -> Box<Expr> {
    Box::new(e)
}

/// Converts `e` to `ty`.
pub fn cast(e: Expr, ty: &Type) -> Expr {
    if e.ty.same(ty) && !ty.is_record() {
        return e;
    }
    expr(ExprKind::Cast(b(e)), ty.clone())
}

fn is_lvalue(e: &Expr) -> bool {
    matches!(
        e.kind,
        ExprKind::Local(_) | ExprKind::Global(_) | ExprKind::Deref(_) | ExprKind::Member(..)
    )
}

/// An expression is free of side effects and cheap to evaluate twice.
fn is_simple(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Local(_) | ExprKind::Global(_) | ExprKind::Int(_) => true,
        ExprKind::Member(x, _) => is_simple(x),
        ExprKind::Deref(x) => matches!(x.kind, ExprKind::Local(_) | ExprKind::Global(_)),
        _ => false,
    }
}

impl<'p> Parser<'p> {
    // ---- token helpers --------------------------------------------------

    fn peek(&self) -> &Token {
        &self.toks[self.pos.min(self.toks.len() - 1)]
    }

    fn peek_at(&self, n: usize) -> &Token {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)]
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().tok, Tok::Eof)
    }

    fn next(&mut self) -> Token {
        let t = self.peek().clone();
        if !self.at_eof() {
            self.pos += 1;
        }
        t
    }

    fn is(&self, p: &str) -> bool {
        let t = self.peek();
        t.is(p) || t.ident() == Some(p)
    }

    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn err_at(&self, t: &Token, msg: impl Into<String>) -> Error {
        Error {
            file: self.files.get(t.file as usize).cloned().unwrap_or_default(),
            line: t.line,
            message: msg.into(),
        }
    }

    fn err(&self, msg: impl Into<String>) -> Error {
        self.err_at(self.peek(), msg)
    }

    fn expect(&mut self, p: &str) -> Result<(), Error> {
        if self.eat(p) {
            Ok(())
        } else {
            let got = self.peek().spelling();
            Err(self.err(format!(
                "expected '{}' before '{}'",
                p,
                if got.is_empty() { "end of input" } else { &got }
            )))
        }
    }

    fn ident(&mut self) -> Result<Rc<str>, Error> {
        match &self.peek().tok {
            Tok::Ident(s) => {
                let s = s.clone();
                self.pos += 1;
                Ok(s)
            }
            _ => Err(self.err(format!(
                "expected identifier before '{}'",
                self.peek().spelling()
            ))),
        }
    }

    fn new_label(&mut self) -> usize {
        self.prog.labels += 1;
        self.prog.labels - 1
    }

    // ---- scopes ------------------------------------------------------------

    fn lookup(&self, name: &str) -> Option<&Sym> {
        self.scopes.iter().rev().find_map(|s| s.syms.get(name))
    }

    fn lookup_tag(&self, name: &str) -> Option<&Type> {
        self.scopes.iter().rev().find_map(|s| s.tags.get(name))
    }

    fn declare(&mut self, name: Rc<str>, sym: Sym) {
        self.scopes.last_mut().unwrap().syms.insert(name, sym);
    }

    fn new_local(&mut self, ty: Type) -> usize {
        let f = self.func.as_mut().expect("local outside a function");
        f.locals.push(Local { ty });
        f.locals.len() - 1
    }

    fn local_expr(&self, idx: usize) -> Expr {
        let ty = self.func.as_ref().unwrap().locals[idx].ty.clone();
        expr(ExprKind::Local(idx), ty)
    }

    fn anon_name(&mut self, what: &str) -> Rc<str> {
        self.prog.anon += 1;
        Rc::from(format!(".{}{}", what, self.prog.anon))
    }

    /// Adds a global variable or updates a previous declaration of it.
    fn add_global(
        &mut self,
        name: Rc<str>,
        ty: Type,
        data: Option<Vec<u8>>,
        relocs: Vec<Reloc>,
    ) -> Result<(), Error> {
        if let Some(&i) = self.defined.get(&name) {
            let g = &mut self.prog.globals[i];
            if data.is_some() {
                if g.data.is_some() && !g.name.starts_with('.') {
                    return Err(self.err(format!("redefinition of '{}'", name)));
                }
                g.data = data;
                g.relocs = relocs;
            }
            if g.ty.size() < ty.size() {
                g.ty = ty;
            }
            return Ok(());
        }
        self.defined.insert(name.clone(), self.prog.globals.len());
        self.prog.globals.push(Global {
            name,
            ty,
            data,
            relocs,
        });
        Ok(())
    }

    /// The link name of a file-scope symbol.
    fn link_name(&self, name: &Rc<str>, is_static: bool) -> Rc<str> {
        if is_static {
            Rc::from(format!("{}.{}", name, self.unit))
        } else {
            name.clone()
        }
    }

    // ---- declarations ------------------------------------------------------

    fn skip_attribute(&mut self) -> Result<(), Error> {
        while self.is("__attribute__")
            || self.is("__attribute")
            || self.is("__asm__")
            || self.is("__asm")
            || self.is("asm")
        {
            self.next();
            self.expect("(")?;
            let mut depth = 1;
            while depth > 0 {
                if self.at_eof() {
                    return Err(self.err("unterminated attribute"));
                }
                let t = self.next();
                if t.is("(") {
                    depth += 1;
                } else if t.is(")") {
                    depth -= 1;
                }
            }
        }
        Ok(())
    }

    fn is_typename(&self) -> bool {
        let Some(name) = self.peek().ident() else {
            return false;
        };
        matches!(
            name,
            "void"
                | "_Bool"
                | "char"
                | "short"
                | "int"
                | "long"
                | "float"
                | "double"
                | "signed"
                | "unsigned"
                | "struct"
                | "union"
                | "enum"
                | "typedef"
                | "static"
                | "extern"
                | "inline"
                | "const"
                | "volatile"
                | "restrict"
                | "auto"
                | "register"
                | "_Noreturn"
                | "__restrict"
                | "__restrict__"
                | "__inline"
                | "__inline__"
                | "__const"
                | "__volatile__"
                | "__signed__"
                | "_Thread_local"
                | "__thread"
                | "__extension__"
                | "__attribute__"
                | "_Alignas"
                | "typeof"
                | "__typeof__"
        ) || matches!(self.lookup(name), Some(Sym::Typedef(_)))
    }

    fn declspec(&mut self, storage: Option<&mut Storage>) -> Result<Type, Error> {
        const VOID: u32 = 1 << 0;
        const BOOL: u32 = 1 << 2;
        const CHAR_: u32 = 1 << 4;
        const SHORT: u32 = 1 << 6;
        const INT_: u32 = 1 << 8;
        const LONG_: u32 = 1 << 10;
        const FLOAT: u32 = 1 << 12;
        const DOUBLE: u32 = 1 << 14;
        const OTHER: u32 = 1 << 16;
        const SIGNED: u32 = 1 << 17;
        const UNSIGNED: u32 = 1 << 18;
        let mut st = Storage::default();
        let mut counter = 0u32;
        let mut ty = INT;
        loop {
            let t = self.peek().clone();
            let Some(name) = t.ident() else { break };
            match name {
                "typedef" | "static" | "extern" => {
                    if storage.is_none() {
                        return Err(self.err_at(&t, "storage class specifier is not allowed here"));
                    }
                    match name {
                        "typedef" => st.typedef = true,
                        "static" => st.is_static = true,
                        _ => st.is_extern = true,
                    }
                    self.next();
                    continue;
                }
                "inline" | "const" | "volatile" | "restrict" | "auto" | "register"
                | "_Noreturn" | "__restrict" | "__restrict__" | "__inline" | "__inline__"
                | "__const" | "__volatile__" | "_Thread_local" | "__thread" | "__extension__" => {
                    self.next();
                    continue;
                }
                "__attribute__" => {
                    self.skip_attribute()?;
                    continue;
                }
                "_Alignas" => {
                    self.next();
                    self.expect("(")?;
                    let mut depth = 1;
                    while depth > 0 {
                        let t = self.next();
                        if t.is("(") {
                            depth += 1;
                        } else if t.is(")") {
                            depth -= 1;
                        }
                    }
                    continue;
                }
                _ => {}
            }
            let typedef = match self.lookup(name) {
                Some(Sym::Typedef(t)) => Some(t.clone()),
                _ => None,
            };
            if let Some(tt) = typedef {
                if counter != 0 {
                    break;
                }
                self.next();
                ty = tt;
                counter |= OTHER;
                continue;
            }
            match name {
                "struct" | "union" => {
                    if counter != 0 {
                        return Err(
                            self.err_at(&t, "two or more data types in declaration specifiers")
                        );
                    }
                    self.next();
                    ty = self.record_decl(name == "union")?;
                    counter |= OTHER;
                    continue;
                }
                "enum" => {
                    if counter != 0 {
                        return Err(
                            self.err_at(&t, "two or more data types in declaration specifiers")
                        );
                    }
                    self.next();
                    ty = self.enum_decl()?;
                    counter |= OTHER;
                    continue;
                }
                "typeof" | "__typeof__" => {
                    self.next();
                    self.expect("(")?;
                    ty = if self.is_typename() {
                        self.typename()?
                    } else {
                        self.expr()?.ty
                    };
                    self.expect(")")?;
                    counter |= OTHER;
                    continue;
                }
                _ => {}
            }
            counter += match name {
                "void" => VOID,
                "_Bool" => BOOL,
                "char" => CHAR_,
                "short" => SHORT,
                "int" => INT_,
                "long" => LONG_,
                "float" => FLOAT,
                "double" => DOUBLE,
                "signed" | "__signed__" => {
                    counter |= SIGNED;
                    0
                }
                "unsigned" => {
                    counter |= UNSIGNED;
                    0
                }
                _ => break,
            };
            self.next();
            let base = counter & !(SIGNED | UNSIGNED);
            let unsigned = counter & UNSIGNED != 0;
            ty = match base {
                VOID => Type::Void,
                BOOL => Type::Bool,
                0 => Type::Int {
                    size: 4,
                    signed: !unsigned,
                },
                CHAR_ => Type::Int {
                    size: 1,
                    signed: !unsigned,
                },
                SHORT | 0x140 => Type::Int {
                    size: 2,
                    signed: !unsigned,
                },
                INT_ => Type::Int {
                    size: 4,
                    signed: !unsigned,
                },
                LONG_ | 0x500 | 0x800 | 0x900 => Type::Int {
                    size: 8,
                    signed: !unsigned,
                },
                FLOAT => Type::Float,
                DOUBLE | 0x4400 => Type::Double,
                _ => return Err(self.err_at(&t, "invalid combination of type specifiers")),
            };
        }
        if counter & !(SIGNED | UNSIGNED) == 0 && counter != 0 {
            ty = Type::Int {
                size: 4,
                signed: counter & UNSIGNED == 0,
            };
        }
        if let Some(s) = storage {
            *s = st;
        }
        Ok(ty)
    }

    fn record_decl(&mut self, union: bool) -> Result<Type, Error> {
        self.skip_attribute()?;
        let tag = if let Tok::Ident(_) = self.peek().tok {
            Some(self.ident()?)
        } else {
            None
        };
        if let Some(tag) = &tag {
            if !self.is("{") {
                if let Some(t) = self.lookup_tag(tag) {
                    return Ok(t.clone());
                }
                let t = Type::Record(Rc::new(RefCell::new(Record {
                    union,
                    ..Default::default()
                })));
                self.scopes
                    .last_mut()
                    .unwrap()
                    .tags
                    .insert(tag.clone(), t.clone());
                return Ok(t);
            }
        }
        self.expect("{")?;
        // Complete a forward declaration from the same scope in place.
        let rec = match tag
            .as_ref()
            .and_then(|t| self.scopes.last().unwrap().tags.get(t))
        {
            Some(Type::Record(r)) if !r.borrow().complete => r.clone(),
            _ => Rc::new(RefCell::new(Record {
                union,
                ..Default::default()
            })),
        };
        let ty = Type::Record(rec.clone());
        if let Some(tag) = &tag {
            self.scopes
                .last_mut()
                .unwrap()
                .tags
                .insert(tag.clone(), ty.clone());
        }
        let mut members = Vec::new();
        while !self.eat("}") {
            let base = self.declspec(None)?;
            if self.eat(";") {
                // Anonymous struct/union member.
                members.push(Member {
                    name: None,
                    ty: base,
                    offset: 0,
                });
                continue;
            }
            loop {
                let (mty, name) = self.declarator(base.clone())?;
                if self.eat(":") {
                    // Bit-fields are laid out as ordinary members.
                    self.const_int()?;
                }
                members.push(Member {
                    name,
                    ty: mty,
                    offset: 0,
                });
                if !self.eat(",") {
                    break;
                }
            }
            self.skip_attribute()?;
            self.expect(";")?;
        }
        self.skip_attribute()?;
        let mut size = 0;
        let mut align = 1;
        for m in &mut members {
            let a = m.ty.align();
            align = align.max(a);
            if union {
                m.offset = 0;
                size = size.max(m.ty.size());
            } else {
                m.offset = align_to(size, a);
                size = m.offset + m.ty.size();
            }
        }
        let mut r = rec.borrow_mut();
        r.members = members;
        r.size = align_to(size, align);
        r.align = align;
        r.complete = true;
        r.union = union;
        drop(r);
        Ok(ty)
    }

    fn enum_decl(&mut self) -> Result<Type, Error> {
        let tag = if let Tok::Ident(_) = self.peek().tok {
            Some(self.ident()?)
        } else {
            None
        };
        if let Some(tag) = &tag {
            if !self.is("{") {
                return Ok(self.lookup_tag(tag).cloned().unwrap_or(INT));
            }
        }
        self.expect("{")?;
        let mut v = 0i64;
        while !self.eat("}") {
            let name = self.ident()?;
            if self.eat("=") {
                v = self.const_int()?;
            }
            self.declare(name, Sym::Enum(v));
            v += 1;
            if !self.eat(",") {
                self.expect("}")?;
                break;
            }
        }
        if let Some(tag) = tag {
            self.scopes.last_mut().unwrap().tags.insert(tag, INT);
        }
        Ok(INT)
    }

    fn pointers(&mut self, mut ty: Type) -> Result<Type, Error> {
        while self.eat("*") {
            ty = Type::ptr(ty);
            while self.is("const")
                || self.is("volatile")
                || self.is("restrict")
                || self.is("__restrict")
                || self.is("__restrict__")
                || self.is("__const")
            {
                self.next();
            }
            self.skip_attribute()?;
        }
        Ok(ty)
    }

    /// Parses a (possibly abstract) declarator applied to `base`.
    fn declarator(&mut self, base: Type) -> Result<(Type, Option<Rc<str>>), Error> {
        let ty = self.pointers(base)?;
        self.skip_attribute()?;
        if self.is("(") && !self.paren_starts_params() {
            // Nested declarator: parse what follows the parentheses first.
            self.next();
            let start = self.pos;
            let mut depth = 1;
            while depth > 0 {
                if self.at_eof() {
                    return Err(self.err("unbalanced parentheses in declarator"));
                }
                let t = self.next();
                if t.is("(") {
                    depth += 1;
                } else if t.is(")") {
                    depth -= 1;
                }
            }
            let outer = self.type_suffix(ty)?;
            let end = self.pos;
            self.pos = start;
            let (ty, name) = self.declarator(outer)?;
            self.expect(")")?;
            self.pos = end;
            return Ok((ty, name));
        }
        let name = if let Tok::Ident(_) = self.peek().tok {
            Some(self.ident()?)
        } else {
            None
        };
        let ty = self.type_suffix(ty)?;
        self.skip_attribute()?;
        Ok((ty, name))
    }

    /// `(` begins a parameter list rather than a nested declarator.
    fn paren_starts_params(&self) -> bool {
        let n = self.peek_at(1);
        n.is(")") || n.is("...") || {
            let name = n.ident();
            name.is_some_and(|name| {
                matches!(
                    name,
                    "void"
                        | "_Bool"
                        | "char"
                        | "short"
                        | "int"
                        | "long"
                        | "float"
                        | "double"
                        | "signed"
                        | "unsigned"
                        | "struct"
                        | "union"
                        | "enum"
                        | "const"
                        | "volatile"
                        | "register"
                ) || matches!(self.lookup(name), Some(Sym::Typedef(_)))
            })
        }
    }

    fn type_suffix(&mut self, ty: Type) -> Result<Type, Error> {
        if self.eat("(") {
            return self.func_params(ty);
        }
        if self.eat("[") {
            while self.is("static")
                || self.is("const")
                || self.is("restrict")
                || self.is("volatile")
            {
                self.next();
            }
            let len = if self.eat("]") {
                None
            } else {
                let n = self.const_int()?;
                self.expect("]")?;
                Some(n as usize)
            };
            let elem = self.type_suffix(ty)?;
            return Ok(Type::Array(Rc::new(elem), len));
        }
        Ok(ty)
    }

    fn func_params(&mut self, ret: Type) -> Result<Type, Error> {
        let mut params = Vec::new();
        let mut variadic = false;
        let mut prototype = true;
        if self.is("void") && self.peek_at(1).is(")") {
            self.pos += 2;
        } else if self.eat(")") {
            prototype = false;
        } else {
            loop {
                if self.eat("...") {
                    variadic = true;
                    self.expect(")")?;
                    break;
                }
                let base = self.declspec(None)?;
                let (ty, name) = self.declarator(base)?;
                let ty = match ty {
                    Type::Array(e, _) => Type::Ptr(e),
                    Type::Func(_) => Type::ptr(ty),
                    t => t,
                };
                params.push((name, ty));
                if self.eat(")") {
                    break;
                }
                self.expect(",")?;
            }
        }
        let ft = FuncType {
            ret,
            params: params.iter().map(|p| p.1.clone()).collect(),
            variadic,
            prototype,
        };
        self.last_params = params;
        Ok(Type::Func(Rc::new(ft)))
    }

    fn typename(&mut self) -> Result<Type, Error> {
        let base = self.declspec(None)?;
        let (ty, _) = self.declarator(base)?;
        Ok(ty)
    }

    fn top_level(&mut self) -> Result<(), Error> {
        if self.eat(";") {
            return Ok(());
        }
        if self.is("_Static_assert") {
            self.next();
            self.expect("(")?;
            let v = self.const_int()?;
            let msg = if self.eat(",") {
                self.next().spelling()
            } else {
                String::new()
            };
            self.expect(")")?;
            self.expect(";")?;
            if v == 0 {
                return Err(self.err(format!("static assertion failed: {}", msg)));
            }
            return Ok(());
        }
        let mut st = Storage::default();
        let base = self.declspec(Some(&mut st))?;
        if self.eat(";") {
            return Ok(());
        }
        let mut first = true;
        loop {
            let (ty, name) = self.declarator(base.clone())?;
            let name = name.ok_or_else(|| self.err("expected identifier in declaration"))?;
            if st.typedef {
                self.declare(name, Sym::Typedef(ty));
            } else if let Type::Func(ft) = &ty {
                let link = match self.lookup(&name) {
                    Some(Sym::Global(l, _)) => l.clone(),
                    _ => self.link_name(&name, st.is_static),
                };
                self.declare(name.clone(), Sym::Global(link.clone(), ty.clone()));
                if first && self.is("{") {
                    return self.function(name, link, ft.clone());
                }
            } else {
                self.global_var(name, ty, st)?;
            }
            first = false;
            if !self.eat(",") {
                break;
            }
        }
        self.expect(";")
    }

    fn global_var(&mut self, name: Rc<str>, ty: Type, st: Storage) -> Result<(), Error> {
        let link = match self.lookup(&name) {
            Some(Sym::Global(l, _)) => l.clone(),
            _ => self.link_name(&name, st.is_static),
        };
        self.declare(name.clone(), Sym::Global(link.clone(), ty.clone()));
        if self.eat("=") {
            let (ty, data, relocs) = self.global_init(ty)?;
            self.declare(name, Sym::Global(link.clone(), ty.clone()));
            self.add_global(link, ty, Some(data), relocs)
        } else if !st.is_extern {
            self.add_global(link, ty, None, Vec::new())
        } else {
            Ok(())
        }
    }

    fn function(&mut self, name: Rc<str>, link: Rc<str>, ft: Rc<FuncType>) -> Result<(), Error> {
        let params = core::mem::take(&mut self.last_params);
        self.func = Some(FnState {
            locals: Vec::new(),
            ret: ft.ret.clone(),
            labels: BTreeMap::new(),
            defined_labels: Vec::new(),
        });
        self.scopes.push(Scope::default());
        let mut param_locals = Vec::new();
        for (pname, pty) in params {
            let idx = self.new_local(pty.clone());
            param_locals.push(idx);
            if let Some(n) = pname {
                self.declare(n, Sym::Local(idx, pty));
            }
        }
        // __func__
        let fname = self.string_global(name.as_bytes());
        self.declare(
            Rc::from("__func__"),
            Sym::Global(fname.0.clone(), fname.1.clone()),
        );
        self.declare(Rc::from("__FUNCTION__"), Sym::Global(fname.0, fname.1));
        self.expect("{")?;
        let body = self.block_items()?;
        self.scopes.pop();
        let f = self.func.take().unwrap();
        for (label, _) in f.labels.iter() {
            if !f.defined_labels.contains(label) {
                return Err(self.err(format!("label '{}' used but not defined", label)));
            }
        }
        self.prog.functions.push(Function {
            name: link,
            ty: ft,
            locals: f.locals,
            params: param_locals,
            body,
        });
        Ok(())
    }

    // ---- initializers --------------------------------------------------

    fn is_string_init(&self, ty: &Type) -> bool {
        matches!(ty, Type::Array(e, _) if matches!(**e, Type::Int { size: 1, .. }))
            && matches!(self.peek().tok, Tok::Str(_))
    }

    /// Parses an initializer for an object of type `ty`, returning the
    /// (possibly completed) type.
    fn initializer(
        &mut self,
        ty: &Type,
        offset: usize,
        out: &mut Vec<InitLeaf>,
    ) -> Result<Type, Error> {
        if self.is_string_init(ty) {
            let s = self.string_literal();
            let Type::Array(e, len) = ty else {
                unreachable!()
            };
            let len = len.unwrap_or(s.len() + 1);
            for (i, c) in s.iter().take(len).enumerate() {
                out.push(InitLeaf {
                    offset: offset + i,
                    ty: (**e).clone(),
                    expr: int(*c as i8 as i64),
                });
            }
            return Ok(Type::Array(e.clone(), Some(len)));
        }
        if self.eat("{") {
            let ty = self.init_list(ty, offset, out, true)?;
            self.eat(",");
            self.expect("}")?;
            return Ok(ty);
        }
        match ty {
            Type::Array(..) => self.init_list(ty, offset, out, false),
            Type::Record(_) => {
                // A struct can be initialized from an expression of its type.
                let save = self.pos;
                let e = self.assign()?;
                if e.ty.is_record() {
                    out.push(InitLeaf {
                        offset,
                        ty: ty.clone(),
                        expr: e,
                    });
                    return Ok(ty.clone());
                }
                self.pos = save;
                self.init_list(ty, offset, out, false)
            }
            _ => {
                let e = self.assign()?;
                let e = self.convert_assign(e, ty)?;
                out.push(InitLeaf {
                    offset,
                    ty: ty.clone(),
                    expr: e,
                });
                Ok(ty.clone())
            }
        }
    }

    /// Items of an aggregate initializer; `braced` is false for elided
    /// inner braces, which stop when the aggregate is full.
    fn init_list(
        &mut self,
        ty: &Type,
        offset: usize,
        out: &mut Vec<InitLeaf>,
        braced: bool,
    ) -> Result<Type, Error> {
        let mut first = true;
        let sep = |p: &mut Self, first: &mut bool| -> bool {
            if *first {
                *first = false;
                return !p.is("}");
            }
            if !p.is(",") || p.peek_at(1).is("}") {
                return false;
            }
            if !braced && (p.peek_at(1).is(".") || p.peek_at(1).is("[")) {
                return false;
            }
            p.next();
            true
        };
        match ty {
            Type::Array(e, len) => {
                let esz = e.size();
                let mut i = 0usize;
                let mut max = 0usize;
                while sep(self, &mut first) {
                    if braced && self.eat("[") {
                        i = self.const_int()? as usize;
                        self.expect("]")?;
                        self.eat("=");
                    } else if !braced && len.is_some_and(|l| i >= l) {
                        // Put the comma back for the enclosing list.
                        self.pos -= 1;
                        break;
                    }
                    self.initializer(e, offset + i * esz, out)?;
                    i += 1;
                    max = max.max(i);
                }
                Ok(Type::Array(e.clone(), Some(len.unwrap_or(max))))
            }
            Type::Record(r) => {
                let members: Vec<Member> = r.borrow().members.clone();
                let union = r.borrow().union;
                let mut m = 0usize;
                while sep(self, &mut first) {
                    if braced && self.is(".") {
                        self.next();
                        let name = self.ident()?;
                        m = members
                            .iter()
                            .position(|x| x.name.as_deref() == Some(&*name))
                            .ok_or_else(|| self.err(format!("no member named '{}'", name)))?;
                        self.eat("=");
                    } else if m >= members.len() || (union && m > 0) {
                        if braced {
                            return Err(self.err("excess elements in initializer"));
                        }
                        self.pos -= 1;
                        break;
                    }
                    let mem = &members[m];
                    self.initializer(&mem.ty, offset + mem.offset, out)?;
                    m += 1;
                }
                Ok(ty.clone())
            }
            _ => {
                if sep(self, &mut first) {
                    self.initializer(ty, offset, out)?;
                }
                Ok(ty.clone())
            }
        }
    }

    fn global_init(&mut self, ty: Type) -> Result<(Type, Vec<u8>, Vec<Reloc>), Error> {
        let mut leaves = Vec::new();
        let ty = self.initializer(&ty, 0, &mut leaves)?;
        let mut data = vec![0u8; ty.size()];
        let mut relocs = Vec::new();
        for leaf in leaves {
            self.write_const(&mut data, &mut relocs, leaf.offset, &leaf.ty, &leaf.expr)?;
        }
        Ok((ty, data, relocs))
    }

    fn write_const(
        &self,
        data: &mut [u8],
        relocs: &mut Vec<Reloc>,
        offset: usize,
        ty: &Type,
        e: &Expr,
    ) -> Result<(), Error> {
        match ty {
            Type::Float => data[offset..offset + 4]
                .copy_from_slice(&(self.eval_float(e)? as f32).to_le_bytes()),
            Type::Double => {
                data[offset..offset + 8].copy_from_slice(&self.eval_float(e)?.to_le_bytes())
            }
            Type::Record(_) => return Err(self.err("initializer element is not constant")),
            _ => {
                let (v, sym) = self.eval(e)?;
                let size = ty.size();
                if let Some(target) = sym {
                    if size != 8 {
                        return Err(self.err("initializer element is not computable at load time"));
                    }
                    relocs.push(Reloc {
                        offset,
                        target,
                        addend: v,
                    });
                } else {
                    data[offset..offset + size].copy_from_slice(&v.to_le_bytes()[..size]);
                }
            }
        }
        Ok(())
    }

    // ---- constant expressions ------------------------------------------

    fn const_int(&mut self) -> Result<i64, Error> {
        let e = self.conditional()?;
        let (v, sym) = self.eval(&e)?;
        if sym.is_some() {
            return Err(self.err("expected an integer constant expression"));
        }
        Ok(v)
    }

    fn eval_float(&self, e: &Expr) -> Result<f64, Error> {
        match &e.kind {
            ExprKind::Float(f) => Ok(*f),
            ExprKind::Cast(x) if x.ty.is_float() => self.eval_float(x),
            ExprKind::Cast(x) => Ok(if x.ty.is_unsigned() {
                self.eval(x)?.0 as u64 as f64
            } else {
                self.eval(x)?.0 as f64
            }),
            ExprKind::Neg(x) => Ok(-self.eval_float(x)?),
            ExprKind::Binary(op, l, r) if e.ty.is_float() => {
                let (a, c) = (self.eval_float(l)?, self.eval_float(r)?);
                Ok(match op {
                    BinOp::Add => a + c,
                    BinOp::Sub => a - c,
                    BinOp::Mul => a * c,
                    BinOp::Div => a / c,
                    _ => return Err(self.err("invalid floating constant expression")),
                })
            }
            ExprKind::Cond(c, a, x) => {
                if self.eval(c)?.0 != 0 {
                    self.eval_float(a)
                } else {
                    self.eval_float(x)
                }
            }
            _ => Ok(self.eval(e)?.0 as f64),
        }
    }

    /// Evaluates a constant expression: a value plus an optional symbol
    /// whose address it is relative to.
    fn eval(&self, e: &Expr) -> Result<(i64, Option<Rc<str>>), Error> {
        let not_const = || self.err("initializer element is not constant");
        let plain = |v: i64| Ok((v, None));
        match &e.kind {
            ExprKind::Int(v) => plain(*v),
            ExprKind::Float(f) => plain(*f as i64),
            ExprKind::Global(name) if e.ty.is_func() || matches!(e.ty, Type::Array(..)) => {
                Ok((0, Some(name.clone())))
            }
            ExprKind::Addr(x) => self.eval_addr(x),
            ExprKind::Cast(x) => {
                if x.ty.is_float() {
                    return plain(self.eval_float(x)? as i64);
                }
                let (v, s) = self.eval(x)?;
                if s.is_some() {
                    return Ok((v, s));
                }
                plain(match &e.ty {
                    Type::Bool => (v != 0) as i64,
                    Type::Int {
                        size: 1,
                        signed: true,
                    } => v as i8 as i64,
                    Type::Int {
                        size: 1,
                        signed: false,
                    } => v as u8 as i64,
                    Type::Int {
                        size: 2,
                        signed: true,
                    } => v as i16 as i64,
                    Type::Int {
                        size: 2,
                        signed: false,
                    } => v as u16 as i64,
                    Type::Int {
                        size: 4,
                        signed: true,
                    } => v as i32 as i64,
                    Type::Int {
                        size: 4,
                        signed: false,
                    } => v as u32 as i64,
                    _ => v,
                })
            }
            ExprKind::Binary(op, l, r) => {
                if l.ty.is_float() {
                    let (a, c) = (self.eval_float(l)?, self.eval_float(r)?);
                    return plain(match op {
                        BinOp::Eq => (a == c) as i64,
                        BinOp::Ne => (a != c) as i64,
                        BinOp::Lt => (a < c) as i64,
                        BinOp::Le => (a <= c) as i64,
                        _ => self.eval_float(e)? as i64,
                    });
                }
                let (a, sa) = self.eval(l)?;
                let (c, sc) = self.eval(r)?;
                if sa.is_some() || sc.is_some() {
                    return match (op, sa, sc) {
                        (BinOp::Add, Some(s), None) => Ok((a.wrapping_add(c), Some(s))),
                        (BinOp::Add, None, Some(s)) => Ok((a.wrapping_add(c), Some(s))),
                        (BinOp::Sub, Some(s), None) => Ok((a.wrapping_sub(c), Some(s))),
                        _ => Err(not_const()),
                    };
                }
                let unsigned = l.ty.is_unsigned();
                plain(match op {
                    BinOp::Add => a.wrapping_add(c),
                    BinOp::Sub => a.wrapping_sub(c),
                    BinOp::Mul => a.wrapping_mul(c),
                    BinOp::Div | BinOp::Mod if c == 0 => return Err(self.err("division by zero")),
                    BinOp::Div if unsigned => ((a as u64) / (c as u64)) as i64,
                    BinOp::Div => a.wrapping_div(c),
                    BinOp::Mod if unsigned => ((a as u64) % (c as u64)) as i64,
                    BinOp::Mod => a.wrapping_rem(c),
                    BinOp::And => a & c,
                    BinOp::Or => a | c,
                    BinOp::Xor => a ^ c,
                    BinOp::Shl => a.wrapping_shl(c as u32),
                    BinOp::Shr if unsigned => ((a as u64) >> (c as u32 & 63)) as i64,
                    BinOp::Shr => a.wrapping_shr(c as u32),
                    BinOp::Eq => (a == c) as i64,
                    BinOp::Ne => (a != c) as i64,
                    BinOp::Lt if unsigned => ((a as u64) < (c as u64)) as i64,
                    BinOp::Lt => (a < c) as i64,
                    BinOp::Le if unsigned => ((a as u64) <= (c as u64)) as i64,
                    BinOp::Le => (a <= c) as i64,
                })
                .and_then(|(v, s): (i64, Option<Rc<str>>)| Ok((self.wrap(v, &e.ty), s)))
            }
            ExprKind::Neg(x) => plain(self.wrap(self.eval_int(x)?.wrapping_neg(), &e.ty)),
            ExprKind::Not(x) => plain((self.eval_int(x)? == 0) as i64),
            ExprKind::BitNot(x) => plain(self.wrap(!self.eval_int(x)?, &e.ty)),
            ExprKind::LogAnd(l, r) => {
                plain((self.eval_int(l)? != 0 && self.eval_int(r)? != 0) as i64)
            }
            ExprKind::LogOr(l, r) => {
                plain((self.eval_int(l)? != 0 || self.eval_int(r)? != 0) as i64)
            }
            ExprKind::Cond(c, a, x) => {
                if self.eval_int(c)? != 0 {
                    self.eval(a)
                } else {
                    self.eval(x)
                }
            }
            ExprKind::Comma(_, x) => self.eval(x),
            ExprKind::Member(..) | ExprKind::Deref(_) if matches!(e.ty, Type::Array(..)) => {
                self.eval_addr(e)
            }
            _ => Err(not_const()),
        }
    }

    fn eval_int(&self, e: &Expr) -> Result<i64, Error> {
        match self.eval(e)? {
            (v, None) => Ok(v),
            _ => Err(self.err("expected an integer constant expression")),
        }
    }

    fn wrap(&self, v: i64, ty: &Type) -> i64 {
        match ty {
            Type::Int {
                size: 4,
                signed: true,
            } => v as i32 as i64,
            Type::Int {
                size: 4,
                signed: false,
            } => v as u32 as i64,
            _ => v,
        }
    }

    fn eval_addr(&self, e: &Expr) -> Result<(i64, Option<Rc<str>>), Error> {
        match &e.kind {
            ExprKind::Global(name) => Ok((0, Some(name.clone()))),
            ExprKind::Deref(x) => self.eval(x),
            ExprKind::Member(x, off) => {
                let (v, s) = self.eval_addr(x)?;
                Ok((v + *off as i64, s))
            }
            _ => Err(self.err("initializer element is not constant")),
        }
    }

    // ---- statements ----------------------------------------------------

    fn block_items(&mut self) -> Result<Vec<Stmt>, Error> {
        let mut items = Vec::new();
        while !self.eat("}") {
            if self.at_eof() {
                return Err(self.err("expected '}' at end of input"));
            }
            if self.is_typename() && !self.peek_at(1).is(":") {
                self.local_decl(&mut items)?;
            } else {
                items.push(self.stmt()?);
            }
        }
        Ok(items)
    }

    fn local_decl(&mut self, out: &mut Vec<Stmt>) -> Result<(), Error> {
        let mut st = Storage::default();
        let base = self.declspec(Some(&mut st))?;
        if self.eat(";") {
            return Ok(());
        }
        loop {
            let (ty, name) = self.declarator(base.clone())?;
            let name = name.ok_or_else(|| self.err("expected identifier in declaration"))?;
            if st.typedef {
                self.declare(name, Sym::Typedef(ty));
            } else if ty.is_func() || st.is_extern {
                let link = name.clone();
                self.declare(name, Sym::Global(link, ty));
            } else if st.is_static {
                let link = self.anon_name(&format!("{}.", name));
                self.declare(name.clone(), Sym::Global(link.clone(), ty.clone()));
                if self.eat("=") {
                    let (ty, data, relocs) = self.global_init(ty)?;
                    self.declare(name, Sym::Global(link.clone(), ty.clone()));
                    self.add_global(link, ty, Some(data), relocs)?;
                } else {
                    self.add_global(link, ty, None, Vec::new())?;
                }
            } else {
                if ty.is_void() {
                    return Err(self.err(format!("variable '{}' declared void", name)));
                }
                let idx = self.new_local(ty.clone());
                self.declare(name.clone(), Sym::Local(idx, ty.clone()));
                if self.eat("=") {
                    let mut leaves = Vec::new();
                    let ty = self.initializer(&ty, 0, &mut leaves)?;
                    self.func.as_mut().unwrap().locals[idx].ty = ty.clone();
                    self.declare(name, Sym::Local(idx, ty.clone()));
                    for s in self.init_stmts(idx, &ty, leaves) {
                        out.push(Stmt::Expr(s));
                    }
                }
            }
            if !self.eat(",") {
                break;
            }
        }
        self.expect(";")
    }

    /// Statements that initialize local `idx` from initializer leaves.
    fn init_stmts(&mut self, idx: usize, ty: &Type, leaves: Vec<InitLeaf>) -> Vec<Expr> {
        let var = self.local_expr(idx);
        let mut out = Vec::new();
        let whole = leaves.len() == 1
            && leaves[0].offset == 0
            && leaves[0].ty.same(ty)
            && !matches!(ty, Type::Array(..));
        if !whole {
            out.push(expr(ExprKind::Zero(idx), Type::Void));
        }
        for leaf in leaves {
            let target = if whole {
                var.clone()
            } else {
                expr(
                    ExprKind::Member(b(var.clone()), leaf.offset),
                    leaf.ty.clone(),
                )
            };
            out.push(expr(ExprKind::Assign(b(target), b(leaf.expr)), leaf.ty));
        }
        out
    }

    fn stmt(&mut self) -> Result<Stmt, Error> {
        let t = self.peek().clone();
        if self.eat("return") {
            if self.eat(";") {
                return Ok(Stmt::Return(None));
            }
            let e = self.expr()?;
            self.expect(";")?;
            let ret = self.func.as_ref().unwrap().ret.clone();
            if ret.is_void() {
                return Ok(Stmt::Block(vec![Stmt::Expr(e), Stmt::Return(None)]));
            }
            let e = self.convert_assign(e, &ret)?;
            return Ok(Stmt::Return(Some(e)));
        }
        if self.eat("if") {
            self.expect("(")?;
            let c = self.cond_expr()?;
            self.expect(")")?;
            let then = self.stmt()?;
            let els = if self.eat("else") {
                Some(Box::new(self.stmt()?))
            } else {
                None
            };
            return Ok(Stmt::If(c, Box::new(then), els));
        }
        if self.eat("while") {
            self.expect("(")?;
            let c = self.cond_expr()?;
            self.expect(")")?;
            let (brk, cont) = (self.new_label(), self.new_label());
            let body = self.loop_body(brk, cont)?;
            return Ok(Stmt::Loop {
                init: None,
                cond: Some(c),
                step: None,
                body: Box::new(body),
                brk,
                cont,
                post_test: false,
            });
        }
        if self.eat("do") {
            let (brk, cont) = (self.new_label(), self.new_label());
            let body = self.loop_body(brk, cont)?;
            self.expect("while")?;
            self.expect("(")?;
            let c = self.cond_expr()?;
            self.expect(")")?;
            self.expect(";")?;
            return Ok(Stmt::Loop {
                init: None,
                cond: Some(c),
                step: None,
                body: Box::new(body),
                brk,
                cont,
                post_test: true,
            });
        }
        if self.eat("for") {
            self.expect("(")?;
            self.scopes.push(Scope::default());
            let init = if self.eat(";") {
                None
            } else if self.is_typename() {
                let mut v = Vec::new();
                self.local_decl(&mut v)?;
                Some(Box::new(Stmt::Block(v)))
            } else {
                let e = self.expr()?;
                self.expect(";")?;
                Some(Box::new(Stmt::Expr(e)))
            };
            let cond = if self.is(";") {
                None
            } else {
                Some(self.cond_expr()?)
            };
            self.expect(";")?;
            let step = if self.is(")") {
                None
            } else {
                Some(self.expr()?)
            };
            self.expect(")")?;
            let (brk, cont) = (self.new_label(), self.new_label());
            let body = self.loop_body(brk, cont)?;
            self.scopes.pop();
            return Ok(Stmt::Loop {
                init,
                cond,
                step,
                body: Box::new(body),
                brk,
                cont,
                post_test: false,
            });
        }
        if self.eat("switch") {
            self.expect("(")?;
            let c = self.expr()?;
            if !c.ty.is_integer() {
                return Err(self.err_at(&t, "switch quantity not an integer"));
            }
            let c = cast(c.clone(), &c.ty.promote());
            self.expect(")")?;
            let brk = self.new_label();
            self.switches.push(SwitchState {
                cases: Vec::new(),
                default: None,
            });
            self.brk.push(brk);
            let body = self.stmt()?;
            self.brk.pop();
            let sw = self.switches.pop().unwrap();
            return Ok(Stmt::Switch {
                cond: c,
                body: Box::new(body),
                cases: sw.cases,
                default: sw.default,
                brk,
            });
        }
        if self.eat("case") {
            let lo = self.const_int()?;
            let hi = if self.eat("...") {
                self.const_int()?
            } else {
                lo
            };
            self.expect(":")?;
            let label = self.new_label();
            if self.switches.is_empty() {
                return Err(self.err_at(&t, "case label not within a switch statement"));
            }
            let sw = self.switches.last_mut().unwrap();
            sw.cases.push((lo, hi, label));
            return Ok(Stmt::Block(vec![Stmt::Label(label), self.stmt_or_empty()?]));
        }
        if self.eat("default") {
            self.expect(":")?;
            let label = self.new_label();
            if self.switches.is_empty() {
                return Err(self.err_at(&t, "'default' label not within a switch statement"));
            }
            let sw = self.switches.last_mut().unwrap();
            sw.default = Some(label);
            return Ok(Stmt::Block(vec![Stmt::Label(label), self.stmt_or_empty()?]));
        }
        if self.eat("break") {
            self.expect(";")?;
            let l = *self
                .brk
                .last()
                .ok_or_else(|| self.err_at(&t, "break statement not within loop or switch"))?;
            return Ok(Stmt::Goto(l));
        }
        if self.eat("continue") {
            self.expect(";")?;
            let l = *self
                .cont
                .last()
                .ok_or_else(|| self.err_at(&t, "continue statement not within a loop"))?;
            return Ok(Stmt::Goto(l));
        }
        if self.eat("goto") {
            let name = self.ident()?;
            self.expect(";")?;
            return Ok(Stmt::Goto(self.named_label(name)));
        }
        if t.ident().is_some() && self.peek_at(1).is(":") {
            let name = self.ident()?;
            self.next();
            let l = self.named_label(name.clone());
            self.func.as_mut().unwrap().defined_labels.push(name);
            return Ok(Stmt::Block(vec![Stmt::Label(l), self.stmt_or_empty()?]));
        }
        if self.eat("{") {
            self.scopes.push(Scope::default());
            let items = self.block_items()?;
            self.scopes.pop();
            return Ok(Stmt::Block(items));
        }
        if self.eat(";") {
            return Ok(Stmt::Block(Vec::new()));
        }
        let e = self.expr()?;
        self.expect(";")?;
        Ok(Stmt::Expr(e))
    }

    /// A labeled statement at the end of a block may have no statement.
    fn stmt_or_empty(&mut self) -> Result<Stmt, Error> {
        if self.is("}") {
            return Ok(Stmt::Block(Vec::new()));
        }
        if self.is_typename() {
            let mut v = Vec::new();
            self.local_decl(&mut v)?;
            return Ok(Stmt::Block(v));
        }
        self.stmt()
    }

    fn loop_body(&mut self, brk: usize, cont: usize) -> Result<Stmt, Error> {
        self.brk.push(brk);
        self.cont.push(cont);
        let body = self.stmt();
        self.brk.pop();
        self.cont.pop();
        body
    }

    fn named_label(&mut self, name: Rc<str>) -> usize {
        if let Some(&l) = self.func.as_ref().unwrap().labels.get(&name) {
            return l;
        }
        let l = self.new_label();
        self.func.as_mut().unwrap().labels.insert(name, l);
        l
    }

    fn cond_expr(&mut self) -> Result<Expr, Error> {
        let e = self.expr()?;
        self.scalar(&e)?;
        Ok(e)
    }

    fn scalar(&self, e: &Expr) -> Result<(), Error> {
        if e.ty.is_scalar() || e.ty.is_pointer_like() || e.ty.is_func() {
            Ok(())
        } else {
            Err(self.err("used a value that is not a scalar where a scalar is required"))
        }
    }

    // ---- expressions ---------------------------------------------------

    pub fn expr(&mut self) -> Result<Expr, Error> {
        let mut e = self.assign()?;
        while self.eat(",") {
            let r = self.assign()?;
            let ty = r.ty.clone();
            e = expr(ExprKind::Comma(b(e), b(r)), ty);
        }
        Ok(e)
    }

    /// Converts `e` for assignment to an object of type `ty`.
    fn convert_assign(&self, e: Expr, ty: &Type) -> Result<Expr, Error> {
        if ty.is_record() {
            if !e.ty.is_record() {
                return Err(self.err("incompatible types in assignment"));
            }
            return Ok(e);
        }
        if e.ty.is_record() {
            return Err(self.err("incompatible types in assignment"));
        }
        if e.ty.is_void() {
            return Err(self.err("void value not ignored as it ought to be"));
        }
        Ok(cast(e, ty))
    }

    fn assign_to(&self, l: Expr, r: Expr) -> Result<Expr, Error> {
        if !is_lvalue(&l) {
            return Err(self.err("lvalue required as left operand of assignment"));
        }
        if matches!(l.ty, Type::Array(..)) {
            return Err(self.err("assignment to expression with array type"));
        }
        let r = self.convert_assign(r, &l.ty)?;
        let ty = l.ty.clone();
        Ok(expr(ExprKind::Assign(b(l), b(r)), ty))
    }

    /// `l op= r` (and `++`/`--`): evaluates the address of `l` once.
    fn compound(&mut self, l: Expr, op: BinOp, r: Expr) -> Result<Expr, Error> {
        if is_simple(&l) {
            let v = self.binary(op, l.clone(), r)?;
            return self.assign_to(l, v);
        }
        if !is_lvalue(&l) {
            return Err(self.err("lvalue required as left operand of assignment"));
        }
        let pty = Type::ptr(l.ty.clone());
        let tmp = self.new_local(pty.clone());
        let tmp_e = self.local_expr(tmp);
        let set = expr(
            ExprKind::Assign(b(tmp_e.clone()), b(expr(ExprKind::Addr(b(l.clone())), pty))),
            tmp_e.ty.clone(),
        );
        let target = expr(ExprKind::Deref(b(tmp_e)), l.ty.clone());
        let v = self.binary(op, target.clone(), r)?;
        let a = self.assign_to(target, v)?;
        let ty = a.ty.clone();
        Ok(expr(ExprKind::Comma(b(set), b(a)), ty))
    }

    fn post_incdec(&mut self, l: Expr, delta: i64) -> Result<Expr, Error> {
        if !is_lvalue(&l) {
            return Err(self.err("lvalue required as increment operand"));
        }
        let ty = l.ty.clone();
        let old = self.new_local(ty.clone());
        let old_e = self.local_expr(old);
        if is_simple(&l) {
            let save = expr(ExprKind::Assign(b(old_e.clone()), b(l.clone())), ty.clone());
            let v = self.binary(BinOp::Add, l.clone(), long(delta))?;
            let upd = self.assign_to(l, v)?;
            return Ok(expr(
                ExprKind::Comma(
                    b(save),
                    b(expr(ExprKind::Comma(b(upd), b(old_e)), ty.clone())),
                ),
                ty,
            ));
        }
        let pty = Type::ptr(ty.clone());
        let tmp = self.new_local(pty.clone());
        let tmp_e = self.local_expr(tmp);
        let set = expr(
            ExprKind::Assign(b(tmp_e.clone()), b(expr(ExprKind::Addr(b(l)), pty.clone()))),
            pty,
        );
        let target = expr(ExprKind::Deref(b(tmp_e)), ty.clone());
        let save = expr(
            ExprKind::Assign(b(old_e.clone()), b(target.clone())),
            ty.clone(),
        );
        let v = self.binary(BinOp::Add, old_e.clone(), long(delta))?;
        let upd = self.assign_to(target, v)?;
        let seq = expr(ExprKind::Comma(b(upd), b(old_e)), ty.clone());
        let seq = expr(ExprKind::Comma(b(save), b(seq)), ty.clone());
        Ok(expr(ExprKind::Comma(b(set), b(seq)), ty))
    }

    fn assign(&mut self) -> Result<Expr, Error> {
        let l = self.conditional()?;
        if self.eat("=") {
            let r = self.assign()?;
            return self.assign_to(l, r);
        }
        let ops = [
            ("+=", BinOp::Add),
            ("-=", BinOp::Sub),
            ("*=", BinOp::Mul),
            ("/=", BinOp::Div),
            ("%=", BinOp::Mod),
            ("&=", BinOp::And),
            ("|=", BinOp::Or),
            ("^=", BinOp::Xor),
            ("<<=", BinOp::Shl),
            (">>=", BinOp::Shr),
        ];
        for (p, op) in ops {
            if self.eat(p) {
                let r = self.assign()?;
                return self.compound(l, op, r);
            }
        }
        Ok(l)
    }

    fn conditional(&mut self) -> Result<Expr, Error> {
        let c = self.log_or()?;
        if !self.eat("?") {
            return Ok(c);
        }
        self.scalar(&c)?;
        // GNU `a ?: b`
        if self.eat(":") {
            let r = self.conditional()?;
            let ty = common(&c.ty, &r.ty);
            let tmp = self.new_local(ty.clone());
            let t = self.local_expr(tmp);
            let set = self.assign_to(t.clone(), c)?;
            let rest = expr(
                ExprKind::Cond(b(t.clone()), b(t), b(cast(r, &ty))),
                ty.clone(),
            );
            return Ok(expr(ExprKind::Comma(b(set), b(rest)), ty));
        }
        let a = self.expr()?;
        self.expect(":")?;
        let x = self.conditional()?;
        let (a, x, ty) = if a.ty.is_void() || x.ty.is_void() {
            (a, x, Type::Void)
        } else if a.ty.is_arith() && x.ty.is_arith() {
            let ty = common(&a.ty, &x.ty);
            (cast(a, &ty), cast(x, &ty), ty)
        } else if a.ty.is_record() {
            let ty = a.ty.clone();
            (a, x, ty)
        } else {
            // Pointers (a null constant takes the other side's type).
            let ty = if a.ty.is_pointer_like() || a.ty.is_func() {
                a.ty.decay()
            } else {
                x.ty.decay()
            };
            (cast(a, &ty), cast(x, &ty), ty)
        };
        Ok(expr(ExprKind::Cond(b(c), b(a), b(x)), ty))
    }

    fn log_or(&mut self) -> Result<Expr, Error> {
        let mut e = self.log_and()?;
        while self.eat("||") {
            let r = self.log_and()?;
            self.scalar(&e)?;
            self.scalar(&r)?;
            e = expr(ExprKind::LogOr(b(e), b(r)), INT);
        }
        Ok(e)
    }

    fn log_and(&mut self) -> Result<Expr, Error> {
        let mut e = self.bit_or()?;
        while self.eat("&&") {
            let r = self.bit_or()?;
            self.scalar(&e)?;
            self.scalar(&r)?;
            e = expr(ExprKind::LogAnd(b(e), b(r)), INT);
        }
        Ok(e)
    }

    fn bit_or(&mut self) -> Result<Expr, Error> {
        let mut e = self.bit_xor()?;
        while self.eat("|") {
            let r = self.bit_xor()?;
            e = self.binary(BinOp::Or, e, r)?;
        }
        Ok(e)
    }

    fn bit_xor(&mut self) -> Result<Expr, Error> {
        let mut e = self.bit_and()?;
        while self.eat("^") {
            let r = self.bit_and()?;
            e = self.binary(BinOp::Xor, e, r)?;
        }
        Ok(e)
    }

    fn bit_and(&mut self) -> Result<Expr, Error> {
        let mut e = self.equality()?;
        while self.eat("&") {
            let r = self.equality()?;
            e = self.binary(BinOp::And, e, r)?;
        }
        Ok(e)
    }

    fn equality(&mut self) -> Result<Expr, Error> {
        let mut e = self.relational()?;
        loop {
            if self.eat("==") {
                let r = self.relational()?;
                e = self.binary(BinOp::Eq, e, r)?;
            } else if self.eat("!=") {
                let r = self.relational()?;
                e = self.binary(BinOp::Ne, e, r)?;
            } else {
                return Ok(e);
            }
        }
    }

    fn relational(&mut self) -> Result<Expr, Error> {
        let mut e = self.shift()?;
        loop {
            if self.eat("<") {
                let r = self.shift()?;
                e = self.binary(BinOp::Lt, e, r)?;
            } else if self.eat("<=") {
                let r = self.shift()?;
                e = self.binary(BinOp::Le, e, r)?;
            } else if self.eat(">") {
                let r = self.shift()?;
                e = self.binary(BinOp::Lt, r, e)?;
            } else if self.eat(">=") {
                let r = self.shift()?;
                e = self.binary(BinOp::Le, r, e)?;
            } else {
                return Ok(e);
            }
        }
    }

    fn shift(&mut self) -> Result<Expr, Error> {
        let mut e = self.additive()?;
        loop {
            if self.eat("<<") {
                let r = self.additive()?;
                e = self.binary(BinOp::Shl, e, r)?;
            } else if self.eat(">>") {
                let r = self.additive()?;
                e = self.binary(BinOp::Shr, e, r)?;
            } else {
                return Ok(e);
            }
        }
    }

    fn additive(&mut self) -> Result<Expr, Error> {
        let mut e = self.multiplicative()?;
        loop {
            if self.eat("+") {
                let r = self.multiplicative()?;
                e = self.binary(BinOp::Add, e, r)?;
            } else if self.eat("-") {
                let r = self.multiplicative()?;
                e = self.binary(BinOp::Sub, e, r)?;
            } else {
                return Ok(e);
            }
        }
    }

    fn multiplicative(&mut self) -> Result<Expr, Error> {
        let mut e = self.cast_expr()?;
        loop {
            let op = if self.eat("*") {
                BinOp::Mul
            } else if self.eat("/") {
                BinOp::Div
            } else if self.eat("%") {
                BinOp::Mod
            } else {
                return Ok(e);
            };
            let r = self.cast_expr()?;
            e = self.binary(op, e, r)?;
        }
    }

    /// Builds a binary operation with C's conversions and pointer
    /// arithmetic.
    fn binary(&self, op: BinOp, l: Expr, r: Expr) -> Result<Expr, Error> {
        let lp = l.ty.is_pointer_like() || l.ty.is_func();
        let rp = r.ty.is_pointer_like() || r.ty.is_func();
        match op {
            BinOp::Add | BinOp::Sub if lp || rp => {
                if lp && rp {
                    if op == BinOp::Add {
                        return Err(self.err("invalid operands to binary +"));
                    }
                    // Pointer difference.
                    let size = l.ty.base().map_or(1, |t| t.size().max(1)) as i64;
                    let diff = expr(
                        ExprKind::Binary(BinOp::Sub, b(cast(l, &LONG)), b(cast(r, &LONG))),
                        LONG,
                    );
                    return Ok(expr(
                        ExprKind::Binary(BinOp::Div, b(diff), b(long(size))),
                        LONG,
                    ));
                }
                let (p, i) = if lp { (l, r) } else { (r, l) };
                if !i.ty.is_integer() {
                    return Err(self.err("invalid operands to pointer arithmetic"));
                }
                if op == BinOp::Sub && !lp {
                    return Err(self.err("invalid operands to binary -"));
                }
                let pty = p.ty.decay();
                let size = pty.base().map_or(1, |t| {
                    if t.is_void() || t.is_func() {
                        1
                    } else {
                        t.size()
                    }
                }) as i64;
                let scaled = expr(
                    ExprKind::Binary(BinOp::Mul, b(cast(i, &LONG)), b(long(size))),
                    LONG,
                );
                let p = cast(p, &pty);
                return Ok(expr(ExprKind::Binary(op, b(p), b(scaled)), pty));
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le if lp || rp => {
                let l = cast(l, &ULONG);
                let r = cast(r, &ULONG);
                return Ok(expr(ExprKind::Binary(op, b(l), b(r)), INT));
            }
            _ => {}
        }
        if !l.ty.is_arith() || !r.ty.is_arith() {
            return Err(self.err("invalid operands to binary expression"));
        }
        match op {
            BinOp::Shl | BinOp::Shr => {
                if !l.ty.is_integer() || !r.ty.is_integer() {
                    return Err(self.err("invalid operands to shift"));
                }
                let ty = l.ty.promote();
                Ok(expr(
                    ExprKind::Binary(op, b(cast(l, &ty)), b(cast(r, &INT))),
                    ty,
                ))
            }
            BinOp::Mod | BinOp::And | BinOp::Or | BinOp::Xor
                if l.ty.is_float() || r.ty.is_float() =>
            {
                Err(self.err("invalid operands to binary expression"))
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le => {
                let ty = common(&l.ty, &r.ty);
                Ok(expr(
                    ExprKind::Binary(op, b(cast(l, &ty)), b(cast(r, &ty))),
                    INT,
                ))
            }
            _ => {
                let ty = common(&l.ty, &r.ty);
                Ok(expr(
                    ExprKind::Binary(op, b(cast(l, &ty)), b(cast(r, &ty))),
                    ty,
                ))
            }
        }
    }

    fn cast_expr(&mut self) -> Result<Expr, Error> {
        if self.is("(") && self.peek_at(1).ident().is_some() {
            let save = self.pos;
            self.next();
            if self.is_typename() {
                let ty = self.typename()?;
                self.expect(")")?;
                if self.is("{") {
                    self.pos = save;
                    return self.unary();
                }
                let e = self.cast_expr()?;
                if ty.is_void() {
                    return Ok(expr(ExprKind::Cast(b(e)), Type::Void));
                }
                if !ty.is_scalar() {
                    return Err(self.err("conversion to non-scalar type requested"));
                }
                return Ok(expr(ExprKind::Cast(b(e)), ty));
            }
            self.pos = save;
        }
        self.unary()
    }

    fn unary(&mut self) -> Result<Expr, Error> {
        if self.eat("+") {
            let e = self.cast_expr()?;
            let ty = e.ty.promote();
            return Ok(cast(e, &ty));
        }
        if self.eat("-") {
            let e = self.cast_expr()?;
            if !e.ty.is_arith() {
                return Err(self.err("wrong type argument to unary minus"));
            }
            let ty = e.ty.promote();
            return Ok(expr(ExprKind::Neg(b(cast(e, &ty))), ty));
        }
        if self.eat("!") {
            let e = self.cast_expr()?;
            self.scalar(&e)?;
            return Ok(expr(ExprKind::Not(b(e)), INT));
        }
        if self.eat("~") {
            let e = self.cast_expr()?;
            if !e.ty.is_integer() {
                return Err(self.err("wrong type argument to bit-complement"));
            }
            let ty = e.ty.promote();
            return Ok(expr(ExprKind::BitNot(b(cast(e, &ty))), ty));
        }
        if self.eat("&") {
            let e = self.cast_expr()?;
            return self.addr_of(e);
        }
        if self.eat("*") {
            let e = self.cast_expr()?;
            return self.deref(e);
        }
        if self.eat("++") {
            let e = self.unary()?;
            return self.compound(e, BinOp::Add, int(1));
        }
        if self.eat("--") {
            let e = self.unary()?;
            return self.compound(e, BinOp::Sub, int(1));
        }
        if self.eat("sizeof") {
            if self.is("(") && self.peek_at(1).ident().is_some() {
                let save = self.pos;
                self.next();
                if self.is_typename() {
                    let ty = self.typename()?;
                    self.expect(")")?;
                    if !self.is("{") {
                        return Ok(expr(ExprKind::Int(ty.size() as i64), ULONG));
                    }
                }
                self.pos = save;
            }
            let e = self.unary()?;
            return Ok(expr(ExprKind::Int(e.ty.size() as i64), ULONG));
        }
        if self.eat("_Alignof") || self.eat("__alignof__") {
            self.expect("(")?;
            let ty = if self.is_typename() {
                self.typename()?
            } else {
                self.expr()?.ty
            };
            self.expect(")")?;
            return Ok(expr(ExprKind::Int(ty.align() as i64), ULONG));
        }
        if self.eat("&&") {
            return Err(self.err("labels as values are not supported"));
        }
        self.postfix()
    }

    fn addr_of(&self, e: Expr) -> Result<Expr, Error> {
        match &e.kind {
            // &*p is p
            ExprKind::Deref(x) if !e.ty.is_func() => {
                let ty = Type::ptr(e.ty.clone());
                return Ok(cast((**x).clone(), &ty));
            }
            _ => {}
        }
        if !is_lvalue(&e)
            && !e.ty.is_func()
            && !matches!(e.kind, ExprKind::Call { .. } | ExprKind::Comma(..))
        {
            return Err(self.err("lvalue required as unary '&' operand"));
        }
        let ty = Type::ptr(e.ty.clone());
        Ok(expr(ExprKind::Addr(b(e)), ty))
    }

    fn deref(&self, e: Expr) -> Result<Expr, Error> {
        if e.ty.is_func() {
            return Ok(e);
        }
        let Some(base) = e.ty.base().cloned() else {
            return Err(self.err("invalid type argument of unary '*'"));
        };
        if base.is_void() {
            return Err(self.err("dereferencing 'void *' pointer"));
        }
        Ok(expr(ExprKind::Deref(b(e)), base))
    }

    fn member(&self, e: Expr, name: &str) -> Result<Expr, Error> {
        if let Type::Record(r) = &e.ty {
            if !r.borrow().complete {
                return Err(self.err("dereferencing pointer to incomplete type"));
            }
        }
        let (ty, off) =
            e.ty.member(name)
                .ok_or_else(|| self.err(format!("no member named '{}'", name)))?;
        Ok(expr(ExprKind::Member(b(e), off), ty))
    }

    fn postfix(&mut self) -> Result<Expr, Error> {
        // Compound literal.
        if self.is("(") && self.peek_at(1).ident().is_some() {
            let save = self.pos;
            self.next();
            if self.is_typename() {
                let ty = self.typename()?;
                self.expect(")")?;
                if self.is("{") && self.func.is_some() {
                    let idx = self.new_local(ty.clone());
                    let mut leaves = Vec::new();
                    let ty = self.initializer(&ty, 0, &mut leaves)?;
                    self.func.as_mut().unwrap().locals[idx].ty = ty.clone();
                    let mut e = self.local_expr(idx);
                    for s in self.init_stmts(idx, &ty, leaves).into_iter().rev() {
                        e = expr(ExprKind::Comma(b(s), b(e)), ty.clone());
                    }
                    return self.postfix_ops(e);
                }
                if self.is("{") {
                    let name = self.anon_name("compound");
                    let (ty, data, relocs) = self.global_init(ty)?;
                    self.prog.globals.push(Global {
                        name: name.clone(),
                        ty: ty.clone(),
                        data: Some(data),
                        relocs,
                    });
                    return self.postfix_ops(expr(ExprKind::Global(name), ty));
                }
            }
            self.pos = save;
        }
        let e = self.primary()?;
        self.postfix_ops(e)
    }

    fn postfix_ops(&mut self, mut e: Expr) -> Result<Expr, Error> {
        loop {
            if self.eat("[") {
                let i = self.expr()?;
                self.expect("]")?;
                let p = self.binary(BinOp::Add, e, i)?;
                e = self.deref(p)?;
            } else if self.eat(".") {
                let name = self.ident()?;
                e = self.member(e, &name)?;
            } else if self.eat("->") {
                let name = self.ident()?;
                let d = self.deref(e)?;
                e = self.member(d, &name)?;
            } else if self.eat("++") {
                e = self.post_incdec(e, 1)?;
            } else if self.eat("--") {
                e = self.post_incdec(e, -1)?;
            } else if self.is("(") {
                self.next();
                e = self.call(e)?;
            } else {
                return Ok(e);
            }
        }
    }

    fn call(&mut self, f: Expr) -> Result<Expr, Error> {
        let ft =
            f.ty.func()
                .cloned()
                .ok_or_else(|| self.err("called object is not a function or function pointer"))?;
        let mut args = Vec::new();
        while !self.eat(")") {
            if !args.is_empty() {
                self.expect(",")?;
            }
            let a = self.assign()?;
            let i = args.len();
            let a = if let Some(pt) = ft.params.get(i) {
                self.convert_assign(a, pt)?
            } else {
                if ft.prototype && !ft.variadic {
                    return Err(self.err("too many arguments to function"));
                }
                // Default argument promotions.
                match a.ty.clone() {
                    Type::Float => cast(a, &Type::Double),
                    t if t.is_integer() => cast(a, &t.promote()),
                    t if t.is_pointer_like() || t.is_func() => {
                        let d = t.decay();
                        cast(a, &d)
                    }
                    _ => a,
                }
            };
            args.push(a);
        }
        if ft.prototype && args.len() < ft.params.len() {
            return Err(self.err("too few arguments to function"));
        }
        let ret = if ft.ret.is_record() {
            Some(self.new_local(ft.ret.clone()))
        } else {
            None
        };
        Ok(expr(
            ExprKind::Call {
                func: b(f),
                args,
                ret,
            },
            ft.ret.clone(),
        ))
    }

    fn string_literal(&mut self) -> Vec<u8> {
        let mut s = Vec::new();
        while let Tok::Str(part) = &self.peek().tok {
            s.extend_from_slice(part);
            self.next();
        }
        s
    }

    /// Interns a string literal as an anonymous global.
    fn string_global(&mut self, s: &[u8]) -> (Rc<str>, Type) {
        let name = self.anon_name("str");
        let mut data = s.to_vec();
        data.push(0);
        let ty = Type::Array(Rc::new(CHAR), Some(data.len()));
        self.prog.globals.push(Global {
            name: name.clone(),
            ty: ty.clone(),
            data: Some(data),
            relocs: Vec::new(),
        });
        (name, ty)
    }

    fn number(&self, t: &Token, s: &str) -> Result<Expr, Error> {
        let lower = s.to_ascii_lowercase();
        let is_hex = lower.starts_with("0x");
        let is_float = (!is_hex && (lower.contains('.') || lower.contains('e')))
            || (is_hex && lower.contains('p'));
        if is_float {
            let (body, ty) = if let Some(b) = lower.strip_suffix('f') {
                (b, Type::Float)
            } else if let Some(b) = lower.strip_suffix('l') {
                (b, Type::Double)
            } else {
                (lower.as_str(), Type::Double)
            };
            let v = if is_hex {
                parse_hex_float(body)
            } else {
                body.parse::<f64>().ok()
            };
            let v =
                v.ok_or_else(|| self.err_at(t, format!("invalid floating constant '{}'", s)))?;
            let v = if matches!(ty, Type::Float) {
                v as f32 as f64
            } else {
                v
            };
            return Ok(expr(ExprKind::Float(v), ty));
        }
        let v = parse_int_literal(s)
            .ok_or_else(|| self.err_at(t, format!("invalid integer constant '{}'", s)))?;
        let suffix: String = lower
            .chars()
            .rev()
            .take_while(|c| *c == 'u' || *c == 'l')
            .collect();
        let unsigned = suffix.contains('u');
        let long_ = suffix.contains('l');
        let decimal = !is_hex && !(s.len() > 1 && s.starts_with('0'));
        let u = v as u64;
        let ty = if long_ {
            if unsigned || (!decimal && u > i64::MAX as u64) {
                ULONG
            } else {
                LONG
            }
        } else if unsigned {
            if u <= u32::MAX as u64 {
                UINT
            } else {
                ULONG
            }
        } else if u <= i32::MAX as u64 {
            INT
        } else if !decimal && u <= u32::MAX as u64 {
            UINT
        } else if decimal || u <= i64::MAX as u64 {
            LONG
        } else {
            ULONG
        };
        Ok(expr(ExprKind::Int(v), ty))
    }

    fn primary(&mut self) -> Result<Expr, Error> {
        let t = self.peek().clone();
        match &t.tok {
            Tok::Num(s) => {
                self.next();
                self.number(&t, s)
            }
            Tok::Char(c) => {
                self.next();
                Ok(int(*c))
            }
            Tok::Str(_) => {
                let s = self.string_literal();
                let (name, ty) = self.string_global(&s);
                Ok(expr(ExprKind::Global(name), ty))
            }
            Tok::Punct("(") => {
                self.next();
                if self.is("{") {
                    // GNU statement expression.
                    self.next();
                    self.scopes.push(Scope::default());
                    let mut items = self.block_items()?;
                    self.scopes.pop();
                    self.expect(")")?;
                    let last = match items.last() {
                        Some(Stmt::Expr(_)) => match items.pop() {
                            Some(Stmt::Expr(e)) => Some(e),
                            _ => None,
                        },
                        _ => None,
                    };
                    let ty = last.as_ref().map_or(Type::Void, |e| e.ty.clone());
                    return Ok(expr(ExprKind::Stmts(items, last.map(b)), ty));
                }
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Ident(name) => {
                let name = name.clone();
                self.next();
                match &*name {
                    "__syscall" => {
                        self.expect("(")?;
                        let mut args = Vec::new();
                        while !self.eat(")") {
                            if !args.is_empty() {
                                self.expect(",")?;
                            }
                            let a = self.assign()?;
                            if a.ty.is_record() || a.ty.is_float() {
                                return Err(self.err("invalid __syscall argument"));
                            }
                            let ty = a.ty.decay();
                            args.push(cast(a, &ty));
                        }
                        if args.is_empty() || args.len() > 7 {
                            return Err(self.err("__syscall takes 1 to 7 arguments"));
                        }
                        return Ok(expr(ExprKind::Syscall(args), LONG));
                    }
                    "__builtin_sqrt" => {
                        self.expect("(")?;
                        let a = self.assign()?;
                        self.expect(")")?;
                        return Ok(expr(
                            ExprKind::Sqrt(b(cast(a, &Type::Double))),
                            Type::Double,
                        ));
                    }
                    "__builtin_offsetof" => {
                        self.expect("(")?;
                        let ty = self.typename()?;
                        self.expect(",")?;
                        let m = self.ident()?;
                        self.expect(")")?;
                        let (_, off) = ty
                            .member(&m)
                            .ok_or_else(|| self.err(format!("no member named '{}'", m)))?;
                        return Ok(expr(ExprKind::Int(off as i64), ULONG));
                    }
                    "__builtin_expect" => {
                        self.expect("(")?;
                        let a = self.assign()?;
                        self.expect(",")?;
                        self.assign()?;
                        self.expect(")")?;
                        return Ok(a);
                    }
                    _ => {}
                }
                match self.lookup(&name).cloned() {
                    Some(Sym::Local(i, ty)) => Ok(expr(ExprKind::Local(i), ty)),
                    Some(Sym::Global(link, ty)) => Ok(expr(ExprKind::Global(link), ty)),
                    Some(Sym::Enum(v)) => Ok(int(v)),
                    Some(Sym::Typedef(_)) => {
                        Err(self.err_at(&t, format!("unexpected type name '{}'", name)))
                    }
                    None => {
                        if self.is("(") {
                            // Implicit declaration: int name().
                            let ft = Type::Func(Rc::new(FuncType {
                                ret: INT,
                                params: Vec::new(),
                                variadic: true,
                                prototype: false,
                            }));
                            self.scopes[0]
                                .syms
                                .insert(name.clone(), Sym::Global(name.clone(), ft.clone()));
                            return Ok(expr(ExprKind::Global(name), ft));
                        }
                        Err(self.err_at(&t, format!("'{}' undeclared", name)))
                    }
                }
            }
            Tok::Eof => Err(self.err_at(&t, "expected expression at end of input")),
            _ => Err(self.err_at(&t, format!("expected expression before '{}'", t.spelling()))),
        }
    }
}

fn parse_hex_float(s: &str) -> Option<f64> {
    let s = s.strip_prefix("0x")?;
    let (mant, exp) = s.split_once('p')?;
    let exp: i32 = exp.parse().ok()?;
    let (int_part, frac) = mant.split_once('.').unwrap_or((mant, ""));
    let mut v = 0f64;
    for c in int_part.chars() {
        v = v * 16.0 + c.to_digit(16)? as f64;
    }
    let mut scale = 1.0 / 16.0;
    for c in frac.chars() {
        v += c.to_digit(16)? as f64 * scale;
        scale /= 16.0;
    }
    let mut r = v;
    if exp >= 0 {
        for _ in 0..exp {
            r *= 2.0;
        }
    } else {
        for _ in 0..-exp {
            r /= 2.0;
        }
    }
    Some(r)
}
