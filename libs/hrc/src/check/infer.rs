//! Per-function type inference: the variable table, unification,
//! normalization of associated types, trait selection and obligations.

use super::lower::{NoInfer, Scope, VarSource};
use super::{Ctx, Pred, TraitRef};
use crate::ast::{self, Name};
use crate::resolve::{DefId, DefKind, ModId};
use crate::tir::{self, LocalId};
use crate::ty::{IntTy, Ty};
use crate::{err, Error, Result, Span};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VarKind {
    General,
    Int,
    Float,
}

#[derive(Clone, Debug)]
enum Var {
    Unbound(VarKind),
    Bound(Ty),
}

/// The result of matching against an impl without committing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum M {
    Yes,
    Maybe,
    No,
}

impl M {
    pub fn and(self, o: M) -> M {
        match (self, o) {
            (M::No, _) | (_, M::No) => M::No,
            (M::Maybe, _) | (_, M::Maybe) => M::Maybe,
            _ => M::Yes,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Selection {
    Impl(DefId, Rc<[Ty]>),
    Builtin,
    /// The type is a trait object of this trait (or a subtrait).
    Dyn,
    Ambiguous,
    None,
}

#[derive(Clone, Debug)]
pub enum Obligation {
    Pred(Pred),
    Eq(Ty, Ty),
}

pub struct LoopCtx {
    pub label: Option<Name>,
    pub id: u32,
    /// The type of `break` values (`loop` and labeled blocks).
    pub break_ty: Option<Ty>,
    /// A labeled block: `continue` and unlabeled `break` skip it.
    pub is_block: bool,
    pub has_break: bool,
}

pub struct ClosureCtx {
    pub id: u32,
    /// Captured locals and whether by reference.
    pub captures: Vec<(LocalId, bool)>,
    pub is_move: bool,
    pub saved_ret: Ty,
    pub saved_loops: Vec<LoopCtx>,
}

pub struct FnCtx<'c, 'a> {
    pub cx: &'c mut Ctx<'a>,
    pub def: DefId,
    pub substs: Rc<[Ty]>,
    pub scope: Scope,
    pub module: ModId,
    vars: Vec<Var>,
    pub locals: Vec<tir::Local>,
    /// Closure nesting depth at which each local was declared.
    pub local_depth: Vec<usize>,
    pub scopes: Vec<Vec<(Name, LocalId)>>,
    pub ret_ty: Ty,
    pub loops: Vec<LoopCtx>,
    pub obligations: Vec<(Obligation, Span)>,
    pub closures: Vec<ClosureCtx>,
    pub closure_sigs: BTreeMap<u32, (Vec<Ty>, Ty)>,
    pub closure_bodies: Vec<(u32, tir::Closure)>,
    pub checked: Vec<Option<tir::Expr>>,
    pub track_caller: bool,
    pub inst: tir::InstId,
    /// Variables known to become a `dyn Fn` (closure signature hints).
    pub closure_hints: Vec<(Ty, Ty)>,
    /// Normalized projections (normalizing selects an impl, which must
    /// happen once per projection).
    proj_cache: BTreeMap<Ty, Ty>,
    /// An error found while revealing an `impl Trait` type.
    pub deferred_error: Option<Error>,
}

impl VarSource for Vec<Var> {
    fn fresh(&mut self) -> Option<Ty> {
        self.push(Var::Unbound(VarKind::General));
        Some(Ty::Infer((self.len() - 1) as u32))
    }
}

/// Type-checks one instance of a function or constant.
pub fn check_fn(cx: &mut Ctx, inst: tir::InstId, def: DefId, substs: Rc<[Ty]>) -> Result<(tir::Function, Ty)> {
    let d = cx.db.def(def).clone();
    let name = instance_name(cx, def, &substs);
    let sc = cx.item_scope(def)?;
    match &d.kind {
        DefKind::Const(t, e) | DefKind::AssocConst(t, Some(e)) => {
            let (t, e) = (t.clone(), e.clone());
            let mut f = FnCtx::new(cx, def, substs.clone(), sc, inst);
            let ty = f.lower_sig_ty(&t)?;
            f.ret_ty = ty.clone();
            let body = f.check_expr_coerce(&e, &ty)?;
            let body = f.finish(body)?;
            let ty = f.resolve(&ty);
            let locals = core::mem::take(&mut f.locals);
            f.finish_closures(inst)?;
            return Ok((
                tir::Function {
                    name,
                    params: Vec::new(),
                    locals,
                    body,
                    ret: ty.clone(),
                    track_caller: false,
                },
                ty,
            ));
        }
        DefKind::Fn(_) => {}
        _ => return err(d.span, format!("`{}` cannot be called", d.name)),
    }
    let f_ast = match &d.kind {
        DefKind::Fn(f) => f.clone(),
        _ => unreachable!(),
    };
    let body_ast = match &f_ast.body {
        Some(b) => b.clone(),
        None => return err(d.span, format!("function `{}` has no body", d.name)),
    };
    let sig = cx.sig(def)?;
    let mut f = FnCtx::new(cx, def, substs.clone(), sc, inst);
    f.track_caller = sig.track_caller;
    let mut params = Vec::new();
    let mut prologue = Vec::new();
    let mut ptys = Vec::new();
    for p in &sig.params {
        let t = p.subst(&substs);
        let t = f.resolve(&t);
        ptys.push(t);
    }
    let mut ret = sig.ret.subst(&substs);
    if let Ty::Opaque(od, _) = &ret {
        if *od == def {
            ret = f.fresh();
        }
    }
    let ret = f.resolve(&ret);
    f.ret_ty = ret.clone();
    f.scopes.push(Vec::new());
    let mut k = 0;
    if f_ast.self_param.is_some() {
        let id = f.new_local(Rc::from("self"), ptys[0].clone());
        f.bind_name(Rc::from("self"), id);
        params.push(id);
        k = 1;
    }
    for (i, p) in f_ast.params.iter().enumerate() {
        let t = ptys[k + i].clone();
        match &p.pat.kind {
            ast::PatKind::Ident { name, by_ref: false, sub: None, .. } if !f.is_unit_path_name(name) => {
                let id = f.new_local(name.clone(), t);
                f.bind_name(name.clone(), id);
                params.push(id);
            }
            _ => {
                let id = f.new_local(Rc::from("__arg"), t.clone());
                params.push(id);
                let (pat, binds) = f.check_pat_top(&p.pat, &t)?;
                for (n, l) in binds {
                    f.bind_name(n, l);
                }
                prologue.push(tir::Stmt::Let(
                    pat,
                    Some(tir::Expr {
                        kind: tir::ExprKind::Local(id),
                        ty: t,
                        span: p.pat.span,
                    }),
                    None,
                ));
            }
        }
    }
    let body = f.check_block_coerce(&body_ast, &ret)?;
    let body = if prologue.is_empty() {
        body
    } else {
        let ty = body.ty.clone();
        let span = body.span;
        tir::Expr {
            kind: tir::ExprKind::Block(prologue, alloc::boxed::Box::new(body)),
            ty,
            span,
        }
    };
    let body = f.finish(body)?;
    let ret = f.resolve(&f.ret_ty.clone());
    for l in 0..f.locals.len() {
        let t = f.locals[l].ty.clone();
        f.locals[l].ty = f.resolve(&t);
    }
    let locals = core::mem::take(&mut f.locals);
    let track_caller = f.track_caller;
    f.finish_closures(inst)?;
    Ok((
        tir::Function {
            name,
            params,
            locals,
            body,
            ret: ret.clone(),
            track_caller,
        },
        ret,
    ))
}

pub fn check_static(cx: &mut Ctx, def: DefId) -> Result<(Ty, tir::Expr, Vec<tir::Local>)> {
    let (t, e) = match &cx.db.def(def).kind {
        DefKind::Static(_, t, e) => (t.clone(), e.clone()),
        _ => unreachable!(),
    };
    let sc = cx.item_scope(def)?;
    let mut f = FnCtx::new(cx, def, Rc::from([]), sc, u32::MAX);
    let ty = f.lower_sig_ty(&t)?;
    f.ret_ty = ty.clone();
    let body = f.check_expr_coerce(&e, &ty)?;
    let body = f.finish(body)?;
    let locals = core::mem::take(&mut f.locals);
    if !f.closure_bodies.is_empty() {
        return err(e.span, String::from("closures are not supported in statics"));
    }
    Ok((ty, body, locals))
}

fn instance_name(cx: &mut Ctx, def: DefId, substs: &[Ty]) -> String {
    let d = cx.db.def(def);
    let mut name = String::new();
    if let Some(p) = d.parent {
        match &cx.db.def(p).kind {
            DefKind::Impl(_) => {
                if let Ok(h) = cx.impl_header(p) {
                    let st = h.self_ty.subst(substs);
                    name.push_str(&cx.ty_name(&st));
                    name.push_str("::");
                }
            }
            DefKind::Trait(_) => {
                if let Some(s) = substs.first() {
                    name.push_str(&cx.ty_name(s));
                    name.push_str("::");
                }
            }
            _ => {}
        }
    }
    name.push_str(&cx.db.def(def).name);
    name
}

impl<'c, 'a> FnCtx<'c, 'a> {
    pub fn new(cx: &'c mut Ctx<'a>, def: DefId, substs: Rc<[Ty]>, scope: Scope, inst: tir::InstId) -> FnCtx<'c, 'a> {
        let module = scope.module;
        FnCtx {
            cx,
            def,
            substs,
            scope,
            module,
            vars: Vec::new(),
            locals: Vec::new(),
            local_depth: Vec::new(),
            scopes: Vec::new(),
            ret_ty: crate::ty::unit(),
            loops: Vec::new(),
            obligations: Vec::new(),
            closures: Vec::new(),
            closure_sigs: BTreeMap::new(),
            closure_bodies: Vec::new(),
            checked: Vec::new(),
            track_caller: false,
            inst,
            closure_hints: Vec::new(),
            proj_cache: BTreeMap::new(),
            deferred_error: None,
        }
    }

    pub fn err<T>(&self, span: Span, msg: String) -> Result<T> {
        Err(Error::new(span, msg))
    }

    pub fn name(&self, t: &Ty) -> String {
        self.cx.ty_name(t)
    }

    // ---- variables ----

    pub fn fresh(&mut self) -> Ty {
        self.vars.push(Var::Unbound(VarKind::General));
        Ty::Infer((self.vars.len() - 1) as u32)
    }

    pub fn fresh_int(&mut self) -> Ty {
        self.vars.push(Var::Unbound(VarKind::Int));
        Ty::IntVar((self.vars.len() - 1) as u32)
    }

    pub fn fresh_float(&mut self) -> Ty {
        self.vars.push(Var::Unbound(VarKind::Float));
        Ty::FloatVar((self.vars.len() - 1) as u32)
    }

    pub fn fresh_substs(&mut self, n: usize) -> Vec<Ty> {
        (0..n).map(|_| self.fresh()).collect()
    }

    /// Follows bound variables (no normalization).
    pub fn peek(&self, t: &Ty) -> Ty {
        let mut t = t.clone();
        loop {
            match &t {
                Ty::Infer(v) | Ty::IntVar(v) | Ty::FloatVar(v) => match &self.vars[*v as usize] {
                    Var::Bound(b) => t = b.clone(),
                    Var::Unbound(_) => return t,
                },
                _ => return t,
            }
        }
    }

    /// Follows variables and normalizes a projection at the top.
    pub fn shallow(&mut self, t: &Ty) -> Ty {
        let mut t = self.peek(t);
        for _ in 0..32 {
            match &t {
                Ty::Proj(..) => match self.try_normalize(&t) {
                    Some(n) => t = self.peek(&n),
                    None => return t,
                },
                Ty::Opaque(d, s) => {
                    let s = self.resolve_list(s);
                    if s.iter().any(|x| x.needs_infer()) {
                        return Ty::Opaque(*d, s.into());
                    }
                    match self.reveal(*d, s.clone().into(), Span::default()) {
                        Ok(Some(h)) => t = h,
                        Ok(None) => return Ty::Opaque(*d, s.into()),
                        Err(e) => {
                            if self.deferred_error.is_none() {
                                self.deferred_error = Some(e);
                            }
                            return Ty::Opaque(*d, s.into());
                        }
                    }
                }
                _ => return t,
            }
        }
        t
    }

    /// Resolves all variables (and projections where possible).
    pub fn resolve(&mut self, t: &Ty) -> Ty {
        let t = self.shallow(t);
        let mut leaves = false;
        t.walk(&mut |x| {
            if matches!(x, Ty::Infer(_) | Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Proj(..) | Ty::Opaque(..)) {
                leaves = true;
            }
        });
        if !leaves {
            return t;
        }
        let list = |f: &mut Self, ts: &Rc<[Ty]>| -> Rc<[Ty]> { ts.iter().map(|x| f.resolve(x)).collect::<Vec<_>>().into() };
        match &t {
            Ty::Tuple(ts) => Ty::Tuple(list(self, ts)),
            Ty::Adt(d, ts) => Ty::Adt(*d, list(self, ts)),
            Ty::FnDef(d, ts) => Ty::FnDef(*d, list(self, ts)),
            Ty::Ref(m, x) => Ty::Ref(*m, Rc::new(self.resolve(x))),
            Ty::Ptr(m, x) => Ty::Ptr(*m, Rc::new(self.resolve(x))),
            Ty::Array(x, n) => Ty::Array(Rc::new(self.resolve(x)), Rc::new(self.resolve(n))),
            Ty::Slice(x) => Ty::Slice(Rc::new(self.resolve(x))),
            Ty::FnPtr(ps, r) => Ty::FnPtr(list(self, ps), Rc::new(self.resolve(r))),
            Ty::Dyn(d) => Ty::Dyn(Rc::new(crate::ty::DynTy {
                trait_: d.trait_,
                args: list(self, &d.args),
                bindings: d.bindings.iter().map(|(n, x)| (n.clone(), self.resolve(x))).collect::<Vec<_>>().into(),
            })),
            Ty::Proj(d, args, n) => {
                let p = Ty::Proj(*d, list(self, args), n.clone());
                match self.try_normalize(&p) {
                    Some(x) => self.resolve(&x),
                    None => p,
                }
            }
            Ty::Opaque(d, s) => {
                let o = Ty::Opaque(*d, list(self, s));
                let r = self.shallow(&o);
                if matches!(r, Ty::Opaque(..)) {
                    r
                } else {
                    self.resolve(&r)
                }
            }
            _ => t.clone(),
        }
    }

    pub fn resolve_list(&mut self, ts: &[Ty]) -> Vec<Ty> {
        ts.iter().map(|t| self.resolve(t)).collect()
    }

    fn occurs(&self, v: u32, t: &Ty) -> bool {
        let mut found = false;
        let t = self.peek(t);
        t.walk(&mut |x| {
            if let Ty::Infer(w) | Ty::IntVar(w) | Ty::FloatVar(w) = x {
                if *w == v {
                    found = true;
                } else if let Var::Bound(b) = &self.vars[*w as usize] {
                    if self.occurs(v, b) {
                        found = true;
                    }
                }
            }
        });
        found
    }

    fn bind(&mut self, v: u32, t: Ty) -> bool {
        if let Ty::Infer(w) | Ty::IntVar(w) | Ty::FloatVar(w) = t {
            if w == v {
                return true;
            }
        }
        if self.occurs(v, &t) {
            return false;
        }
        self.vars[v as usize] = Var::Bound(t);
        true
    }

    /// Makes two types equal.
    pub fn unify(&mut self, a: &Ty, b: &Ty) -> bool {
        let a = self.shallow(a);
        let b = self.shallow(b);
        if a == b {
            return true;
        }
        match (&a, &b) {
            (Ty::Error, _) | (_, Ty::Error) => true,
            // An associated type that cannot be normalized yet: equate
            // later (binding a variable to it could make a cycle).
            (Ty::Proj(..), _) | (_, Ty::Proj(..)) | (Ty::Opaque(..), _) | (_, Ty::Opaque(..)) => {
                self.obligations.push((Obligation::Eq(a.clone(), b.clone()), Span::default()));
                true
            }
            (Ty::Infer(v), _) => self.bind(*v, b.clone()),
            (_, Ty::Infer(v)) => self.bind(*v, a.clone()),
            (Ty::IntVar(v), Ty::IntVar(_)) | (Ty::IntVar(v), Ty::Int(_)) => self.bind(*v, b.clone()),
            (Ty::Int(_), Ty::IntVar(v)) => self.bind(*v, a.clone()),
            (Ty::FloatVar(v), Ty::FloatVar(_)) | (Ty::FloatVar(v), Ty::Float(_)) => self.bind(*v, b.clone()),
            (Ty::Float(_), Ty::FloatVar(v)) => self.bind(*v, a.clone()),
            (Ty::Never, _) | (_, Ty::Never) => true,
            (Ty::Tuple(x), Ty::Tuple(y)) => x.len() == y.len() && self.unify_lists(x, y),
            (Ty::Ref(m1, x), Ty::Ref(m2, y)) | (Ty::Ptr(m1, x), Ty::Ptr(m2, y)) => m1 == m2 && self.unify(x, y),
            (Ty::Array(x, n), Ty::Array(y, k)) => self.unify(x, y) && self.unify(n, k),
            (Ty::Slice(x), Ty::Slice(y)) => self.unify(x, y),
            (Ty::Adt(d1, x), Ty::Adt(d2, y)) | (Ty::FnDef(d1, x), Ty::FnDef(d2, y)) => d1 == d2 && self.unify_lists(x, y),
            (Ty::FnPtr(p1, r1), Ty::FnPtr(p2, r2)) => p1.len() == p2.len() && self.unify_lists(p1, p2) && self.unify(r1, r2),
            (Ty::Dyn(x), Ty::Dyn(y)) => {
                x.trait_ == y.trait_
                    && self.unify_lists(&x.args, &y.args)
                    && x.bindings.len() == y.bindings.len()
                    && x.bindings.iter().zip(y.bindings.iter()).all(|((n1, _), (n2, _))| n1 == n2)
                    && {
                        let pairs: Vec<(Ty, Ty)> = x.bindings.iter().zip(y.bindings.iter()).map(|(a, b)| (a.1.clone(), b.1.clone())).collect();
                        pairs.iter().all(|(a, b)| self.unify(a, b))
                    }
            }
            _ => false,
        }
    }

    fn unify_lists(&mut self, x: &[Ty], y: &[Ty]) -> bool {
        x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| self.unify(a, b))
    }

    pub fn expect_eq(&mut self, expected: &Ty, found: &Ty, span: Span) -> Result<()> {
        if self.unify(expected, found) {
            return Ok(());
        }
        let e = self.resolve(expected);
        let f = self.resolve(found);
        err(
            span,
            format!(
                "mismatched types: expected `{}`, found `{}`",
                self.name(&e),
                self.name(&f)
            ),
        )
    }

    /// Whether two types could be made equal (no side effects).
    pub fn compat(&self, a: &Ty, b: &Ty) -> M {
        let a = self.peek(a);
        let b = self.peek(b);
        if a == b {
            return M::Yes;
        }
        match (&a, &b) {
            (Ty::Error, _) | (_, Ty::Error) => M::Yes,
            (Ty::Infer(_), _) | (_, Ty::Infer(_)) => M::Maybe,
            (Ty::IntVar(_), Ty::Int(_) | Ty::IntVar(_)) | (Ty::Int(_), Ty::IntVar(_)) => M::Maybe,
            (Ty::FloatVar(_), Ty::Float(_) | Ty::FloatVar(_)) | (Ty::Float(_), Ty::FloatVar(_)) => M::Maybe,
            (Ty::IntVar(_) | Ty::FloatVar(_), _) | (_, Ty::IntVar(_) | Ty::FloatVar(_)) => M::No,
            (Ty::Proj(..) | Ty::Opaque(..) | Ty::Param(_), _) | (_, Ty::Proj(..) | Ty::Opaque(..) | Ty::Param(_)) => M::Maybe,
            (Ty::Tuple(x), Ty::Tuple(y)) if x.len() == y.len() => self.compat_lists(x, y),
            (Ty::Ref(m1, x), Ty::Ref(m2, y)) | (Ty::Ptr(m1, x), Ty::Ptr(m2, y)) if m1 == m2 => self.compat(x, y),
            (Ty::Array(x, n), Ty::Array(y, k)) => self.compat(x, y).and(self.compat(n, k)),
            (Ty::Slice(x), Ty::Slice(y)) => self.compat(x, y),
            (Ty::Adt(d1, x), Ty::Adt(d2, y)) | (Ty::FnDef(d1, x), Ty::FnDef(d2, y)) if d1 == d2 => self.compat_lists(x, y),
            (Ty::FnPtr(p1, r1), Ty::FnPtr(p2, r2)) if p1.len() == p2.len() => self.compat_lists(p1, p2).and(self.compat(r1, r2)),
            (Ty::Dyn(x), Ty::Dyn(y)) if x.trait_ == y.trait_ => self.compat_lists(&x.args, &y.args),
            _ => M::No,
        }
    }

    fn compat_lists(&self, x: &[Ty], y: &[Ty]) -> M {
        let mut m = M::Yes;
        for (a, b) in x.iter().zip(y.iter()) {
            m = m.and(self.compat(a, b));
            if m == M::No {
                break;
            }
        }
        m
    }

    // ---- lowering helpers ----

    /// Lowers a type in this function's scope and substitutes the instance
    /// arguments (`_` becomes a variable).
    pub fn lower_body_ty(&mut self, t: &ast::Type) -> Result<Ty> {
        let sc = self.body_scope();
        let ty = self.cx.lower_ty(&sc, t, &mut self.vars)?;
        Ok(ty.subst(&self.substs))
    }

    pub fn lower_sig_ty(&mut self, t: &ast::Type) -> Result<Ty> {
        let sc = self.body_scope();
        let ty = self.cx.lower_ty(&sc, t, &mut NoInfer)?;
        let ty = ty.subst(&self.substs);
        Ok(self.resolve(&ty))
    }

    pub fn body_scope(&self) -> Scope {
        let mut sc = self.scope.clone();
        sc.module = self.module;
        sc
    }

    pub fn vars_source(&mut self) -> &mut dyn VarSource {
        &mut self.vars
    }

    // ---- locals ----

    pub fn new_local(&mut self, name: Name, ty: Ty) -> LocalId {
        let owner = self.closures.last().map(|c| c.id + 1).unwrap_or(0);
        self.locals.push(tir::Local { name, ty, owner });
        self.local_depth.push(self.closures.len());
        (self.locals.len() - 1) as LocalId
    }

    pub fn bind_name(&mut self, name: Name, id: LocalId) {
        if let Some(s) = self.scopes.last_mut() {
            s.push((name, id));
        }
    }

    pub fn lookup_local(&self, name: &str) -> Option<LocalId> {
        for s in self.scopes.iter().rev() {
            for (n, id) in s.iter().rev() {
                if &**n == name {
                    return Some(*id);
                }
            }
        }
        None
    }

    // ---- normalization ----

    /// Normalizes `<T as Trait>::Name` when the impl is known.
    pub fn try_normalize(&mut self, t: &Ty) -> Option<Ty> {
        let (tr, args, name) = match t {
            Ty::Proj(tr, args, name) => (*tr, args.clone(), name.clone()),
            _ => return None,
        };
        let args: Rc<[Ty]> = args.iter().map(|a| self.peek_deep(a)).collect::<Vec<_>>().into();
        let key = Ty::Proj(tr, args.clone(), name.clone());
        if let Some(r) = self.proj_cache.get(&key) {
            return Some(r.clone());
        }
        let r = self.normalize_uncached(tr, args, name)?;
        self.proj_cache.insert(key, r.clone());
        Some(r)
    }

    /// Follows bound variables everywhere in a type (no normalization).
    pub fn peek_deep(&self, t: &Ty) -> Ty {
        let t = self.peek(t);
        t.map(&mut |x| match x {
            Ty::Infer(_) | Ty::IntVar(_) | Ty::FloatVar(_) => {
                let p = self.peek(x);
                if p == *x {
                    None
                } else {
                    Some(self.peek_deep(&p))
                }
            }
            _ => None,
        })
    }

    fn normalize_uncached(&mut self, tr: DefId, args: Rc<[Ty]>, name: Name) -> Option<Ty> {
        let self_ty = self.shallow(&args[0]);
        if self_ty.is_var() && !matches!(self_ty, Ty::IntVar(_) | Ty::FloatVar(_)) {
            return None;
        }
        // Closures and function types: `FnOnce::Output`.
        if self.cx.db.is_lang(tr, "FnOnce") && &*name == "Output" {
            if let Some((_, ret)) = self.callable_sig(&self_ty) {
                return Some(ret);
            }
        }
        if let Ty::Dyn(d) = &self_ty {
            if let Some((_, t)) = d.bindings.iter().find(|(n, _)| *n == name) {
                return Some(t.clone());
            }
        }
        let tref = TraitRef {
            trait_: tr,
            args: args[1..].to_vec().into(),
        };
        match self.select(&self_ty, &tref, Span::default()) {
            Ok(Selection::Impl(imp, isubsts)) => {
                if let Some(c) = self.cx.db.child(imp, &name) {
                    if let Ok(t) = self.cx.impl_assoc_ty(imp, c) {
                        return Some(t.subst(&isubsts));
                    }
                }
                self.trait_default_assoc(tr, &name, &args)
            }
            Ok(Selection::Builtin) => {
                // `Iterator::Item` etc. have no builtin impls; FnOnce handled above.
                None
            }
            _ => None,
        }
    }

    fn trait_default_assoc(&mut self, tr: DefId, name: &str, args: &[Ty]) -> Option<Ty> {
        let c = self.cx.db.child(tr, name)?;
        if let DefKind::AssocType(_, Some(d)) = &self.cx.db.def(c).kind {
            let d = d.clone();
            let sc = self.cx.item_scope(tr).ok()?;
            let t = self.cx.lower_ty(&sc, &d, &mut NoInfer).ok()?;
            return Some(t.subst(args));
        }
        None
    }

    /// The parameter and return types of something callable.
    pub fn callable_sig(&mut self, t: &Ty) -> Option<(Vec<Ty>, Ty)> {
        let t = self.shallow(t);
        match &t {
            Ty::Closure(id) => {
                if let Some(s) = self.closure_sigs.get(id) {
                    return Some(s.clone());
                }
                self.cx.closure_sig(*id)
            }
            Ty::FnPtr(ps, r) => Some((ps.to_vec(), (**r).clone())),
            Ty::FnDef(d, s) => {
                let d = *d;
                let s = s.clone();
                if let DefKind::Struct(_) | DefKind::Variant(_) = self.cx.db.def(d).kind {
                    return self.ctor_sig(d, &s);
                }
                let sig = self.cx.sig(d).ok()?;
                let ps: Vec<Ty> = sig.params.iter().map(|p| p.subst(&s)).collect();
                let r = sig.ret.subst(&s);
                Some((ps, r))
            }
            Ty::Ref(_, inner) => self.callable_sig(inner),
            Ty::Adt(d, args) if self.cx.db.is_lang(*d, "Box") => self.callable_sig(&args[0]),
            Ty::Dyn(d) if self.is_fn_trait(d.trait_) => {
                let params = match d.args.first().map(|a| self.peek(a)) {
                    Some(Ty::Tuple(ts)) => ts.to_vec(),
                    _ => Vec::new(),
                };
                let ret = d.bindings.iter().find(|(n, _)| &**n == "Output").map(|(_, t)| t.clone()).unwrap_or_else(crate::ty::unit);
                Some((params, ret))
            }
            _ => None,
        }
    }

    pub fn ctor_sig(&mut self, d: DefId, s: &[Ty]) -> Option<(Vec<Ty>, Ty)> {
        let (adt_def, vi) = match self.cx.db.def(d).kind {
            DefKind::Variant(i) => (self.cx.db.def(d).parent?, i),
            _ => (d, 0),
        };
        let adt = self.cx.adt(adt_def).ok()?;
        let ps = adt.variants[vi].fields.iter().map(|(_, t)| t.subst(s)).collect();
        Some((ps, Ty::Adt(adt_def, s.to_vec().into())))
    }

    pub fn is_fn_trait(&self, tr: DefId) -> bool {
        let db = self.cx.db;
        db.is_lang(tr, "Fn") || db.is_lang(tr, "FnMut") || db.is_lang(tr, "FnOnce")
    }

    /// The hidden type of `impl Trait` returned by an instance.
    /// The hidden type of an `impl Trait` (`None` while closures it
    /// depends on are not fully inferred).
    pub fn reveal(&mut self, def: DefId, substs: Rc<[Ty]>, span: Span) -> Result<Option<Ty>> {
        // The instance may call closures of this function: publish their
        // signatures once they are known.
        let ids: Vec<u32> = self.closure_sigs.keys().copied().collect();
        for id in ids {
            let (ps, r) = self.closure_sigs[&id].clone();
            let ps: Vec<Ty> = ps.iter().map(|p| self.resolve(p)).collect();
            let r = self.resolve(&r);
            if !ps.iter().any(|p| p.needs_infer()) && !r.needs_infer() {
                self.cx.set_closure_sig(id, ps, r);
            }
        }
        let mut missing = false;
        for t in substs.iter() {
            t.walk(&mut |x| {
                if let Ty::Closure(c) = x {
                    if self.cx.closure_sig(*c).is_none() {
                        missing = true;
                    }
                }
            });
        }
        if missing {
            return Ok(None);
        }
        let id = self.cx.instance(def, substs);
        if id == self.inst {
            return err(span, String::from("cannot use a recursive `impl Trait` function's result here"));
        }
        let t = self.cx.check_instance(id, span)?;
        Ok(Some(t))
    }

    // ---- trait selection ----

    /// Selects how `ty` implements `tr`; an impl found is committed:
    /// its parameters are unified with the type and its bounds become
    /// obligations.
    pub fn select(&mut self, ty: &Ty, tr: &TraitRef, span: Span) -> Result<Selection> {
        let ty = self.resolve(ty);
        let args = self.resolve_list(&tr.args);
        let tr = TraitRef {
            trait_: tr.trait_,
            args: args.into(),
        };
        if let Some(s) = self.builtin_select(&ty, &tr, span)? {
            return Ok(s);
        }
        if let Ty::Dyn(d) = &ty {
            let principal = TraitRef {
                trait_: d.trait_,
                args: d.args.clone(),
            };
            if self.cx.elaborate(&principal, &ty)?.iter().any(|t| t.trait_ == tr.trait_) {
                return Ok(Selection::Dyn);
            }
        }
        if matches!(ty, Ty::Infer(_)) {
            return Ok(Selection::Ambiguous);
        }
        let impls = self.cx.impls_of(tr.trait_)?;
        let mut viable: Vec<(DefId, Vec<Option<Ty>>, M)> = Vec::new();
        for imp in impls {
            let (m, binds) = self.match_impl(imp, &ty, &tr, 0)?;
            if m != M::No {
                viable.push((imp, binds, m));
                if viable.len() > 1 && viable.iter().filter(|v| v.2 == M::Yes).count() > 1 {
                    break;
                }
            }
        }
        if viable.is_empty() {
            return Ok(Selection::None);
        }
        if viable.len() > 1 {
            // Prefer a unique definite match only when the type is fully known.
            let yes: Vec<usize> = (0..viable.len()).filter(|&i| viable[i].2 == M::Yes).collect();
            if yes.len() == 1 && !ty.needs_infer() && !tr.args.iter().any(|a| a.needs_infer()) {
                let v = viable.swap_remove(yes[0]);
                return self.commit(v.0, v.1, &ty, &tr, span);
            }
            return Ok(Selection::Ambiguous);
        }
        let (imp, binds, _) = viable.pop().unwrap();
        self.commit(imp, binds, &ty, &tr, span)
    }

    fn commit(&mut self, imp: DefId, binds: Vec<Option<Ty>>, ty: &Ty, tr: &TraitRef, span: Span) -> Result<Selection> {
        let h = self.cx.impl_header(imp)?;
        let substs: Vec<Ty> = binds.into_iter().map(|b| b.unwrap_or_else(|| self.fresh())).collect();
        let st = h.self_ty.subst(&substs);
        self.unify(&st, ty);
        if let Some(htr) = &h.trait_ref {
            let a = crate::ty::subst_list(&htr.args, &substs);
            for (x, y) in a.iter().zip(tr.args.iter()) {
                self.unify(x, y);
            }
        }
        for p in &h.preds {
            let p = p.subst(&substs);
            self.obligations.push((Obligation::Pred(p), span));
        }
        Ok(Selection::Impl(imp, substs.into()))
    }

    /// Matches an impl against a type and trait arguments without side
    /// effects.
    pub fn match_impl(&mut self, imp: DefId, ty: &Ty, tr: &TraitRef, depth: u32) -> Result<(M, Vec<Option<Ty>>)> {
        let h = self.cx.impl_header(imp)?;
        let mut binds: Vec<Option<Ty>> = vec![None; h.generics.count()];
        let mut m = self.match_ty(&h.self_ty, ty, &mut binds);
        if m == M::No {
            return Ok((m, binds));
        }
        if let Some(htr) = &h.trait_ref {
            for (p, t) in htr.args.iter().zip(tr.args.iter()) {
                m = m.and(self.match_ty(p, t, &mut binds));
                if m == M::No {
                    return Ok((m, binds));
                }
            }
        }
        if depth < 6 {
            for p in &h.preds {
                if let Pred::Trait(t, r) = p {
                    let t2 = subst_partial(t, &binds);
                    let r2 = TraitRef {
                        trait_: r.trait_,
                        args: r.args.iter().map(|a| subst_partial(a, &binds)).collect::<Vec<_>>().into(),
                    };
                    m = m.and(self.evaluate(&t2, &r2, depth + 1)?);
                    if m == M::No {
                        return Ok((m, binds));
                    }
                }
            }
        }
        Ok((m, binds))
    }

    /// Whether a type implements a trait (without committing anything).
    pub fn evaluate(&mut self, ty: &Ty, tr: &TraitRef, depth: u32) -> Result<M> {
        let ty = self.resolve(ty);
        if ty.has_params() || matches!(ty, Ty::Infer(_) | Ty::Proj(..) | Ty::Opaque(..)) {
            return Ok(M::Maybe);
        }
        if depth > 8 {
            return Ok(M::Maybe);
        }
        if let Some(m) = self.builtin_evaluate(&ty, tr, depth)? {
            return Ok(m);
        }
        if let Ty::Dyn(d) = &ty {
            let principal = TraitRef {
                trait_: d.trait_,
                args: d.args.clone(),
            };
            if self.cx.elaborate(&principal, &ty)?.iter().any(|t| t.trait_ == tr.trait_) {
                return Ok(M::Yes);
            }
        }
        let mut best = M::No;
        for imp in self.cx.impls_of(tr.trait_)? {
            let (m, _) = self.match_impl(imp, &ty, tr, depth)?;
            match m {
                M::Yes => return Ok(M::Yes),
                M::Maybe => best = M::Maybe,
                M::No => {}
            }
        }
        Ok(best)
    }

    /// One-way matching of an impl type pattern against a type.
    pub fn match_ty(&self, pat: &Ty, target: &Ty, binds: &mut Vec<Option<Ty>>) -> M {
        let target = self.peek(target);
        if let Ty::Param(i) = pat {
            let i = *i as usize;
            return match binds[i].clone() {
                None => {
                    binds[i] = Some(target);
                    M::Yes
                }
                Some(b) => self.compat(&b, &target),
            };
        }
        match &target {
            Ty::Infer(_) | Ty::Proj(..) | Ty::Opaque(..) => return M::Maybe,
            Ty::IntVar(_) => return if matches!(pat, Ty::Int(_)) { M::Maybe } else { M::No },
            Ty::FloatVar(_) => return if matches!(pat, Ty::Float(_)) { M::Maybe } else { M::No },
            Ty::Error => return M::Yes,
            _ => {}
        }
        match (pat, &target) {
            (Ty::Proj(..), _) => M::Maybe,
            (Ty::Tuple(x), Ty::Tuple(y)) if x.len() == y.len() => self.match_lists(x, y, binds),
            (Ty::Ref(m1, x), Ty::Ref(m2, y)) | (Ty::Ptr(m1, x), Ty::Ptr(m2, y)) if m1 == m2 => self.match_ty(x, y, binds),
            (Ty::Array(x, n), Ty::Array(y, k)) => self.match_ty(x, y, binds).and(self.match_ty(n, k, binds)),
            (Ty::Slice(x), Ty::Slice(y)) => self.match_ty(x, y, binds),
            (Ty::Adt(d1, x), Ty::Adt(d2, y)) | (Ty::FnDef(d1, x), Ty::FnDef(d2, y)) if d1 == d2 => self.match_lists(x, y, binds),
            (Ty::FnPtr(p1, r1), Ty::FnPtr(p2, r2)) if p1.len() == p2.len() => self.match_lists(p1, p2, binds).and(self.match_ty(r1, r2, binds)),
            (Ty::Dyn(x), Ty::Dyn(y)) if x.trait_ == y.trait_ => self.match_lists(&x.args, &y.args, binds),
            (a, b) if a == b => M::Yes,
            _ => M::No,
        }
    }

    fn match_lists(&self, x: &[Ty], y: &[Ty], binds: &mut Vec<Option<Ty>>) -> M {
        let mut m = M::Yes;
        for (a, b) in x.iter().zip(y.iter()) {
            m = m.and(self.match_ty(a, b, binds));
            if m == M::No {
                break;
            }
        }
        m
    }

    fn lang_name(&self, tr: DefId) -> Option<&'static str> {
        self.cx.db.lang.items.iter().find(|(_, d)| **d == tr).map(|(n, _)| *n)
    }

    fn is_copy_builtin(&mut self, ty: &Ty, depth: u32) -> Result<M> {
        Ok(match ty {
            Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Never | Ty::Ptr(..) | Ty::FnPtr(..) | Ty::FnDef(..) | Ty::Ref(false, _) | Ty::IntVar(_) | Ty::FloatVar(_) => M::Yes,
            Ty::Tuple(ts) => {
                let mut m = M::Yes;
                for t in ts.iter() {
                    let copy = TraitRef {
                        trait_: self.cx.db.lang("Copy"),
                        args: Rc::from([]),
                    };
                    m = m.and(self.evaluate(t, &copy, depth + 1)?);
                }
                m
            }
            Ty::Array(t, _) => {
                let copy = TraitRef {
                    trait_: self.cx.db.lang("Copy"),
                    args: Rc::from([]),
                };
                self.evaluate(t, &copy, depth + 1)?
            }
            Ty::Closure(_) => M::Yes,
            _ => M::No,
        })
    }

    fn builtin_evaluate(&mut self, ty: &Ty, tr: &TraitRef, depth: u32) -> Result<Option<M>> {
        let name = match self.lang_name(tr.trait_) {
            Some(n) => n,
            None => {
                let n = &*self.cx.db.def(tr.trait_).name;
                if matches!(n, "Send" | "Sync" | "Unpin") && self.cx.db.def(tr.trait_).in_std {
                    return Ok(Some(M::Yes));
                }
                return Ok(None);
            }
        };
        Ok(match name {
            "Sized" => Some(if ty.is_unsized() { M::No } else { M::Yes }),
            "Copy" => match ty {
                Ty::Adt(..) => None,
                _ => Some(self.is_copy_builtin(ty, depth)?),
            },
            "Clone" => match ty {
                Ty::Adt(..) | Ty::Str | Ty::Slice(_) | Ty::Dyn(_) => None,
                Ty::Tuple(ts) => {
                    let mut m = M::Yes;
                    for t in ts.iter() {
                        m = m.and(self.evaluate(t, tr, depth + 1)?);
                    }
                    Some(m)
                }
                Ty::Array(t, _) => Some(self.evaluate(t, tr, depth + 1)?),
                Ty::Ref(true, _) => Some(M::No),
                _ => Some(self.is_copy_builtin(ty, depth)?),
            },
            "Fn" | "FnMut" | "FnOnce" => match ty {
                Ty::Closure(_) | Ty::FnPtr(..) | Ty::FnDef(..) => Some(M::Yes),
                Ty::Ref(_, inner) => Some(self.evaluate(inner, tr, depth + 1)?),
                Ty::Adt(d, a) if self.cx.db.is_lang(*d, "Box") => Some(self.evaluate(&a[0].clone(), tr, depth + 1)?),
                _ => None,
            },
            _ => None,
        })
    }

    fn builtin_select(&mut self, ty: &Ty, tr: &TraitRef, span: Span) -> Result<Option<Selection>> {
        let name = match self.lang_name(tr.trait_) {
            Some(n) => n,
            None => {
                let n = &*self.cx.db.def(tr.trait_).name;
                if matches!(n, "Send" | "Sync" | "Unpin") && self.cx.db.def(tr.trait_).in_std {
                    return Ok(Some(Selection::Builtin));
                }
                return Ok(None);
            }
        };
        match name {
            "Sized" => Ok(Some(if ty.is_unsized() { Selection::None } else { Selection::Builtin })),
            "Copy" | "Clone" => {
                if let Ty::Adt(..) | Ty::Infer(_) = ty {
                    return Ok(None);
                }
                if name == "Clone" && matches!(ty, Ty::Str | Ty::Slice(_) | Ty::Dyn(_)) {
                    return Ok(None);
                }
                match self.builtin_evaluate(ty, tr, 0)? {
                    Some(M::Yes) | Some(M::Maybe) => Ok(Some(Selection::Builtin)),
                    Some(M::No) => Ok(Some(Selection::None)),
                    None => Ok(None),
                }
            }
            "Fn" | "FnMut" | "FnOnce" => {
                let inner = match ty {
                    Ty::Ref(_, inner) => (**inner).clone(),
                    Ty::Adt(d, a) if self.cx.db.is_lang(*d, "Box") => a[0].clone(),
                    t => t.clone(),
                };
                let inner = self.shallow(&inner);
                match &inner {
                    Ty::Closure(_) | Ty::FnPtr(..) | Ty::FnDef(..) => {
                        if let Some((ps, _)) = self.callable_sig(&inner) {
                            if let Some(a) = tr.args.first() {
                                let t = Ty::Tuple(ps.into());
                                self.unify(a, &t);
                            }
                        }
                        Ok(Some(Selection::Builtin))
                    }
                    Ty::Dyn(d) if self.is_fn_trait(d.trait_) => {
                        if let (Some(a), Some(b)) = (tr.args.first(), d.args.first()) {
                            let (a, b) = (a.clone(), b.clone());
                            self.unify(&a, &b);
                        }
                        Ok(Some(if matches!(ty, Ty::Dyn(_)) { Selection::Dyn } else { Selection::Builtin }))
                    }
                    Ty::Infer(_) => Ok(Some(Selection::Ambiguous)),
                    _ => {
                        let _ = span;
                        Ok(None)
                    }
                }
            }
            _ => Ok(None),
        }
    }

    // ---- obligations ----

    pub fn register(&mut self, p: Pred, span: Span) {
        self.obligations.push((Obligation::Pred(p), span));
    }

    /// Processes obligations until no more progress; with `final_`,
    /// reports what is left.
    pub fn select_obligations(&mut self, final_: bool) -> Result<()> {
        for _round in 0..64 {
            let bound_before = self.vars.iter().filter(|v| matches!(v, Var::Bound(_))).count();
            let count_before = self.obligations.len();
            let mut progress = false;
            let obls = core::mem::take(&mut self.obligations);
            let mut keep = Vec::new();
            for (o, span) in obls {
                match &o {
                    Obligation::Eq(a, b) => {
                        let a2 = self.shallow(a);
                        let b2 = self.shallow(b);
                        let pending = |t: &Ty| matches!(t, Ty::Proj(..) | Ty::Opaque(..));
                        if pending(&a2) || pending(&b2) {
                            if pending(&a2) && pending(&b2) && a2 == b2 {
                                progress = true;
                                continue;
                            }
                            keep.push((o, span));
                        } else {
                            progress = true;
                            if !self.unify(&a2, &b2) {
                                let a3 = self.resolve(&a2);
                                let b3 = self.resolve(&b2);
                                return err(span, format!("mismatched types: expected `{}`, found `{}`", self.name(&a3), self.name(&b3)));
                            }
                        }
                    }
                    Obligation::Pred(Pred::Trait(ty, tr)) => {
                        let (ty, tr) = (ty.clone(), tr.clone());
                        match self.select(&ty, &tr, span)? {
                            Selection::Impl(..) | Selection::Builtin | Selection::Dyn => progress = true,
                            Selection::Ambiguous => keep.push((o, span)),
                            Selection::None => {
                                let t = self.resolve(&ty);
                                let tr2 = TraitRef {
                                    trait_: tr.trait_,
                                    args: self.resolve_list(&tr.args).into(),
                                };
                                return err(span, format!("the trait bound `{}` is not satisfied", self.cx.trait_ref_name(&t, &tr2)));
                            }
                        }
                    }
                    Obligation::Pred(Pred::Proj(ty, tr, name, val)) => {
                        let mut args = vec![ty.clone()];
                        args.extend(tr.args.iter().cloned());
                        let p = Ty::Proj(tr.trait_, args.into(), name.clone());
                        match self.try_normalize(&p) {
                            Some(n) => {
                                progress = true;
                                let val = val.clone();
                                if !self.unify(&n, &val) {
                                    let a = self.resolve(&n);
                                    let b = self.resolve(&val);
                                    return err(span, format!("type mismatch resolving `{}`: expected `{}`, found `{}`", name, self.name(&b), self.name(&a)));
                                }
                            }
                            None => keep.push((o, span)),
                        }
                    }
                }
            }
            self.obligations.extend(keep);
            // Selections and normalizations bind variables as a side effect.
            let bound_after = self.vars.iter().filter(|v| matches!(v, Var::Bound(_))).count();
            if bound_after != bound_before || self.obligations.len() != count_before {
                progress = true;
            }
            if !progress {
                break;
            }
        }
        if final_ && !self.obligations.is_empty() {
            let (o, span) = self.obligations[0].clone();
            return match o {
                Obligation::Pred(Pred::Trait(ty, tr)) => {
                    let t = self.resolve(&ty);
                    if t.needs_infer() {
                        err(span, format!("type annotations needed: cannot satisfy `{}`", self.cx.trait_ref_name(&t, &tr)))
                    } else {
                        err(span, format!("the trait bound `{}` is not satisfied", self.cx.trait_ref_name(&t, &tr)))
                    }
                }
                Obligation::Pred(Pred::Proj(ty, _, name, _)) => {
                    let t = self.resolve(&ty);
                    err(span, format!("cannot resolve associated type `{}` of `{}`", name, self.name(&t)))
                }
                Obligation::Eq(a, b) => {
                    let a = self.resolve(&a);
                    let b = self.resolve(&b);
                    err(span, format!("type annotations needed: cannot resolve `{}` == `{}`", self.name(&a), self.name(&b)))
                }
            };
        }
        Ok(())
    }

    /// Integer and float variables nobody constrained become i32 / f64.
    pub fn fallback(&mut self) {
        for i in 0..self.vars.len() {
            if let Var::Unbound(k) = self.vars[i] {
                match k {
                    VarKind::Int => self.vars[i] = Var::Bound(Ty::Int(IntTy::I32)),
                    VarKind::Float => self.vars[i] = Var::Bound(Ty::Float(crate::ty::FloatTy::F64)),
                    VarKind::General => {}
                }
            }
        }
    }

    /// Finishes inference for the body and resolves everything in it.
    pub fn finish(&mut self, mut body: tir::Expr) -> Result<tir::Expr> {
        if let Some(e) = self.deferred_error.take() {
            return Err(e);
        }
        self.select_obligations(false)?;
        self.fallback();
        self.select_obligations(true)?;
        self.zonk(&mut body)?;
        for i in 0..self.closure_bodies.len() {
            let mut b = core::mem::replace(
                &mut self.closure_bodies[i].1.body,
                tir::Expr {
                    kind: tir::ExprKind::Unit,
                    ty: crate::ty::unit(),
                    span: Span::default(),
                },
            );
            self.zonk(&mut b)?;
            self.closure_bodies[i].1.body = b;
            let ret = self.closure_bodies[i].1.ret.clone();
            self.closure_bodies[i].1.ret = self.resolve(&ret);
            let caps = self.closure_bodies[i].1.captures.clone();
            let caps = caps.into_iter().map(|(t, r)| (self.resolve(&t), r)).collect();
            self.closure_bodies[i].1.captures = caps;
        }
        let sigs: Vec<(u32, (Vec<Ty>, Ty))> = self.closure_sigs.iter().map(|(k, v)| (*k, v.clone())).collect();
        for (id, (ps, r)) in sigs {
            let ps: Vec<Ty> = ps.iter().map(|p| self.resolve(p)).collect();
            let r = self.resolve(&r);
            self.cx.set_closure_sig(id, ps.clone(), r.clone());
            self.closure_sigs.insert(id, (ps, r));
        }
        Ok(body)
    }

    /// Moves finished closure bodies into the program.
    pub fn finish_closures(&mut self, inst: tir::InstId) -> Result<()> {
        for (id, mut c) in core::mem::take(&mut self.closure_bodies) {
            c.func = inst;
            self.cx.closures[id as usize] = Some(c);
        }
        Ok(())
    }

    pub fn is_unit_path_name(&self, name: &str) -> bool {
        matches!(
            self.cx.db.lookup_scope(self.module, name, crate::resolve::Ns::Value),
            Some(crate::resolve::Res::Def(d)) if matches!(self.cx.db.def(d).kind, DefKind::Variant(_) | DefKind::Struct(_) | DefKind::Const(..) | DefKind::Static(..))
        )
    }
}

/// Substitutes the bound parameters, leaving unbound ones as parameters.
fn subst_partial(t: &Ty, binds: &[Option<Ty>]) -> Ty {
    t.map(&mut |x| match x {
        Ty::Param(i) => binds.get(*i as usize).cloned().flatten(),
        _ => None,
    })
}

impl<'a> Ctx<'a> {
    /// The type of an impl's associated type (with impl parameters).
    pub fn impl_assoc_ty(&mut self, imp: DefId, child: DefId) -> Result<Ty> {
        if let Some(t) = self.assoc_cache.get(&child) {
            return Ok(t.clone());
        }
        let t = match &self.db.def(child).kind {
            DefKind::ImplType(t) => t.clone(),
            _ => return err(self.db.def(child).span, String::from("not an associated type")),
        };
        let sc = self.item_scope(imp)?;
        let ty = self.lower_ty(&sc, &t, &mut NoInfer)?;
        self.assoc_cache.insert(child, ty.clone());
        Ok(ty)
    }

    pub fn closure_sig(&self, id: u32) -> Option<(Vec<Ty>, Ty)> {
        self.closure_sigs.get(id as usize).cloned().flatten()
    }

    pub fn set_closure_sig(&mut self, id: u32, ps: Vec<Ty>, r: Ty) {
        while self.closure_sigs.len() <= id as usize {
            self.closure_sigs.push(None);
        }
        self.closure_sigs[id as usize] = Some((ps, r));
    }
}
