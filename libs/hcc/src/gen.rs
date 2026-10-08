//! Code generation: a simple stack machine over the typed AST.
//!
//! Every expression leaves its value in rax: integers and pointers
//! extended to 64 bits according to their type, `float`/`double` as the
//! bit pattern of a double, aggregates as their address. Binary operators
//! push the right operand and pop it into rdi.
//!
//! Calling convention (internal; the whole program is compiled by hcc):
//! arguments are pushed right to left, each in an 8-byte-aligned slot
//! (`float` as 4 bytes, structs by value), and the caller pops them. A
//! function returning a struct gets a hidden pointer to the result
//! pushed last. The return value is in rax. This keeps `va_list` a plain
//! pointer walking the argument slots.

use crate::asm::Cond as Cc;
use crate::asm::*;
use crate::ast::*;
use crate::ty::{align_to, Type};
use crate::Error;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

pub const BASE_ADDR: u64 = 0x400000;
pub const TEXT_ADDR: u64 = 0x401000;

pub struct Image {
    pub text: Vec<u8>,
    pub data: Vec<u8>,
    /// Size of the data segment in memory (data plus bss).
    pub data_mem: usize,
    pub data_addr: u64,
    pub entry: u64,
}

struct Gen<'p> {
    a: Asm,
    names: BTreeMap<Rc<str>, usize>,
    offsets: Vec<i32>,
    locals: &'p [Local],
    ret_label: usize,
    struct_ret: bool,
}

fn err(msg: String) -> Error {
    Error {
        file: String::new(),
        line: 0,
        message: msg,
    }
}

fn slot(ty: &Type) -> usize {
    align_to(ty.size(), 8)
}

// ---- reachability -----------------------------------------------------

fn refs_expr(e: &Expr, out: &mut Vec<Rc<str>>) {
    use ExprKind::*;
    match &e.kind {
        Global(n) => out.push(n.clone()),
        Int(_) | Float(_) | Local(_) | Zero(_) => {}
        Binary(_, l, r) | LogAnd(l, r) | LogOr(l, r) | Comma(l, r) | Assign(l, r) => {
            refs_expr(l, out);
            refs_expr(r, out);
        }
        Neg(x) | Not(x) | BitNot(x) | Deref(x) | Addr(x) | Member(x, _) | Cast(x) | Sqrt(x) => {
            refs_expr(x, out)
        }
        Cond(c, a, b) => {
            refs_expr(c, out);
            refs_expr(a, out);
            refs_expr(b, out);
        }
        Call { func, args, .. } => {
            refs_expr(func, out);
            args.iter().for_each(|a| refs_expr(a, out));
        }
        Syscall(args) => args.iter().for_each(|a| refs_expr(a, out)),
        Stmts(s, last) => {
            s.iter().for_each(|s| refs_stmt(s, out));
            if let Some(l) = last {
                refs_expr(l, out);
            }
        }
    }
}

fn refs_stmt(s: &Stmt, out: &mut Vec<Rc<str>>) {
    match s {
        Stmt::Expr(e) => refs_expr(e, out),
        Stmt::Block(v) => v.iter().for_each(|s| refs_stmt(s, out)),
        Stmt::If(c, t, e) => {
            refs_expr(c, out);
            refs_stmt(t, out);
            if let Some(e) = e {
                refs_stmt(e, out);
            }
        }
        Stmt::Loop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            if let Some(i) = init {
                refs_stmt(i, out);
            }
            if let Some(c) = cond {
                refs_expr(c, out);
            }
            if let Some(s) = step {
                refs_expr(s, out);
            }
            refs_stmt(body, out);
        }
        Stmt::Switch { cond, body, .. } => {
            refs_expr(cond, out);
            refs_stmt(body, out);
        }
        Stmt::Return(Some(e)) => refs_expr(e, out),
        Stmt::Return(None) | Stmt::Label(_) | Stmt::Goto(_) => {}
    }
}

pub fn generate(p: &Program) -> Result<Image, Error> {
    let mut functions: BTreeMap<Rc<str>, &Function> = BTreeMap::new();
    for f in &p.functions {
        if functions.insert(f.name.clone(), f).is_some() {
            return Err(err(format!("multiple definition of '{}'", f.name)));
        }
    }
    let globals: BTreeMap<Rc<str>, &Global> =
        p.globals.iter().map(|g| (g.name.clone(), g)).collect();

    // Keep only what is reachable from the entry point.
    let start = if functions.contains_key("__libc_start") {
        "__libc_start"
    } else {
        "main"
    };
    let mut reachable: BTreeSet<Rc<str>> = BTreeSet::new();
    let mut work: Vec<Rc<str>> = alloc::vec![Rc::from(start)];
    let mut referenced_from: BTreeMap<Rc<str>, Rc<str>> = BTreeMap::new();
    while let Some(name) = work.pop() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        let mut refs = Vec::new();
        if let Some(deps) = builtin(&name) {
            refs.extend(deps.iter().map(|d| Rc::from(*d)));
        } else if let Some(f) = functions.get(&name) {
            f.body.iter().for_each(|s| refs_stmt(s, &mut refs));
        } else if let Some(g) = globals.get(&name) {
            refs.extend(g.relocs.iter().map(|r| r.target.clone()));
        } else {
            let user = referenced_from.get(&name).map_or(String::new(), |u| {
                format!("in function '{}': ", u.split('.').next().unwrap_or(u))
            });
            return Err(err(format!("{}undefined reference to '{}'", user, name)));
        }
        for r in refs {
            if !reachable.contains(&r) {
                referenced_from
                    .entry(r.clone())
                    .or_insert_with(|| name.clone());
                work.push(r);
            }
        }
    }

    let mut g = Gen {
        a: Asm::new(p.labels),
        names: BTreeMap::new(),
        offsets: Vec::new(),
        locals: &[],
        ret_label: 0,
        struct_ret: false,
    };

    // _start: pass the initial stack pointer (argc, argv, envp, auxv).
    let entry = g.a.pos();
    g.a.mov(RAX, RSP);
    g.a.push(RAX);
    let l = g.label(start);
    g.a.call(l);
    g.a.mov(RDI, RAX);
    g.a.mov_imm(RAX, 231); // exit_group
    g.a.syscall();
    g.a.hlt();

    for f in p.functions.iter().filter(|f| reachable.contains(&f.name)) {
        g.function(f)?;
    }
    for name in &reachable {
        if builtin(name).is_some() && !functions.contains_key(name) {
            g.builtin(name);
        }
    }

    // Data and bss.
    let mut data: Vec<u8> = Vec::new();
    let mut bss = 0usize;
    let mut relocs: Vec<(usize, usize, i64)> = Vec::new();
    for gl in p
        .globals
        .iter()
        .filter(|g| reachable.contains(&g.name) && !functions.contains_key(&g.name))
    {
        let label = g.label(&gl.name);
        if g.a.labels[label].is_some() {
            continue;
        }
        let align = gl.ty.align().max(1);
        match &gl.data {
            Some(bytes) => {
                let off = align_to(data.len(), align);
                data.resize(off, 0);
                data.extend_from_slice(bytes);
                g.a.labels[label] = Some((Section::Data, off));
                for r in &gl.relocs {
                    let target = g.label(&r.target);
                    relocs.push((off + r.offset, target, r.addend));
                }
            }
            None => {
                let off = align_to(bss, align);
                bss = off + gl.ty.size().max(1);
                g.a.labels[label] = Some((Section::Bss, off));
            }
        }
    }

    let text_end = TEXT_ADDR + g.a.code.len() as u64;
    let data_addr = align_to(text_end as usize, 0x1000) as u64;
    let bss_addr = data_addr + align_to(data.len(), 16) as u64;
    let names: BTreeMap<usize, Rc<str>> = g.names.iter().map(|(k, v)| (*v, k.clone())).collect();
    let addr = |labels: &[Option<(Section, usize)>], l: usize| -> Result<u64, Error> {
        match labels[l] {
            Some((Section::Text, o)) => Ok(TEXT_ADDR + o as u64),
            Some((Section::Data, o)) => Ok(data_addr + o as u64),
            Some((Section::Bss, o)) => Ok(bss_addr + o as u64),
            None => Err(err(format!(
                "undefined reference to '{}'",
                names.get(&l).map_or("<label>", |n| n)
            ))),
        }
    };
    for f in &g.a.fixups {
        let target = addr(&g.a.labels, f.label)? as i64 + f.addend;
        let from = (TEXT_ADDR + f.at as u64 + 4) as i64;
        let rel = target - from;
        g.a.code[f.at..f.at + 4].copy_from_slice(&(rel as i32).to_le_bytes());
    }
    for (off, target, addend) in relocs {
        let v = addr(&g.a.labels, target)? as i64 + addend;
        data[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }
    let data_mem = (bss_addr - data_addr) as usize + bss;
    Ok(Image {
        text: g.a.code,
        data,
        data_mem,
        data_addr,
        entry: TEXT_ADDR + entry as u64,
    })
}

/// Functions that cannot be written in C, built into the compiler; the
/// value lists what they call.
fn builtin(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "setjmp" | "longjmp" | "__restore_rt" => Some(&[]),
        "__hcc_sigtramp" => Some(&["__sig_dispatch"]),
        _ => None,
    }
}

impl<'p> Gen<'p> {
    fn builtin(&mut self, name: &str) {
        let l = self.label(name);
        self.a.bind(l);
        match name {
            // jmp_buf: return address, rbp, rsp after return.
            "setjmp" => {
                self.a.load(RDI, RSP, 8, 8, false);
                self.a.load(RAX, RSP, 0, 8, false);
                self.a.store(RAX, RDI, 0, 8);
                self.a.store(RBP, RDI, 8, 8);
                self.a.lea(RAX, RSP, 8);
                self.a.store(RAX, RDI, 16, 8);
                self.a.mov_imm(RAX, 0);
                self.a.ret();
            }
            "longjmp" => {
                self.a.load(RDI, RSP, 8, 8, false);
                self.a.load(RAX, RSP, 16, 4, true);
                let nonzero = self.a.new_label();
                self.a.test(RAX, RAX);
                self.a.jcc(Cc::Ne, nonzero);
                self.a.mov_imm(RAX, 1);
                self.a.bind(nonzero);
                self.a.load(RBP, RDI, 8, 8, false);
                self.a.load(RCX, RDI, 0, 8, false);
                self.a.load(RSP, RDI, 16, 8, false);
                self.a.jmp_reg(RCX);
            }
            // Signal return trampoline (sa_restorer).
            "__restore_rt" => {
                self.a.mov_imm(RAX, 15);
                self.a.syscall();
                self.a.hlt();
            }
            // Signal handler entry: the kernel passes the signal in rdi.
            _ => {
                self.a.push(RDI);
                let d = self.label("__sig_dispatch");
                self.a.call(d);
                self.a.add_imm(RSP, 8);
                self.a.ret();
            }
        }
    }

    fn label(&mut self, name: &str) -> usize {
        if let Some(&l) = self.names.get(name) {
            return l;
        }
        let l = self.a.new_label();
        self.names.insert(Rc::from(name), l);
        l
    }

    fn function(&mut self, f: &'p Function) -> Result<(), Error> {
        let l = self.label(&f.name);
        self.a.bind(l);
        self.a.push(RBP);
        self.a.mov(RBP, RSP);
        let patch = self.a.sub_rsp_patchable();
        self.locals = &f.locals;
        self.struct_ret = f.ty.ret.is_record();
        self.offsets = alloc::vec![0; f.locals.len()];
        let mut param_off = if self.struct_ret { 24 } else { 16 };
        for &p in &f.params {
            self.offsets[p] = param_off as i32;
            param_off += slot(&f.locals[p].ty);
        }
        let mut frame = 0usize;
        for (i, local) in f.locals.iter().enumerate() {
            if f.params.contains(&i) {
                continue;
            }
            frame = align_to(frame + local.ty.size(), local.ty.align());
            self.offsets[i] = -(frame as i32);
        }
        self.a.patch32(patch, align_to(frame, 16) as i32);
        self.ret_label = self.a.new_label();
        for s in &f.body {
            self.stmt(s)?;
        }
        self.a.mov_imm(RAX, 0);
        self.a.bind(self.ret_label);
        self.a.leave();
        self.a.ret();
        Ok(())
    }

    // ---- statements ----------------------------------------------------

    fn stmt(&mut self, s: &Stmt) -> Result<(), Error> {
        match s {
            Stmt::Expr(e) => self.expr(e)?,
            Stmt::Block(v) => {
                for s in v {
                    self.stmt(s)?;
                }
            }
            Stmt::If(c, t, e) => {
                let els = self.a.new_label();
                let end = self.a.new_label();
                self.jump_if_false(c, els)?;
                self.stmt(t)?;
                self.a.jmp(end);
                self.a.bind(els);
                if let Some(e) = e {
                    self.stmt(e)?;
                }
                self.a.bind(end);
            }
            Stmt::Loop {
                init,
                cond,
                step,
                body,
                brk,
                cont,
                post_test,
            } => {
                if let Some(i) = init {
                    self.stmt(i)?;
                }
                let top = self.a.new_label();
                self.a.bind(top);
                if *post_test {
                    self.stmt(body)?;
                    self.a.bind(*cont);
                    if let Some(c) = cond {
                        self.expr(c)?;
                        self.test_zero(&c.ty);
                        self.a.jcc(Cc::Ne, top);
                    } else {
                        self.a.jmp(top);
                    }
                } else {
                    if let Some(c) = cond {
                        self.jump_if_false(c, *brk)?;
                    }
                    self.stmt(body)?;
                    self.a.bind(*cont);
                    if let Some(s) = step {
                        self.expr(s)?;
                    }
                    self.a.jmp(top);
                }
                self.a.bind(*brk);
            }
            Stmt::Switch {
                cond,
                body,
                cases,
                default,
                brk,
            } => {
                self.expr(cond)?;
                for &(lo, hi, label) in cases {
                    if lo == hi {
                        self.cmp_const(lo);
                        self.a.jcc(Cc::E, label);
                    } else {
                        let next = self.a.new_label();
                        self.cmp_const(lo);
                        self.a.jcc(Cc::L, next);
                        self.cmp_const(hi);
                        self.a.jcc(Cc::Le, label);
                        self.a.bind(next);
                    }
                }
                self.a.jmp(default.unwrap_or(*brk));
                self.stmt(body)?;
                self.a.bind(*brk);
            }
            Stmt::Label(l) => self.a.bind(*l),
            Stmt::Goto(l) => self.a.jmp(*l),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e)?;
                    if self.struct_ret {
                        self.a.mov(RSI, RAX);
                        self.a.load(RDI, RBP, 16, 8, false);
                        self.a.mov(RAX, RDI);
                        self.a.mov_imm(RCX, e.ty.size() as i64);
                        self.a.rep_movsb();
                    }
                }
                self.a.jmp(self.ret_label);
            }
        }
        Ok(())
    }

    fn cmp_const(&mut self, v: i64) {
        if v >= i32::MIN as i64 && v <= i32::MAX as i64 {
            self.a.cmp_imm(RAX, v as i32);
        } else {
            self.a.mov_imm(RDI, v);
            self.a.cmp(RAX, RDI);
        }
    }

    /// Sets ZF if the value in rax (of type `ty`) is zero.
    fn test_zero(&mut self, ty: &Type) {
        if ty.is_float() {
            self.a.movq_to_xmm(0, RAX);
            self.a.xorpd(1, 1);
            self.a.ucomisd(0, 1);
        } else {
            self.a.test(RAX, RAX);
        }
    }

    fn jump_if_false(&mut self, c: &Expr, label: usize) -> Result<(), Error> {
        self.expr(c)?;
        self.test_zero(&c.ty);
        self.a.jcc(Cc::E, label);
        Ok(())
    }

    // ---- expressions ---------------------------------------------------

    /// Extends the value in rax to the canonical form of `ty`.
    fn normalize(&mut self, ty: &Type) {
        if let Type::Int { size, signed } = ty {
            self.a.extend(RAX, *size as usize, *signed);
        }
    }

    fn round_float(&mut self) {
        self.a.movq_to_xmm(0, RAX);
        self.a.cvtsd2ss(0, 0);
        self.a.cvtss2sd(0, 0);
        self.a.movq_from_xmm(RAX, 0);
    }

    /// rax = *(ty *)rax
    fn load(&mut self, ty: &Type) {
        match ty {
            Type::Array(..) | Type::Record(_) | Type::Func(_) | Type::Void => {}
            Type::Bool => self.a.load(RAX, RAX, 0, 1, false),
            Type::Int { size, signed } => self.a.load(RAX, RAX, 0, *size as usize, *signed),
            Type::Float => {
                self.a.load(RAX, RAX, 0, 4, false);
                self.a.movd_to_xmm(0, RAX);
                self.a.cvtss2sd(0, 0);
                self.a.movq_from_xmm(RAX, 0);
            }
            Type::Double | Type::Ptr(_) => self.a.load(RAX, RAX, 0, 8, false),
        }
    }

    /// *(ty *)rdi = rax
    fn store(&mut self, ty: &Type) {
        match ty {
            Type::Record(_) => {
                self.a.mov(RSI, RAX);
                self.a.mov(RAX, RDI);
                self.a.mov_imm(RCX, ty.size() as i64);
                self.a.rep_movsb();
            }
            Type::Float => {
                self.a.movq_to_xmm(0, RAX);
                self.a.cvtsd2ss(0, 0);
                self.a.movd_from_xmm(RDX, 0);
                self.a.store(RDX, RDI, 0, 4);
            }
            Type::Bool => self.a.store(RAX, RDI, 0, 1),
            _ => self.a.store(RAX, RDI, 0, ty.size()),
        }
    }

    fn addr(&mut self, e: &Expr) -> Result<(), Error> {
        match &e.kind {
            ExprKind::Local(i) => self.a.lea(RAX, RBP, self.offsets[*i]),
            ExprKind::Global(name) => {
                let l = self.label(name);
                self.a.lea_label(RAX, l, 0);
            }
            ExprKind::Deref(x) => self.expr(x)?,
            ExprKind::Member(x, off) => {
                self.addr(x)?;
                self.a.add_imm(RAX, *off as i32);
            }
            ExprKind::Comma(a, b) => {
                self.expr(a)?;
                self.addr(b)?;
            }
            // Aggregates are evaluated to their address.
            _ if e.ty.is_record() || e.ty.is_func() || matches!(e.ty, Type::Array(..)) => {
                self.expr(e)?
            }
            _ => return Err(err(String::from("not an lvalue"))),
        }
        Ok(())
    }

    fn cast(&mut self, from: &Type, to: &Type) {
        if to.is_void() || from.is_record() || to.is_record() {
            return;
        }
        if matches!(to, Type::Bool) {
            if from.is_float() {
                self.test_zero(from);
                self.a.setcc(Cc::Ne, RAX);
                self.a.setcc(Cc::P, RDX);
                self.a.or8(RAX, RDX);
            } else {
                self.a.test(RAX, RAX);
                self.a.setcc(Cc::Ne, RAX);
            }
            self.a.extend(RAX, 1, false);
            return;
        }
        match (from.is_float(), to.is_float()) {
            (false, false) => self.normalize(to),
            (false, true) => {
                if from.size() == 8 && from.is_unsigned() {
                    // Unsigned 64-bit: halve (keeping the low bit) when the
                    // top bit is set, convert, double.
                    let big = self.a.new_label();
                    let done = self.a.new_label();
                    self.a.test(RAX, RAX);
                    self.a.jcc(Cc::L, big);
                    self.a.cvtsi2sd(0, RAX);
                    self.a.jmp(done);
                    self.a.bind(big);
                    self.a.mov(RDI, RAX);
                    self.a.mov_imm(RDX, 1);
                    self.a.and(RDI, RDX);
                    self.a.mov_imm(RCX, 1);
                    self.a.shr_cl(RAX);
                    self.a.or(RAX, RDI);
                    self.a.cvtsi2sd(0, RAX);
                    self.a.addsd(0, 0);
                    self.a.bind(done);
                } else {
                    self.a.cvtsi2sd(0, RAX);
                }
                self.a.movq_from_xmm(RAX, 0);
                if matches!(to, Type::Float) {
                    self.round_float();
                }
            }
            (true, false) => {
                self.a.movq_to_xmm(0, RAX);
                self.a.cvttsd2si(RAX, 0);
                self.normalize(to);
            }
            (true, true) => {
                if matches!(to, Type::Float) && matches!(from, Type::Double) {
                    self.round_float();
                }
            }
        }
    }

    fn expr(&mut self, e: &Expr) -> Result<(), Error> {
        use ExprKind::*;
        match &e.kind {
            Int(v) => self.a.mov_imm(RAX, *v),
            Float(f) => self.a.mov_imm(RAX, f.to_bits() as i64),
            Local(_) | Global(_) | Member(..) => {
                self.addr(e)?;
                self.load(&e.ty);
            }
            Deref(x) => {
                self.expr(x)?;
                self.load(&e.ty);
            }
            Addr(x) => self.addr(x)?,
            Assign(l, r) => {
                // Variables need no saved address: this also keeps the
                // stack clean across `x = setjmp(...)`.
                match &l.kind {
                    Local(i) => {
                        self.expr(r)?;
                        self.a.lea(RDI, RBP, self.offsets[*i]);
                    }
                    Global(name) => {
                        self.expr(r)?;
                        let label = self.label(name);
                        self.a.lea_label(RDI, label, 0);
                    }
                    _ => {
                        self.addr(l)?;
                        self.a.push(RAX);
                        self.expr(r)?;
                        self.a.pop(RDI);
                    }
                }
                self.store(&l.ty);
                if matches!(l.ty, Type::Float) {
                    self.round_float();
                }
            }
            Cast(x) => {
                self.expr(x)?;
                self.cast(&x.ty, &e.ty);
            }
            Comma(a, b) => {
                self.expr(a)?;
                self.expr(b)?;
            }
            Cond(c, a, b) => {
                let els = self.a.new_label();
                let end = self.a.new_label();
                self.jump_if_false(c, els)?;
                self.expr(a)?;
                self.a.jmp(end);
                self.a.bind(els);
                self.expr(b)?;
                self.a.bind(end);
            }
            LogAnd(l, r) => {
                let f = self.a.new_label();
                let end = self.a.new_label();
                self.jump_if_false(l, f)?;
                self.jump_if_false(r, f)?;
                self.a.mov_imm(RAX, 1);
                self.a.jmp(end);
                self.a.bind(f);
                self.a.mov_imm(RAX, 0);
                self.a.bind(end);
            }
            LogOr(l, r) => {
                let t = self.a.new_label();
                let end = self.a.new_label();
                self.expr(l)?;
                self.test_zero(&l.ty);
                self.a.jcc(Cc::Ne, t);
                self.expr(r)?;
                self.test_zero(&r.ty);
                self.a.jcc(Cc::Ne, t);
                self.a.mov_imm(RAX, 0);
                self.a.jmp(end);
                self.a.bind(t);
                self.a.mov_imm(RAX, 1);
                self.a.bind(end);
            }
            Not(x) => {
                self.expr(x)?;
                self.test_zero(&x.ty);
                self.a.setcc(Cc::E, RAX);
                self.a.extend(RAX, 1, false);
            }
            Neg(x) => {
                self.expr(x)?;
                if e.ty.is_float() {
                    self.a.flip_sign(RAX);
                } else {
                    self.a.neg(RAX);
                    self.normalize(&e.ty);
                }
            }
            BitNot(x) => {
                self.expr(x)?;
                self.a.not(RAX);
                self.normalize(&e.ty);
            }
            Binary(op, l, r) => {
                self.expr(r)?;
                self.a.push(RAX);
                self.expr(l)?;
                self.a.pop(RDI);
                if l.ty.is_float() {
                    self.float_op(*op, &e.ty);
                } else {
                    self.int_op(*op, &l.ty, &e.ty);
                }
            }
            Call { func, args, ret } => self.call(func, args, *ret)?,
            Syscall(args) => {
                for a in args.iter().rev() {
                    self.expr(a)?;
                    self.a.push(RAX);
                }
                for &r in [RAX, RDI, RSI, RDX, R10, R8, R9].iter().take(args.len()) {
                    self.a.pop(r);
                }
                self.a.syscall();
            }
            Sqrt(x) => {
                self.expr(x)?;
                self.a.movq_to_xmm(0, RAX);
                self.a.sqrtsd(0, 0);
                self.a.movq_from_xmm(RAX, 0);
            }
            Zero(i) => {
                let size = self.locals[*i].ty.size();
                self.a.lea(RDI, RBP, self.offsets[*i]);
                self.a.mov_imm(RAX, 0);
                self.a.mov_imm(RCX, size as i64);
                self.a.rep_stosb();
            }
            Stmts(items, last) => {
                for s in items {
                    self.stmt(s)?;
                }
                if let Some(l) = last {
                    self.expr(l)?;
                }
            }
        }
        Ok(())
    }

    fn float_op(&mut self, op: BinOp, ty: &Type) {
        self.a.movq_to_xmm(0, RAX);
        self.a.movq_to_xmm(1, RDI);
        match op {
            BinOp::Add => self.a.addsd(0, 1),
            BinOp::Sub => self.a.subsd(0, 1),
            BinOp::Mul => self.a.mulsd(0, 1),
            BinOp::Div => self.a.divsd(0, 1),
            BinOp::Eq | BinOp::Ne => {
                self.a.ucomisd(0, 1);
                if op == BinOp::Eq {
                    self.a.setcc(Cc::E, RAX);
                    self.a.setcc(Cc::Np, RDX);
                    self.a.and8(RAX, RDX);
                } else {
                    self.a.setcc(Cc::Ne, RAX);
                    self.a.setcc(Cc::P, RDX);
                    self.a.or8(RAX, RDX);
                }
                self.a.extend(RAX, 1, false);
                return;
            }
            BinOp::Lt | BinOp::Le => {
                self.a.ucomisd(1, 0);
                self.a
                    .setcc(if op == BinOp::Lt { Cc::A } else { Cc::Ae }, RAX);
                self.a.extend(RAX, 1, false);
                return;
            }
            _ => {}
        }
        self.a.movq_from_xmm(RAX, 0);
        if matches!(ty, Type::Float) {
            self.round_float();
        }
    }

    fn int_op(&mut self, op: BinOp, operand: &Type, ty: &Type) {
        let unsigned = ty.is_unsigned();
        match op {
            BinOp::Add => self.a.add(RAX, RDI),
            BinOp::Sub => self.a.sub(RAX, RDI),
            BinOp::Mul => self.a.imul(RAX, RDI),
            BinOp::And => self.a.and(RAX, RDI),
            BinOp::Or => self.a.or(RAX, RDI),
            BinOp::Xor => self.a.xor(RAX, RDI),
            BinOp::Div | BinOp::Mod => {
                if unsigned {
                    self.a.mov_imm(RDX, 0);
                    self.a.div(RDI);
                } else {
                    self.a.cqo();
                    self.a.idiv(RDI);
                }
                if op == BinOp::Mod {
                    self.a.mov(RAX, RDX);
                }
            }
            BinOp::Shl | BinOp::Shr => {
                self.a.mov(RCX, RDI);
                if op == BinOp::Shl {
                    self.a.shl_cl(RAX);
                } else if unsigned {
                    self.a.shr_cl(RAX);
                } else {
                    self.a.sar_cl(RAX);
                }
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le => {
                self.a.cmp(RAX, RDI);
                let u = operand.is_unsigned();
                let c = match op {
                    BinOp::Eq => Cc::E,
                    BinOp::Ne => Cc::Ne,
                    BinOp::Lt if u => Cc::B,
                    BinOp::Lt => Cc::L,
                    _ if u => Cc::Be,
                    _ => Cc::Le,
                };
                self.a.setcc(c, RAX);
                self.a.extend(RAX, 1, false);
                return;
            }
        }
        self.normalize(ty);
    }

    fn call(&mut self, func: &Expr, args: &[Expr], ret: Option<usize>) -> Result<(), Error> {
        let mut total = 0usize;
        for a in args.iter().rev() {
            self.expr(a)?;
            if a.ty.is_record() {
                let size = a.ty.size();
                let s = slot(&a.ty);
                self.a.sub_imm(RSP, s as i32);
                self.a.mov(RSI, RAX);
                self.a.mov(RDI, RSP);
                self.a.mov_imm(RCX, size as i64);
                self.a.rep_movsb();
                total += s;
            } else {
                if matches!(a.ty, Type::Float) {
                    self.a.movq_to_xmm(0, RAX);
                    self.a.cvtsd2ss(0, 0);
                    self.a.movd_from_xmm(RAX, 0);
                }
                self.a.push(RAX);
                total += 8;
            }
        }
        if let Some(r) = ret {
            self.a.lea(RAX, RBP, self.offsets[r]);
            self.a.push(RAX);
            total += 8;
        }
        match &func.kind {
            ExprKind::Global(name) if func.ty.is_func() => {
                let l = self.label(name);
                self.a.call(l);
            }
            _ => {
                self.expr(func)?;
                self.a.call_reg(RAX);
            }
        }
        self.a.add_imm(RSP, total as i32);
        Ok(())
    }
}
