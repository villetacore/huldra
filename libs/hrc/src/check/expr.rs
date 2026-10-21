//! Checking expressions, blocks, paths, calls and closures.

use super::desugar as d;
use super::infer::{ClosureCtx, FnCtx, LoopCtx, Obligation};
use super::lower::prim_type;
use super::{FieldsKind, Pred, TraitRef};
use crate::ast::{self, BinOp, ExprKind as E, Lit, Name, UnOp};
use crate::resolve::{DefId, DefKind, Ns, Res};
use crate::tir::{self, Callee, ExprKind as K};
use crate::ty::{FloatTy, IntTy, Ty};
use crate::{err, Result, Span};
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub fn mk(kind: K, ty: Ty, span: Span) -> tir::Expr {
    tir::Expr { kind, ty, span }
}

pub fn unit_expr(span: Span) -> tir::Expr {
    mk(K::Unit, crate::ty::unit(), span)
}

/// What a value path refers to.
pub enum ValueRes {
    Local(tir::Expr),
    /// A function, method, constructor or constant with its substs.
    Item(DefId, Vec<Ty>),
    /// A trait method or constant (substs: Self, trait args, own args).
    TraitItem(DefId, Vec<Ty>),
    Static(DefId),
    /// A unit struct or variant.
    Unit(DefId, Vec<Ty>),
    ConstParam(u64),
}

impl<'c, 'a> FnCtx<'c, 'a> {
    // ---- entry points ----

    pub fn check_expr_coerce(&mut self, e: &ast::Expr, ty: &Ty) -> Result<tir::Expr> {
        let x = self.check_expr(e, Some(ty))?;
        self.coerce(x, ty)
    }

    pub fn check_block_coerce(&mut self, b: &ast::Block, ty: &Ty) -> Result<tir::Expr> {
        let x = self.check_block(b, Some(ty))?;
        self.coerce(x, ty)
    }

    pub fn check_bool(&mut self, e: &ast::Expr) -> Result<tir::Expr> {
        self.check_expr_coerce(e, &Ty::Bool)
    }

    pub fn check_expr(&mut self, e: &ast::Expr, hint: Option<&Ty>) -> Result<tir::Expr> {
        let span = e.span;
        let hint = hint.map(|h| self.shallow(h));
        let hint = hint.as_ref();
        Ok(match &e.kind {
            E::Lit(l) => self.check_lit(l, hint, span)?,
            E::Paren(x) => self.check_expr(x, hint)?,
            E::Path(p) => self.check_path_expr(p, span)?,
            E::Unary(op, x) => self.check_unary(*op, x, hint, span)?,
            E::Ref(m, x) => {
                // `&expr` against `&str`: the inner hint would wrongly force
                // `str` on a `String`; only sized hints are useful.
                let inner_hint = match hint {
                    Some(Ty::Ref(_, t)) if !t.is_unsized() => Some((**t).clone()),
                    _ => None,
                };
                let mut place = self.check_expr(x, inner_hint.as_ref())?;
                if *m {
                    self.mutate_place(&mut place);
                }
                let ty = Ty::Ref(*m, Rc::new(place.ty.clone()));
                mk(K::AddrOf(*m, Box::new(place)), ty, span)
            }
            E::Binary(op, a, b) => self.check_binary(*op, a, b, hint, span)?,
            E::Assign(a, b) => self.check_assign(a, b, span)?,
            E::AssignOp(op, a, b) => self.check_assign_op(*op, a, b, span)?,
            E::Cast(x, t) => self.check_cast(x, t, span)?,
            E::Call(f, args) => self.check_call(f, args, hint, span)?,
            E::MethodCall { recv, name, args, params } => self.check_method_call(recv, name, args.as_ref(), params, hint, span)?,
            E::Field(x, name) => self.check_field(x, name, span)?,
            E::TupleField(x, i) => {
                let n: Name = Rc::from(format!("{}", i).as_str());
                self.check_field(x, &n, span)?
            }
            E::Index(b, i) => self.check_index(b, i, e.start(), span)?,
            E::Range(lo, hi, incl) => self.check_range(lo.as_deref(), hi.as_deref(), *incl, hint, span)?,
            E::Tuple(es) => {
                let hints: Vec<Option<Ty>> = match hint {
                    Some(Ty::Tuple(ts)) if ts.len() == es.len() => ts.iter().map(|t| Some(t.clone())).collect(),
                    _ => vec![None; es.len()],
                };
                let mut out = Vec::new();
                for (x, h) in es.iter().zip(hints.iter()) {
                    let v = match h {
                        Some(h) => {
                            let v = self.check_expr(x, Some(h))?;
                            let hr = self.shallow(h);
                            if hr.is_var() {
                                v
                            } else {
                                self.coerce(v, &hr)?
                            }
                        }
                        None => self.check_expr(x, None)?,
                    };
                    out.push(v);
                }
                let ty = Ty::Tuple(out.iter().map(|x| x.ty.clone()).collect::<Vec<_>>().into());
                if out.is_empty() {
                    unit_expr(span)
                } else {
                    mk(K::Tuple(out), ty, span)
                }
            }
            E::Array(es) => {
                let elem = match hint {
                    Some(Ty::Array(t, _)) => (**t).clone(),
                    _ => self.fresh(),
                };
                let mut out = Vec::new();
                for x in es {
                    let v = self.check_expr(x, Some(&elem))?;
                    out.push(self.coerce(v, &elem)?);
                }
                let ty = Ty::array(elem, out.len() as u64);
                mk(K::Array(out), ty, span)
            }
            E::ArrayRepeat(x, n) => {
                let elem = match hint {
                    Some(Ty::Array(t, _)) => (**t).clone(),
                    _ => self.fresh(),
                };
                let v = self.check_expr_coerce(x, &elem)?;
                let n = self.eval_len(n)?;
                mk(K::Repeat(Box::new(v), n), Ty::array(elem, n), span)
            }
            E::StructLit { path, fields, base } => self.check_struct_lit(path, fields, base.as_deref(), hint, span)?,
            E::Block(b, label) => match label {
                None => self.check_block(b, hint)?,
                Some(l) => self.check_labeled_block(b, l, hint, span)?,
            },
            E::Unsafe(b) => self.check_block(b, hint)?,
            E::If(c, t, els) => self.check_if(c, t, els.as_deref(), hint, span)?,
            E::Let(..) => return err(span, String::from("expected expression, found `let` statement")),
            E::Match(s, arms) => self.check_match(s, arms, hint, span)?,
            E::While(c, b, label) => self.check_while(c, b, label.clone(), span)?,
            E::Loop(b, label) => self.check_loop(b, label.clone(), hint, span)?,
            E::For(pat, it, body, label) => {
                let x = d::for_loop(pat, it, body, label, span);
                self.check_expr(&x, None)?
            }
            E::Break(label, v) => self.check_break(label.as_ref(), v.as_deref(), span)?,
            E::Continue(label) => {
                let id = self.find_loop(label.as_ref(), span, true)?;
                mk(K::Continue(id), Ty::Never, span)
            }
            E::Return(v) => {
                let rt = self.ret_ty.clone();
                let val = match v {
                    Some(v) => self.check_expr_coerce(v, &rt)?,
                    None => {
                        let u = unit_expr(span);
                        self.coerce(u, &rt)?
                    }
                };
                mk(K::Return(Box::new(val)), Ty::Never, span)
            }
            E::Try(x) => self.check_try(x, span)?,
            E::Closure { params, ret, body, is_move } => self.check_closure(params, ret.as_ref(), body, *is_move, hint, span)?,
            E::Format(kind, f) => {
                let x = d::macro_expr(*kind, f);
                self.check_expr(&x, hint)?
            }
            E::Write(dst, f) => {
                let x = d::write_expr(dst, f);
                self.check_expr(&x, hint)?
            }
            E::VecLit(es) => self.check_vec_lit(es, hint, span)?,
            E::VecRepeat(x, n) => {
                let f = d::call(d::std_expr(&["vec", "from_elem"], span), vec![(**x).clone(), (**n).clone()], span);
                self.check_expr(&f, hint)?
            }
            E::Assert(c, m) => {
                let x = d::assert_expr(c, m, span);
                self.check_expr(&x, None)?
            }
            E::AssertCmp(eq, a, b, m) => {
                let x = d::assert_cmp_expr(*eq, a, b, m, span);
                self.check_expr(&x, None)?
            }
            E::Matches(x, p, g) => {
                let x = d::matches_expr(x, p, g, span);
                self.check_expr(&x, None)?
            }
            E::Dbg(x, text) => {
                let x = d::dbg_expr(x, text, span);
                self.check_expr(&x, hint)?
            }
            E::Checked(i) => match self.checked.get_mut(*i as usize).and_then(|c| c.take()) {
                Some(x) => x,
                None => return err(span, String::from("internal error: checked expression used twice")),
            },
            E::Location => {
                let loc = self.cx.db.lang("Location");
                mk(K::Location(span), Ty::reference(false, Ty::Adt(loc, Rc::from([]))), span)
            }
        })
    }

    /// Stashes a checked expression for an AST built around it.
    pub fn stash(&mut self, x: tir::Expr) -> ast::Expr {
        let span = x.span;
        self.checked.push(Some(x));
        d::e(E::Checked((self.checked.len() - 1) as u32), span)
    }

    // ---- literals ----

    fn check_lit(&mut self, l: &Lit, hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        Ok(match l {
            Lit::Int(v, suffix) => {
                let ty = match suffix {
                    Some(s) => match prim_type(s) {
                        Some(t @ (Ty::Int(_) | Ty::Float(_))) => t,
                        _ => return err(span, format!("invalid suffix `{}` for number literal", s)),
                    },
                    None => match hint {
                        Some(h @ (Ty::Int(_) | Ty::IntVar(_))) => h.clone(),
                        _ => self.fresh_int(),
                    },
                };
                if let Ty::Float(_) = ty {
                    mk(K::Float(*v as f64), ty, span)
                } else {
                    mk(K::Int(*v), ty, span)
                }
            }
            Lit::Float(text, suffix) => {
                let ty = match suffix.as_deref() {
                    Some("f32") => Ty::Float(FloatTy::F32),
                    Some("f64") => Ty::Float(FloatTy::F64),
                    Some(s) => return err(span, format!("invalid suffix `{}` for float literal", s)),
                    None => match hint {
                        Some(h @ (Ty::Float(_) | Ty::FloatVar(_))) => h.clone(),
                        _ => self.fresh_float(),
                    },
                };
                let v = parse_float(text).ok_or_else(|| crate::Error::new(span, String::from("invalid float literal")))?;
                mk(K::Float(v), ty, span)
            }
            Lit::Str(s) => mk(K::Str(s.clone()), Ty::reference(false, Ty::Str), span),
            Lit::ByteStr(b) => {
                let n = b.len() as u64;
                mk(K::ByteStr(b.clone()), Ty::reference(false, Ty::array(Ty::Int(IntTy::U8), n)), span)
            }
            Lit::Char(c) => mk(K::Char(*c), Ty::Char, span),
            Lit::Byte(b) => mk(K::Int(*b as u128), Ty::Int(IntTy::U8), span),
            Lit::Bool(b) => mk(K::Bool(*b), Ty::Bool, span),
        })
    }

    // ---- paths ----

    fn check_path_expr(&mut self, p: &ast::Path, span: Span) -> Result<tir::Expr> {
        let r = self.resolve_value_path(p, span)?;
        self.value_res_expr(r, span)
    }

    pub fn value_res_expr(&mut self, r: ValueRes, span: Span) -> Result<tir::Expr> {
        Ok(match r {
            ValueRes::Local(x) => x,
            ValueRes::ConstParam(v) => mk(K::Int(v as u128), Ty::usize(), span),
            ValueRes::Static(def) => {
                let i = self.cx.static_index(def)?;
                let ty = self.cx.statics[i].ty.clone();
                mk(K::Static(def), ty, span)
            }
            ValueRes::Unit(def, substs) => {
                let (adt, _) = self.adt_of(def);
                mk(K::Adt(def, Vec::new()), Ty::Adt(adt, substs.into()), span)
            }
            ValueRes::Item(def, substs) | ValueRes::TraitItem(def, substs) => {
                let trait_item = matches!(self.cx.db.def(def).parent.map(|p| &self.cx.db.def(p).kind), Some(DefKind::Trait(_)));
                match &self.cx.db.def(def).kind {
                    DefKind::Const(..) | DefKind::AssocConst(..) => {
                        let ty = self.const_type(def, &substs)?;
                        // Inline literal constants.
                        if let DefKind::Const(_, ce) = &self.cx.db.def(def).kind {
                            if let E::Lit(l) = &ce.kind {
                                let l = l.clone();
                                let x = self.check_lit(&l, Some(&ty), span)?;
                                return self.coerce(x, &ty);
                            }
                        }
                        let callee = if trait_item { Callee::Trait(def, substs.into()) } else { Callee::Item(def, substs.into()) };
                        mk(K::Call(callee, Vec::new()), ty, span)
                    }
                    _ => mk(K::FnItem, Ty::FnDef(def, substs.into()), span),
                }
            }
        })
    }

    fn const_type(&mut self, def: DefId, substs: &[Ty]) -> Result<Ty> {
        let t = match &self.cx.db.def(def).kind {
            DefKind::Const(t, _) | DefKind::AssocConst(t, _) => t.clone(),
            _ => unreachable!(),
        };
        let sc = self.cx.item_scope(def)?;
        let ty = self.cx.lower_ty(&sc, &t, &mut super::lower::NoInfer)?;
        let ty = ty.subst(substs);
        Ok(self.resolve(&ty))
    }

    /// The enum (or struct) and variant index of a constructor.
    pub fn adt_of(&self, def: DefId) -> (DefId, usize) {
        match self.cx.db.def(def).kind {
            DefKind::Variant(i) => (self.cx.db.def(def).parent.unwrap(), i),
            _ => (def, 0),
        }
    }

    /// The place of a local, through closure captures if needed.
    pub fn local_place(&mut self, id: tir::LocalId, span: Span) -> tir::Expr {
        let ty = self.locals[id as usize].ty.clone();
        let depth = self.local_depth[id as usize];
        if depth == self.closures.len() {
            return mk(K::Local(id), ty, span);
        }
        // Captured by every closure from `depth` inward.
        let mut idx = 0;
        for level in depth..self.closures.len() {
            let c = &mut self.closures[level];
            idx = match c.captures.iter().position(|(l, _)| *l == id) {
                Some(i) => i,
                None => {
                    let by_ref = !c.is_move;
                    c.captures.push((id, by_ref));
                    c.captures.len() - 1
                }
            };
        }
        mk(K::Upvar(idx as u32), ty, span)
    }

    fn item_substs(&mut self, def: DefId, mut parent: Vec<Ty>, args: Option<&ast::GenericArgs>, span: Span) -> Result<Vec<Ty>> {
        let g = self.cx.generics_of(def);
        while parent.len() < g.own_start {
            let v = self.fresh();
            parent.push(v);
        }
        parent.truncate(g.own_start);
        let own = g.count() - g.own_start;
        let given: &[ast::Type] = args.map(|a| &a.types[..]).unwrap_or(&[]);
        let explicit_own = g.params[g.own_start..].iter().filter(|p| !p.name.starts_with("impl#")).count();
        if given.len() > explicit_own {
            return err(span, format!("function takes {} generic arguments but {} were supplied", explicit_own, given.len()));
        }
        for i in 0..own {
            let p = g.params[g.own_start + i].clone();
            let t = match given.get(i) {
                Some(t) if p.is_const => match &t.kind {
                    ast::TypeKind::Array(_, e) => Ty::Const(self.eval_len(e)?),
                    ast::TypeKind::Path(pp) => {
                        let e = ast::Expr {
                            kind: E::Path(pp.clone()),
                            span: t.span,
                        };
                        Ty::Const(self.eval_len(&e)?)
                    }
                    _ => self.fresh(),
                },
                Some(t) => self.lower_body_ty(t)?,
                None => self.fresh(),
            };
            parent.push(t);
        }
        Ok(parent)
    }

    /// Evaluates an array length (a constant or a const parameter).
    pub fn eval_len(&mut self, e: &ast::Expr) -> Result<u64> {
        if let E::Path(p) = &e.kind {
            if let Some(n) = p.as_ident() {
                if self.lookup_local(n).is_none() {
                    if let Some(i) = self.scope.generics.index_of(n) {
                        if let Some(Ty::Const(v)) = self.substs.get(i) {
                            return Ok(*v);
                        }
                    }
                }
            }
        }
        let v = self.cx.const_eval(self.module, e)?;
        if v < 0 {
            return err(e.span, String::from("array length must be non-negative"));
        }
        Ok(v as u64)
    }

    /// Resolves a path in value position.
    pub fn resolve_value_path(&mut self, p: &ast::Path, span: Span) -> Result<ValueRes> {
        if let Some(q) = &p.qself {
            let base = self.lower_body_ty(&q.ty)?;
            let seg = &p.segs[0];
            if let Some(tp) = &q.trait_ {
                let sc = self.body_scope();
                let (tr, _) = self.cx.lower_trait_ref(&sc, tp, &base, &mut super::lower::NoInfer)?;
                let tr = tr.subst(&self.substs);
                return self.trait_item_path(tr.trait_, Some(base), Some(tr.args.to_vec()), seg, span);
            }
            return self.type_relative(base, seg, span);
        }
        let names: Vec<Name> = p.segs.iter().map(|s| s.name.clone()).collect();
        let first = &p.segs[0];
        if !p.global && p.segs.len() == 1 && first.args.is_none() {
            if let Some(id) = self.lookup_local(&first.name) {
                return Ok(ValueRes::Local(self.local_place(id, span)));
            }
            if &*first.name == "__hrc_file" {
                let f = self.cx.sources.names.get(span.file as usize).cloned().unwrap_or_default();
                return Ok(ValueRes::Local(mk(K::Str(Rc::from(f.as_str())), Ty::reference(false, Ty::Str), span)));
            }
            if let Some(i) = self.scope.generics.index_of(&first.name) {
                if self.scope.generics.params[i].is_const {
                    if let Some(Ty::Const(v)) = self.substs.get(i) {
                        return Ok(ValueRes::ConstParam(*v));
                    }
                }
            }
        }
        // `T::method`, `Self::new`, `Self(..)`
        if !p.global && first.args.is_none() {
            let gi = self.scope.generics.index_of(&first.name).filter(|_| &*first.name != "Self" || self.scope.trait_def.is_some());
            let base = if let Some(i) = gi {
                Some(self.substs.get(i).cloned().unwrap_or(Ty::Error))
            } else if &*first.name == "Self" {
                self.scope.self_ty.clone().map(|t| t.subst(&self.substs))
            } else {
                None
            };
            if let Some(base) = base {
                let base = self.resolve(&base);
                if p.segs.len() == 1 {
                    // `Self` as a value: a unit or tuple struct.
                    if let Ty::Adt(def, substs) = &base {
                        return Ok(match self.cx.adt(*def)?.variants[0].kind {
                            FieldsKind::Unit => ValueRes::Unit(*def, substs.to_vec()),
                            _ => ValueRes::Item(*def, substs.to_vec()),
                        });
                    }
                    return err(span, String::from("the `Self` constructor can only be used with tuple or unit structs"));
                }
                let mut t = base;
                let mut via_self = &*first.name == "Self";
                for s in &p.segs[1..p.segs.len() - 1] {
                    let sc = self.body_scope();
                    t = self.cx.assoc_type(&sc, t, &s.name, span, via_self)?;
                    via_self = false;
                    t = self.resolve(&t);
                }
                return self.type_relative(t, p.segs.last().unwrap(), span);
            }
        }
        let (m, i) = self.cx.db.resolve_module_prefix(self.module, &names, p.global);
        if i >= names.len() {
            return err(span, format!("expected value, found module `{}`", names.last().unwrap()));
        }
        if i + 1 == names.len() {
            let seg = &p.segs[i];
            match self.cx.db.lookup_seg(m, i, &seg.name, Ns::Value) {
                Some(Res::Def(def)) => return self.value_item(def, Vec::new(), seg.args.as_ref(), span),
                _ => {
                    return err(span, format!("cannot find value `{}` in this scope", seg.name));
                }
            }
        }
        // The segment at `i` is a type (or trait); the rest is relative to it.
        let seg = &p.segs[i];
        let res = self.cx.db.lookup_seg(m, i, &seg.name, Ns::Type);
        let base: Ty = match res {
            Some(Res::Def(def)) => match self.cx.db.def(def).kind.clone() {
                DefKind::Enum(_) if i + 2 == names.len() => {
                    let vname = &p.segs[i + 1].name;
                    if let Some(v) = self.cx.db.def(def).children.iter().copied().find(|c| &*self.cx.db.def(*c).name == &**vname) {
                        // Generic args on the enum or on the variant.
                        let args = seg.args.as_ref().or(p.segs[i + 1].args.as_ref());
                        let substs = self.item_substs(def, Vec::new(), args, span)?;
                        return self.variant_value(v, substs);
                    }
                    let substs = self.item_substs(def, Vec::new(), seg.args.as_ref(), span)?;
                    Ty::Adt(def, substs.into())
                }
                DefKind::Struct(_) | DefKind::Enum(_) => {
                    let substs = self.item_substs(def, Vec::new(), seg.args.as_ref(), span)?;
                    Ty::Adt(def, substs.into())
                }
                DefKind::TypeAlias(..) => {
                    let mut q = p.clone();
                    q.segs.truncate(i + 1);
                    let t = ast::Type {
                        kind: ast::TypeKind::Path(q),
                        span,
                    };
                    let ty = self.lower_body_ty(&t)?;
                    self.resolve(&ty)
                }
                DefKind::Trait(_) => {
                    if i + 2 != names.len() {
                        return err(span, String::from("unsupported path"));
                    }
                    return self.trait_item_path(def, None, None, &p.segs[i + 1], span);
                }
                _ => return err(span, format!("expected type, found `{}`", self.cx.db.def(def).name)),
            },
            Some(Res::Mod(_)) => return err(span, format!("cannot find value `{}`", names.last().unwrap())),
            None => match (i, prim_type(&seg.name)) {
                (0, Some(t)) => t,
                _ => return err(span, format!("failed to resolve: use of undeclared type `{}`", seg.name)),
            },
        };
        let mut t = base;
        for s in &p.segs[i + 1..p.segs.len() - 1] {
            let sc = self.body_scope();
            t = self.cx.assoc_type(&sc, t, &s.name, span, false)?;
            t = self.resolve(&t);
        }
        self.type_relative(t, p.segs.last().unwrap(), span)
    }

    fn variant_value(&mut self, v: DefId, substs: Vec<Ty>) -> Result<ValueRes> {
        let (adt, vi) = self.adt_of(v);
        let info = self.cx.adt(adt)?;
        Ok(match info.variants[vi].kind {
            FieldsKind::Unit => ValueRes::Unit(v, substs),
            _ => ValueRes::Item(v, substs),
        })
    }

    fn value_item(&mut self, def: DefId, parent: Vec<Ty>, args: Option<&ast::GenericArgs>, span: Span) -> Result<ValueRes> {
        let kind = self.cx.db.def(def).kind.clone();
        match kind {
            DefKind::Fn(_) | DefKind::ExternFn(..) => {
                let substs = self.item_substs(def, parent, args, span)?;
                Ok(ValueRes::Item(def, substs))
            }
            DefKind::Const(..) => Ok(ValueRes::Item(def, Vec::new())),
            DefKind::Static(..) => Ok(ValueRes::Static(def)),
            DefKind::Struct(_) => {
                let substs = self.item_substs(def, Vec::new(), args, span)?;
                let info = self.cx.adt(def)?;
                match info.variants[0].kind {
                    FieldsKind::Unit => Ok(ValueRes::Unit(def, substs)),
                    FieldsKind::Tuple => Ok(ValueRes::Item(def, substs)),
                    FieldsKind::Named => err(span, format!("expected value, found struct `{}`", self.cx.db.def(def).name)),
                }
            }
            DefKind::Variant(_) => {
                let enum_ = self.cx.db.def(def).parent.unwrap();
                let substs = self.item_substs(enum_, Vec::new(), args, span)?;
                self.variant_value(def, substs)
            }
            _ => err(span, format!("expected value, found `{}`", self.cx.db.def(def).name)),
        }
    }

    /// `Trait::item` (Self from the arguments or `<T as Trait>`).
    fn trait_item_path(&mut self, tr: DefId, self_ty: Option<Ty>, tr_args: Option<Vec<Ty>>, seg: &ast::PathSeg, span: Span) -> Result<ValueRes> {
        let item = match self.cx.db.child(tr, &seg.name) {
            Some(c) => c,
            None => {
                // A method of a supertrait?
                return err(span, format!("cannot find method or associated constant `{}` in trait `{}`", seg.name, self.cx.db.def(tr).name));
            }
        };
        let g = self.cx.generics_of(tr);
        let mut parent = vec![self_ty.unwrap_or_else(|| self.fresh())];
        match tr_args {
            Some(a) => parent.extend(a),
            None => {
                for _ in 1..g.count() {
                    let v = self.fresh();
                    parent.push(v);
                }
            }
        }
        match self.cx.db.def(item).kind {
            DefKind::Fn(_) => {
                let substs = self.item_substs(item, parent, seg.args.as_ref(), span)?;
                Ok(ValueRes::TraitItem(item, substs))
            }
            DefKind::AssocConst(..) => Ok(ValueRes::TraitItem(item, parent)),
            _ => err(span, format!("`{}` is not a value", seg.name)),
        }
    }

    /// `Type::item`: a variant, an inherent associated item, or a trait's.
    pub fn type_relative(&mut self, ty: Ty, seg: &ast::PathSeg, span: Span) -> Result<ValueRes> {
        let ty = self.resolve(&ty);
        let name = &seg.name;
        if let Ty::Adt(def, substs) = &ty {
            if let DefKind::Enum(_) = self.cx.db.def(*def).kind {
                if let Some(v) = self.cx.db.def(*def).children.iter().copied().find(|c| &*self.cx.db.def(*c).name == &**name) {
                    return self.variant_value(v, substs.to_vec());
                }
            }
        }
        // Inherent impls.
        for imp in self.cx.inherent_impls()? {
            let Some(child) = self.cx.db.child(imp, name) else { continue };
            let h = self.cx.impl_header(imp)?;
            let mut binds = vec![None; h.generics.count()];
            if self.match_ty(&h.self_ty, &ty, &mut binds) == super::infer::M::No {
                continue;
            }
            let isubsts: Vec<Ty> = binds.into_iter().map(|b| b.unwrap_or_else(|| self.fresh())).collect();
            let st = h.self_ty.subst(&isubsts);
            self.unify(&st, &ty);
            return match self.cx.db.def(child).kind {
                DefKind::Fn(_) => {
                    let substs = self.item_substs(child, isubsts, seg.args.as_ref(), span)?;
                    Ok(ValueRes::Item(child, substs))
                }
                DefKind::AssocConst(..) => Ok(ValueRes::Item(child, isubsts)),
                _ => err(span, format!("`{}` is not a value", name)),
            };
        }
        // Trait items (`T::default()`, `i32::from_str_radix` lives in an impl).
        let mut found: Vec<DefId> = Vec::new();
        for (tr, _) in self.traits_with_item(name) {
            let g = self.cx.generics_of(tr);
            let tref = TraitRef {
                trait_: tr,
                args: (1..g.count()).map(|_| Ty::Error).collect::<Vec<_>>().into(),
            };
            let m = if g.count() > 1 {
                super::infer::M::Maybe
            } else {
                self.evaluate(&ty, &tref, 0)?
            };
            if m != super::infer::M::No {
                found.push(tr);
            }
        }
        if found.len() > 1 {
            // Prefer a trait the type surely implements.
            let mut sure = Vec::new();
            for &tr in &found {
                let g = self.cx.generics_of(tr);
                if g.count() == 1 {
                    let tref = TraitRef {
                        trait_: tr,
                        args: Rc::from([]),
                    };
                    if self.evaluate(&ty, &tref, 0)? == super::infer::M::Yes {
                        sure.push(tr);
                    }
                }
            }
            if sure.len() == 1 {
                found = sure;
            }
        }
        match found.len() {
            0 => err(span, format!("no function or associated item named `{}` found for `{}`", name, self.name(&ty))),
            1 => self.trait_item_path(found[0], Some(ty), None, seg, span),
            _ => err(span, format!("multiple applicable items in scope: `{}` is defined in several traits for `{}`", name, self.name(&ty))),
        }
    }

    /// Traits that have an associated function or constant with this name.
    fn traits_with_item(&mut self, name: &str) -> Vec<(DefId, DefId)> {
        let mut out = Vec::new();
        for (i, d) in self.cx.db.defs.iter().enumerate() {
            if &*d.name != name {
                continue;
            }
            if let Some(p) = d.parent {
                if let DefKind::Trait(_) = self.cx.db.def(p).kind {
                    if matches!(d.kind, DefKind::Fn(_) | DefKind::AssocConst(..)) {
                        out.push((p, i as DefId));
                    }
                }
            }
        }
        out
    }

    // ---- unary and binary operators ----

    fn check_unary(&mut self, op: UnOp, x: &ast::Expr, hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        match op {
            UnOp::Deref => {
                let v = self.check_expr(x, None)?;
                self.deref_expr(v, span)
            }
            UnOp::Neg => {
                // Fold negative literals so `-128i8` is in range.
                if let E::Lit(Lit::Int(v, _)) = &x.kind {
                    let lit = self.check_expr(x, hint)?;
                    let t = self.shallow(&lit.ty);
                    if let Ty::Int(it) = t {
                        if !it.signed() {
                            return err(span, format!("cannot apply unary operator `-` to type `{}`", it.name()));
                        }
                    }
                    if let K::Float(f) = lit.kind {
                        return Ok(mk(K::Float(-f), lit.ty, span));
                    }
                    return Ok(mk(K::Int((-(*v as i128)) as u128), lit.ty, span));
                }
                let mut v = self.check_expr(x, hint)?;
                let mut t = self.shallow(&v.ty);
                if let Ty::Ref(_, inner) = &t {
                    if inner.is_numeric() || matches!(self.shallow(inner), Ty::Int(_) | Ty::Float(_)) {
                        let inner = (**inner).clone();
                        v = mk(K::Deref(Box::new(v)), inner.clone(), span);
                        t = self.shallow(&inner);
                    }
                }
                match t {
                    Ty::Int(it) if !it.signed() => err(span, format!("cannot apply unary operator `-` to type `{}`", it.name())),
                    Ty::Int(_) | Ty::IntVar(_) | Ty::Float(_) | Ty::FloatVar(_) => Ok(mk(K::Unary(UnOp::Neg, Box::new(v)), t, span)),
                    _ => self.overloaded_unary("Neg", "neg", v, span),
                }
            }
            UnOp::Not => {
                let mut v = self.check_expr(x, hint)?;
                let mut t = self.shallow(&v.ty);
                if let Ty::Ref(_, inner) = &t {
                    let it = self.shallow(inner);
                    if matches!(it, Ty::Int(_) | Ty::Bool | Ty::IntVar(_)) {
                        v = mk(K::Deref(Box::new(v)), it.clone(), span);
                        t = it;
                    }
                }
                match t {
                    Ty::Bool | Ty::Int(_) | Ty::IntVar(_) => Ok(mk(K::Unary(UnOp::Not, Box::new(v)), t, span)),
                    _ => self.overloaded_unary("Not", "not", v, span),
                }
            }
        }
    }

    fn overloaded_unary(&mut self, tr: &str, method: &str, v: tir::Expr, span: Span) -> Result<tir::Expr> {
        let trd = self.cx.db.lang(tr);
        let m = self.cx.db.child(trd, method).unwrap();
        let st = v.ty.clone();
        let out = Ty::Proj(trd, Rc::from([st.clone()]), Rc::from("Output"));
        self.register(
            Pred::Trait(
                st.clone(),
                TraitRef {
                    trait_: trd,
                    args: Rc::from([]),
                },
            ),
            span,
        );
        let out = self.shallow(&out);
        Ok(mk(K::Call(Callee::Trait(m, Rc::from([st])), vec![v]), out, span))
    }

    /// `*e` on references, raw pointers, `Box` and `Deref` types.
    pub fn deref_expr(&mut self, v: tir::Expr, span: Span) -> Result<tir::Expr> {
        let t = self.shallow(&v.ty);
        match &t {
            Ty::Ref(_, inner) | Ty::Ptr(_, inner) => {
                let inner = (**inner).clone();
                Ok(mk(K::Deref(Box::new(v)), inner, span))
            }
            Ty::Adt(d, args) if self.cx.db.is_lang(*d, "Box") => {
                let inner = args[0].clone();
                Ok(mk(K::Deref(Box::new(v)), inner, span))
            }
            Ty::Infer(_) => err(span, String::from("type annotations needed")),
            _ => {
                let deref = self.cx.db.lang("Deref");
                let tref = TraitRef {
                    trait_: deref,
                    args: Rc::from([]),
                };
                match self.select(&t, &tref, span)? {
                    super::infer::Selection::Impl(..) => {}
                    _ => return err(span, format!("type `{}` cannot be dereferenced", self.name(&t))),
                }
                let target = Ty::Proj(deref, Rc::from([t.clone()]), Rc::from("Target"));
                let target = self.resolve(&target);
                let m = self.cx.db.child(deref, "deref").unwrap();
                let r = mk(K::AddrOf(false, Box::new(v)), Ty::reference(false, t.clone()), span);
                let call = mk(K::Call(Callee::Trait(m, Rc::from([t])), vec![r]), Ty::reference(false, target.clone()), span);
                Ok(mk(K::Deref(Box::new(call)), target, span))
            }
        }
    }

    /// Processes pending obligations when a type is not known yet.
    pub fn settle(&mut self, t: &Ty) -> Result<()> {
        let s = self.shallow(t);
        let unknown = match &s {
            Ty::Infer(_) | Ty::Proj(..) => true,
            Ty::Ref(_, inner) => matches!(self.shallow(inner), Ty::Infer(_) | Ty::Proj(..)),
            _ => false,
        };
        if unknown {
            self.select_obligations(false)?;
        }
        Ok(())
    }

    fn is_prim_operand(&mut self, t: &Ty) -> bool {
        matches!(self.shallow(t), Ty::Int(_) | Ty::IntVar(_) | Ty::Float(_) | Ty::FloatVar(_) | Ty::Bool | Ty::Char)
    }

    /// Strips a reference to a primitive (`&i32 + 1`).
    fn deref_prim(&mut self, v: tir::Expr) -> tir::Expr {
        let t = self.shallow(&v.ty);
        if let Ty::Ref(_, inner) = &t {
            let it = self.shallow(inner);
            if matches!(it, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::IntVar(_) | Ty::FloatVar(_)) {
                let span = v.span;
                let x = mk(K::Deref(Box::new(v)), it, span);
                return self.deref_prim(x);
            }
        }
        v
    }

    fn check_binary(&mut self, op: BinOp, a: &ast::Expr, b: &ast::Expr, hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        if op == BinOp::And || op == BinOp::Or {
            let l = self.check_bool(a)?;
            let r = self.check_bool(b)?;
            let k = if op == BinOp::And { K::And(Box::new(l), Box::new(r)) } else { K::Or(Box::new(l), Box::new(r)) };
            return Ok(mk(k, Ty::Bool, span));
        }
        if op.is_comparison() {
            let mut l = self.check_expr(a, None)?;
            self.settle(&l.ty)?;
            let lt = self.shallow(&l.ty);
            let mut r = self.check_expr(b, Some(&lt))?;
            self.settle(&r.ty)?;
            // `&a == &b` compares the referents.
            loop {
                let lt = self.shallow(&l.ty);
                let rt = self.shallow(&r.ty);
                match (&lt, &rt) {
                    (Ty::Ref(_, x), Ty::Ref(_, y)) => {
                        let (x, y) = ((**x).clone(), (**y).clone());
                        l = mk(K::Deref(Box::new(l)), x, span);
                        r = mk(K::Deref(Box::new(r)), y, span);
                    }
                    _ => break,
                }
            }
            let lt = self.shallow(&l.ty);
            let rt = self.shallow(&r.ty);
            let prim = |t: &Ty| matches!(t, Ty::Int(_) | Ty::IntVar(_) | Ty::Float(_) | Ty::FloatVar(_) | Ty::Bool | Ty::Char | Ty::Ptr(..) | Ty::FnPtr(..));
            if prim(&lt) && (prim(&rt) || rt.is_var()) || (lt.is_var() && prim(&rt)) {
                if !self.unify(&lt, &rt) {
                    return err(span, format!("mismatched types: expected `{}`, found `{}`", self.name(&lt), self.name(&rt)));
                }
                return Ok(mk(K::Binary(op, Box::new(l), Box::new(r)), Ty::Bool, span));
            }
            if lt.is_unit() && rt.is_unit() {
                let v = matches!(op, BinOp::Eq | BinOp::Le | BinOp::Ge);
                return Ok(mk(K::Bool(v), Ty::Bool, span));
            }
            let (tr, m) = match op {
                BinOp::Eq => ("PartialEq", "eq"),
                BinOp::Ne => ("PartialEq", "ne"),
                BinOp::Lt => ("PartialOrd", "lt"),
                BinOp::Le => ("PartialOrd", "le"),
                BinOp::Gt => ("PartialOrd", "gt"),
                _ => ("PartialOrd", "ge"),
            };
            let trd = self.cx.db.lang(tr);
            let md = self.cx.db.child(trd, m).unwrap();
            self.register(
                Pred::Trait(
                    lt.clone(),
                    TraitRef {
                        trait_: trd,
                        args: Rc::from([rt.clone()]),
                    },
                ),
                span,
            );
            let la = mk(K::AddrOf(false, Box::new(l)), Ty::reference(false, lt.clone()), span);
            let ra = mk(K::AddrOf(false, Box::new(r)), Ty::reference(false, rt.clone()), span);
            return Ok(mk(K::Call(Callee::Trait(md, Rc::from([lt, rt])), vec![la, ra]), Ty::Bool, span));
        }
        // Arithmetic and bit operations.
        let l = self.check_expr(a, hint.filter(|h| h.is_numeric()))?;
        self.settle(&l.ty)?;
        let l = self.deref_prim(l);
        let lt = self.shallow(&l.ty);
        let rhint = if matches!(op, BinOp::Shl | BinOp::Shr) || !self.is_prim_operand(&lt) { None } else { Some(lt.clone()) };
        let r = self.check_expr(b, rhint.as_ref())?;
        self.settle(&r.ty)?;
        let r = self.deref_prim(r);
        let rt = self.shallow(&r.ty);
        let lp = self.is_prim_operand(&lt);
        let rp = self.is_prim_operand(&rt);
        if lp && (rp || rt.is_var()) || (lt.is_var() && rp) {
            if matches!(op, BinOp::Shl | BinOp::Shr) {
                if !lt.is_integral() && !lt.is_var() || !rt.is_integral() && !rt.is_var() {
                    return err(span, format!("no implementation for `{} {} {}`", self.name(&lt), op.symbol(), self.name(&rt)));
                }
                return Ok(mk(K::Binary(op, Box::new(l), Box::new(r)), lt, span));
            }
            if !self.unify(&lt, &rt) {
                let (lt, rt) = (self.resolve(&lt), self.resolve(&rt));
                return err(span, format!("cannot apply `{}` to `{}` and `{}`", op.symbol(), self.name(&lt), self.name(&rt)));
            }
            let t = self.shallow(&lt);
            let ok = match op {
                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => matches!(t, Ty::Int(_) | Ty::IntVar(_) | Ty::Bool),
                _ => t.is_numeric(),
            };
            if !ok {
                return err(span, format!("cannot apply binary operator `{}` to type `{}`", op.symbol(), self.name(&t)));
            }
            return Ok(mk(K::Binary(op, Box::new(l), Box::new(r)), t, span));
        }
        let tr = match op {
            BinOp::Add => "Add",
            BinOp::Sub => "Sub",
            BinOp::Mul => "Mul",
            BinOp::Div => "Div",
            BinOp::Rem => "Rem",
            BinOp::BitAnd => "BitAnd",
            BinOp::BitOr => "BitOr",
            BinOp::BitXor => "BitXor",
            BinOp::Shl => "Shl",
            _ => "Shr",
        };
        let trd = self.cx.db.lang(tr);
        let mname = tr.to_ascii_lowercase();
        let mname = match mname.as_str() {
            "bitand" => "bitand",
            "bitor" => "bitor",
            "bitxor" => "bitxor",
            m => m,
        };
        let md = self.cx.db.child(trd, mname).unwrap();
        // `s + &t` with `t: String`: the right operand is coerced (`&String`
        // to `&str`) when no impl takes it as it is.
        let (r, rt) = self.coerce_operand(&lt, trd, r, rt)?;
        self.register(
            Pred::Trait(
                lt.clone(),
                TraitRef {
                    trait_: trd,
                    args: Rc::from([rt.clone()]),
                },
            ),
            span,
        );
        let out = Ty::Proj(trd, Rc::from([lt.clone(), rt.clone()]), Rc::from("Output"));
        self.select_obligations(false)?;
        let out = self.shallow(&out);
        Ok(mk(K::Call(Callee::Trait(md, Rc::from([lt, rt])), vec![l, r]), out, span))
    }

    /// Deref-coerces a reference operand to the first type the operator
    /// trait is implemented for.
    fn coerce_operand(&mut self, lt: &Ty, trd: DefId, r: tir::Expr, rt: Ty) -> Result<(tir::Expr, Ty)> {
        let rt = self.resolve(&rt);
        let Ty::Ref(m, inner) = &rt else { return Ok((r, rt)) };
        let tref = |t: Ty| TraitRef {
            trait_: trd,
            args: Rc::from([t]),
        };
        if lt.needs_infer() || self.evaluate(lt, &tref(rt.clone()), 0)? != super::infer::M::No {
            return Ok((r, rt));
        }
        let steps = self.autoderef_steps(inner, r.span)?;
        for (t, _) in steps.iter().skip(1) {
            let cand = Ty::Ref(*m, Rc::new(t.clone()));
            if self.evaluate(lt, &tref(cand.clone()), 0)? != super::infer::M::No {
                let c = self.coerce(r, &cand)?;
                return Ok((c, cand));
            }
        }
        Ok((r, rt))
    }

    fn check_assign(&mut self, a: &ast::Expr, b: &ast::Expr, span: Span) -> Result<tir::Expr> {
        // `_ = e`
        if let E::Path(p) = &a.kind {
            if p.as_ident().is_some_and(|n| &**n == "_") {
                let v = self.check_expr(b, None)?;
                return Ok(mk(K::Block(vec![tir::Stmt::Expr(v)], Box::new(unit_expr(span))), crate::ty::unit(), span));
            }
        }
        // `(a, b) = (x, y)`
        if let E::Tuple(targets) = &a.kind {
            let v = self.check_expr(b, None)?;
            let vt = self.resolve(&v.ty);
            let tmp = self.new_local(Rc::from("__hrc_tup"), vt.clone());
            let mut stmts = vec![tir::Stmt::Let(
                tir::Pat {
                    kind: tir::PatKind::Bind(tmp, None, None),
                    ty: vt.clone(),
                },
                Some(v),
                None,
            )];
            let elems: Vec<Ty> = match &vt {
                Ty::Tuple(ts) if ts.len() == targets.len() => ts.to_vec(),
                _ => return err(span, String::from("mismatched types in destructuring assignment")),
            };
            for (i, (t, ety)) in targets.iter().zip(elems.iter()).enumerate() {
                if let E::Path(p) = &t.kind {
                    if p.as_ident().is_some_and(|n| &**n == "_") {
                        continue;
                    }
                }
                let mut place = self.check_expr(t, None)?;
                self.mutate_place(&mut place);
                self.expect_eq(&place.ty, ety, t.span)?;
                let src = mk(K::Field(Box::new(mk(K::Local(tmp), vt.clone(), span)), i), ety.clone(), span);
                stmts.push(tir::Stmt::Expr(mk(K::Assign(Box::new(place), Box::new(src)), crate::ty::unit(), span)));
            }
            return Ok(mk(K::Block(stmts, Box::new(unit_expr(span))), crate::ty::unit(), span));
        }
        let mut place = self.check_expr(a, None)?;
        self.mutate_place(&mut place);
        let pt = place.ty.clone();
        let v = self.check_expr_coerce(b, &pt)?;
        Ok(mk(K::Assign(Box::new(place), Box::new(v)), crate::ty::unit(), span))
    }

    fn check_assign_op(&mut self, op: BinOp, a: &ast::Expr, b: &ast::Expr, span: Span) -> Result<tir::Expr> {
        let mut place = self.check_expr(a, None)?;
        self.mutate_place(&mut place);
        let lt = self.shallow(&place.ty);
        let lp = self.is_prim_operand(&lt);
        let rhint = if matches!(op, BinOp::Shl | BinOp::Shr) || !lp { None } else { Some(lt.clone()) };
        let r = self.check_expr(b, rhint.as_ref())?;
        let r = self.deref_prim(r);
        let rt = self.shallow(&r.ty);
        if lp && (self.is_prim_operand(&rt) || rt.is_var()) {
            if !matches!(op, BinOp::Shl | BinOp::Shr) && !self.unify(&lt, &rt) {
                let (lt, rt) = (self.resolve(&lt), self.resolve(&rt));
                return err(span, format!("cannot apply `{}=` to `{}` and `{}`", op.symbol(), self.name(&lt), self.name(&rt)));
            }
            return Ok(mk(K::AssignOp(op, Box::new(place), Box::new(r)), crate::ty::unit(), span));
        }
        let (tr, m) = match op {
            BinOp::Add => ("AddAssign", "add_assign"),
            BinOp::Sub => ("SubAssign", "sub_assign"),
            BinOp::Mul => ("MulAssign", "mul_assign"),
            BinOp::Div => ("DivAssign", "div_assign"),
            BinOp::Rem => ("RemAssign", "rem_assign"),
            BinOp::BitAnd => ("BitAndAssign", "bitand_assign"),
            BinOp::BitOr => ("BitOrAssign", "bitor_assign"),
            BinOp::BitXor => ("BitXorAssign", "bitxor_assign"),
            BinOp::Shl => ("ShlAssign", "shl_assign"),
            _ => ("ShrAssign", "shr_assign"),
        };
        let trd = self.cx.db.lang(tr);
        let md = self.cx.db.child(trd, m).unwrap();
        let (r, rt) = self.coerce_operand(&lt, trd, r, rt)?;
        self.register(
            Pred::Trait(
                lt.clone(),
                TraitRef {
                    trait_: trd,
                    args: Rc::from([rt.clone()]),
                },
            ),
            span,
        );
        let pa = mk(K::AddrOf(true, Box::new(place)), Ty::reference(true, lt.clone()), span);
        Ok(mk(K::Call(Callee::Trait(md, Rc::from([lt, rt])), vec![pa, r]), crate::ty::unit(), span))
    }

    fn check_cast(&mut self, x: &ast::Expr, t: &ast::Type, span: Span) -> Result<tir::Expr> {
        let target = self.lower_body_ty(t)?;
        let target = self.shallow(&target);
        let hint = match &target {
            Ty::Ref(..) | Ty::Ptr(..) | Ty::FnPtr(..) => Some(target.clone()),
            _ => None,
        };
        let v = self.check_expr(x, hint.as_ref())?;
        let vt = self.shallow(&v.ty);
        // Pointer casts that are coercions.
        if matches!(target, Ty::Ref(..) | Ty::FnPtr(..)) || (matches!(target, Ty::Ptr(..)) && matches!(vt, Ty::Ref(..))) {
            if let Ty::Ptr(m, inner) = &target {
                if let Ty::Ref(_, src) = &vt {
                    let (src, inner) = ((**src).clone(), (**inner).clone());
                    if self.unify(&src, &inner) {
                        let p = Ty::Ptr(*m, Rc::new(inner));
                        return Ok(mk(K::Cast(Box::new(v)), p, span));
                    }
                }
            }
            return self.coerce(v, &target);
        }
        if vt.is_var() && !matches!(vt, Ty::IntVar(_) | Ty::FloatVar(_)) {
            if !self.unify(&vt, &target) {
                return err(span, String::from("type annotations needed"));
            }
        }
        Ok(mk(K::Cast(Box::new(v)), target, span))
    }

    // ---- calls ----

    fn check_call(&mut self, f: &ast::Expr, args: &[ast::Expr], hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        let callee = self.check_expr(f, None)?;
        self.call_value(callee, args, hint, span)
    }

    pub fn call_value(&mut self, mut callee: tir::Expr, args: &[ast::Expr], hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        for _ in 0..8 {
            let t = self.shallow(&callee.ty);
            match &t {
                Ty::FnDef(def, substs) => {
                    let (def, substs) = (*def, substs.to_vec());
                    return self.call_def(def, substs, None, args, hint, span);
                }
                Ty::FnPtr(ps, r) => {
                    let ps = ps.to_vec();
                    let r = (**r).clone();
                    let a = self.check_args(&ps, args, false, span)?;
                    return Ok(mk(K::Call(Callee::Value(Box::new(callee)), a), r, span));
                }
                Ty::Closure(id) => {
                    let id = *id;
                    let (ps, r) = match self.callable_sig(&t) {
                        Some(s) => s,
                        None => return err(span, String::from("internal error: closure signature not known here")),
                    };
                    let a = self.check_args(&ps, args, false, span)?;
                    return Ok(mk(K::Call(Callee::Closure(Box::new(callee), id), a), r, span));
                }
                Ty::Dyn(d) if self.is_fn_trait(d.trait_) => {
                    let (ps, r) = self.callable_sig(&t).unwrap();
                    let mut a = vec![mk(K::AddrOf(false, Box::new(callee)), Ty::reference(false, t.clone()), span)];
                    a.extend(self.check_args(&ps, args, false, span)?);
                    return Ok(mk(K::Call(Callee::DynFn, a), r, span));
                }
                Ty::Ref(..) | Ty::Adt(..) => {
                    callee = self.deref_expr(callee, span)?;
                }
                Ty::Infer(_) => {
                    // A closure parameter with an `Fn` bound.
                    if let Some((ps, r)) = self.fn_obligation_sig(&t) {
                        let fnptr = Ty::FnPtr(ps.into(), Rc::new(r));
                        let _ = fnptr;
                    }
                    return err(span, String::from("type annotations needed: cannot call a value of unknown type"));
                }
                _ => return err(span, format!("expected function, found `{}`", self.name(&t))),
            }
        }
        err(span, String::from("expected function"))
    }

    /// A call to a function item (or constructor). `recv` is a receiver
    /// already checked (method calls).
    pub fn call_def(&mut self, def: DefId, substs: Vec<Ty>, recv: Option<tir::Expr>, args: &[ast::Expr], hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        let kind = self.cx.db.def(def).kind.clone();
        if let DefKind::Struct(_) | DefKind::Variant(_) = kind {
            let (ps, r) = self.ctor_sig(def, &substs).unwrap();
            if let Some(h) = hint {
                if let (Ty::Adt(a, _), Ty::Adt(b, _)) = (self.shallow(h), &r) {
                    if a == *b {
                        let h = h.clone();
                        self.unify(&h, &r);
                    }
                }
            }
            if ps.len() != args.len() {
                return err(span, format!("this struct takes {} field{} but {} were supplied", ps.len(), if ps.len() == 1 { "" } else { "s" }, args.len()));
            }
            let a = self.check_args(&ps, args, false, span)?;
            return Ok(mk(K::Adt(def, a), r, span));
        }
        let sig = self.cx.sig(def)?;
        let trait_item = matches!(self.cx.db.def(def).parent.map(|p| &self.cx.db.def(p).kind), Some(DefKind::Trait(_)));
        let mut ps: Vec<Ty> = sig.params.iter().map(|p| p.subst(&substs)).collect();
        let ret = sig.ret.subst(&substs);
        for p in &sig.preds {
            let p = p.subst(&substs);
            self.register(p, span);
        }
        let ret = self.shallow(&ret);
        // Expected inputs from the expected output (`Box::new(|x| ..)`).
        if let Some(h) = hint {
            let h = self.resolve(h);
            if !h.needs_infer() {
                if let Ty::Adt(..) = &h {
                    self.hint_from_output(&ret, &h);
                }
            }
        }
        let mut out = Vec::new();
        if let Some(r) = recv {
            out.push(r);
            ps.remove(0);
        }
        let n = if sig.variadic { args.len().max(ps.len()) } else { ps.len() };
        if args.len() != n || (sig.variadic && args.len() < ps.len()) {
            return err(
                span,
                format!(
                    "this function takes {} argument{} but {} argument{} supplied",
                    ps.len(),
                    if ps.len() == 1 { "" } else { "s" },
                    args.len(),
                    if args.len() == 1 { " was" } else { "s were" }
                ),
            );
        }
        let a = self.check_args(&ps, args, sig.variadic, span)?;
        out.extend(a);
        let callee = match kind {
            DefKind::ExternFn(..) => Callee::Item(def, substs.into()),
            _ if trait_item => Callee::Trait(def, substs.into()),
            _ => Callee::Item(def, substs.into()),
        };
        let ret = self.shallow(&ret);
        Ok(mk(K::Call(callee, out), ret, span))
    }

    /// Unifies variables in `formal` with the expected output only where
    /// the expected type has a trait object (to guide closure inference).
    fn hint_from_output(&mut self, formal: &Ty, expected: &Ty) {
        let f = self.shallow(formal);
        if let (Ty::Adt(a, x), Ty::Adt(b, y)) = (&f, expected) {
            if a == b && self.cx.db.is_lang(*a, "Box") {
                let xi = self.shallow(&x[0]);
                if let (Ty::Infer(v), Ty::Dyn(dy)) = (&xi, &y[0]) {
                    if self.is_fn_trait(dy.trait_) {
                        let _ = v;
                        self.closure_hints.push((xi.clone(), y[0].clone()));
                    }
                }
            }
        }
    }

    /// Checks call arguments against parameter types; closures last.
    pub fn check_args(&mut self, ps: &[Ty], args: &[ast::Expr], variadic: bool, span: Span) -> Result<Vec<tir::Expr>> {
        let mut out: Vec<Option<tir::Expr>> = (0..args.len()).map(|_| None).collect();
        for pass in 0..2 {
            for (i, a) in args.iter().enumerate() {
                let is_closure = matches!(a.kind, E::Closure { .. });
                if (pass == 0) == is_closure {
                    continue;
                }
                let x = match ps.get(i) {
                    Some(p) => self.check_expr_coerce(a, p)?,
                    None if variadic => self.check_expr(a, None)?,
                    None => return err(span, String::from("too many arguments")),
                };
                out[i] = Some(x);
            }
        }
        Ok(out.into_iter().map(|x| x.unwrap()).collect())
    }

    /// The signature an `Fn*` obligation gives a variable.
    pub fn fn_obligation_sig(&mut self, t: &Ty) -> Option<(Vec<Ty>, Ty)> {
        let t = self.peek(t);
        let mut params = None;
        let mut ret = None;
        for (o, _) in self.obligations.clone() {
            match o {
                Obligation::Pred(Pred::Trait(ty, tr)) if self.is_fn_trait(tr.trait_) => {
                    if self.peek(&ty) == t {
                        if let Some(a) = tr.args.first() {
                            if let Ty::Tuple(ts) = self.peek(a) {
                                params = Some(ts.to_vec());
                            }
                        }
                    }
                }
                Obligation::Pred(Pred::Proj(ty, tr, name, val)) if self.is_fn_trait(tr.trait_) && &*name == "Output" => {
                    if self.peek(&ty) == t {
                        ret = Some(val);
                    }
                }
                _ => {}
            }
        }
        if params.is_none() {
            for (k, v) in self.closure_hints.clone() {
                if self.peek(&k) == t {
                    if let Some(s) = self.callable_sig(&v) {
                        return Some(s);
                    }
                }
            }
        }
        let p = params?;
        let r = ret.unwrap_or_else(|| self.fresh());
        Some((p, r))
    }

    // ---- fields and indexing ----

    fn check_field(&mut self, x: &ast::Expr, name: &Name, span: Span) -> Result<tir::Expr> {
        let v = self.check_expr(x, None)?;
        let steps = self.autoderef_steps(&v.ty, span)?;
        for (i, (t, _)) in steps.iter().enumerate() {
            let t = self.shallow(t);
            let found = match &t {
                Ty::Adt(def, substs) => {
                    let info = self.cx.adt(*def)?;
                    let in_std = self.cx.db.def(self.def).in_std;
                    if info.is_enum {
                        None
                    } else {
                        info.field_index(0, name)
                            .filter(|fi| in_std || !info.variants[0].hidden[*fi])
                            .map(|fi| (fi, info.variants[0].fields[fi].1.subst(substs)))
                    }
                }
                Ty::Tuple(ts) => name.parse::<usize>().ok().filter(|k| *k < ts.len()).map(|k| (k, ts[k].clone())),
                _ => None,
            };
            if let Some((fi, fty)) = found {
                let base = self.apply_steps(v, &steps[..=i], false, span)?;
                let fty = self.shallow(&fty);
                return Ok(mk(K::Field(Box::new(base), fi), fty, span));
            }
        }
        let t = self.resolve(&v.ty);
        if t.is_var() {
            return err(span, String::from("type annotations needed: cannot access a field of a value of unknown type"));
        }
        err(span, format!("no field `{}` on type `{}`", name, self.name(&t)))
    }

    /// `start` is where builtin indexing reports panics, `span` (the
    /// bracket) where indexing through `Index` or `Deref` (a `Vec`) does.
    fn check_index(&mut self, b: &ast::Expr, i: &ast::Expr, start: Span, span: Span) -> Result<tir::Expr> {
        let base = self.check_expr(b, None)?;
        let idx = self.check_expr(i, None)?;
        let it = self.shallow(&idx.ty);
        let range_kind = match &it {
            Ty::Adt(d, _) => ["Range", "RangeInclusive", "RangeFrom", "RangeTo", "RangeToInclusive", "RangeFull"].iter().find(|n| self.cx.db.is_lang(*d, n)).copied(),
            _ => None,
        };
        let steps = self.autoderef_steps(&base.ty, span)?;
        for (k, (t, _)) in steps.iter().enumerate() {
            let t = self.shallow(t);
            let elem = match &t {
                Ty::Array(e, _) | Ty::Slice(e) => Some((**e).clone()),
                Ty::Str if range_kind.is_some() => Some(Ty::Str),
                _ => None,
            };
            if let Some(elem) = elem {
                let place = self.apply_steps(base, &steps[..=k], false, span)?;
                if let Some(rk) = range_kind {
                    let usize_t = Ty::usize();
                    if let Ty::Adt(_, a) = &it {
                        if let Some(x) = a.first() {
                            let x = x.clone();
                            self.expect_eq(&usize_t, &x, i.span)?;
                        }
                    }
                    let overloaded = steps[1..=k].iter().any(|(_, s)| matches!(s, Some(super::method::Step::Overloaded)));
                    let at = if overloaded { span } else { start };
                    return self.slice_index(place, idx, rk, &t, at);
                }
                if !self.unify(&it, &Ty::usize()) {
                    return err(i.span, format!("the type `{}` cannot be indexed by `{}`", self.name(&t), self.name(&it)));
                }
                if let Ty::Str = elem {
                    return err(span, String::from("the type `str` cannot be indexed by an integer"));
                }
                let overloaded = steps[1..=k].iter().any(|(_, s)| matches!(s, Some(super::method::Step::Overloaded)));
                let at = if overloaded { span } else { start };
                return Ok(mk(K::Index(Box::new(place), Box::new(idx)), elem, at));
            }
            // Overloaded `Index` (HashMap, user types).
            if let Ty::Adt(..) = &t {
                let index = self.cx.db.lang("Index");
                let tref = TraitRef {
                    trait_: index,
                    args: Rc::from([it.clone()]),
                };
                if self.evaluate(&t, &tref, 0)? != super::infer::M::No {
                    let place = self.apply_steps(base, &steps[..=k], false, span)?;
                    self.register(Pred::Trait(t.clone(), tref), span);
                    let out = Ty::Proj(index, Rc::from([t.clone(), it.clone()]), Rc::from("Output"));
                    self.select_obligations(false)?;
                    let out = self.resolve(&out);
                    let m = self.cx.db.child(index, "index").unwrap();
                    let r = mk(K::AddrOf(false, Box::new(place)), Ty::reference(false, t.clone()), span);
                    let call = mk(K::Call(Callee::Trait(m, Rc::from([t, it])), vec![r, idx]), Ty::reference(false, out.clone()), span);
                    return Ok(mk(K::Deref(Box::new(call)), out, span));
                }
            }
        }
        let t = self.resolve(&base.ty);
        err(span, format!("cannot index into a value of type `{}`", self.name(&t)))
    }

    /// `x[a..b]` on an array, slice or str place: a place of the unsized type.
    fn slice_index(&mut self, place: tir::Expr, idx: tir::Expr, kind: &str, t: &Ty, span: Span) -> Result<tir::Expr> {
        let unsized_ty = match t {
            Ty::Array(e, _) | Ty::Slice(e) => Ty::Slice(e.clone()),
            _ => Ty::Str,
        };
        // Bind the range to a temporary and take its fields.
        let rt = self.resolve(&idx.ty);
        let tmp = self.new_local(Rc::from("__hrc_range"), rt.clone());
        let r = mk(K::Local(tmp), rt.clone(), span);
        let field = |i: usize| mk(K::Field(Box::new(r.clone()), i), Ty::usize(), span);
        let one = mk(K::Int(1), Ty::usize(), span);
        let (lo, hi) = match kind {
            "Range" => (Some(field(0)), Some(field(1))),
            "RangeFrom" => (Some(field(0)), None),
            "RangeTo" => (None, Some(field(0))),
            "RangeInclusive" => (Some(field(0)), Some(mk(K::Binary(BinOp::Add, Box::new(field(1)), Box::new(one.clone())), Ty::usize(), span))),
            "RangeToInclusive" => (None, Some(mk(K::Binary(BinOp::Add, Box::new(field(0)), Box::new(one)), Ty::usize(), span))),
            _ => (None, None),
        };
        let fat = mk(K::Slice(Box::new(place), lo.map(Box::new), hi.map(Box::new)), Ty::reference(false, unsized_ty.clone()), span);
        let body = mk(K::Deref(Box::new(fat)), unsized_ty.clone(), span);
        let let_ = tir::Stmt::Let(
            tir::Pat {
                kind: tir::PatKind::Bind(tmp, None, None),
                ty: rt,
            },
            Some(idx),
            None,
        );
        Ok(mk(K::Block(vec![let_], Box::new(body)), unsized_ty, span))
    }

    fn check_range(&mut self, lo: Option<&ast::Expr>, hi: Option<&ast::Expr>, incl: bool, hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        let elem_hint = match hint {
            Some(Ty::Adt(_, a)) if !a.is_empty() => Some(a[0].clone()),
            _ => None,
        };
        let lo = match lo {
            Some(x) => Some(self.check_expr(x, elem_hint.as_ref())?),
            None => None,
        };
        let h2 = lo.as_ref().map(|l| l.ty.clone()).or(elem_hint);
        let hi = match hi {
            Some(x) => Some(self.check_expr(x, h2.as_ref())?),
            None => None,
        };
        if let (Some(l), Some(h)) = (&lo, &hi) {
            let (lt, ht) = (l.ty.clone(), h.ty.clone());
            self.expect_eq(&lt, &ht, span)?;
        }
        let name = match (&lo, &hi, incl) {
            (Some(_), Some(_), false) => "Range",
            (Some(_), Some(_), true) => "RangeInclusive",
            (Some(_), None, _) => "RangeFrom",
            (None, Some(_), false) => "RangeTo",
            (None, Some(_), true) => "RangeToInclusive",
            (None, None, _) => "RangeFull",
        };
        let def = self.cx.db.lang(name);
        let elem = lo.as_ref().or(hi.as_ref()).map(|x| x.ty.clone());
        let substs: Rc<[Ty]> = match &elem {
            Some(t) => Rc::from([t.clone()]),
            None => Rc::from([]),
        };
        let ty = Ty::Adt(def, substs.clone());
        if name == "RangeInclusive" {
            let new = self.cx.db.child_fn_of_impl(def, "new").ok_or_else(|| crate::Error::new(span, String::from("std has no RangeInclusive::new")))?;
            let args = vec![lo.unwrap(), hi.unwrap()];
            return Ok(mk(K::Call(Callee::Item(new, substs), args), ty, span));
        }
        let fields: Vec<tir::Expr> = lo.into_iter().chain(hi).collect();
        Ok(mk(K::Adt(def, fields), ty, span))
    }

    fn check_vec_lit(&mut self, es: &[ast::Expr], hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        let vec_def = self.cx.db.lang("Vec");
        let elem = match hint {
            Some(Ty::Adt(d, a)) if *d == vec_def => a[0].clone(),
            _ => self.fresh(),
        };
        let mut out = Vec::new();
        for x in es {
            let v = self.check_expr(x, Some(&elem))?;
            out.push(self.coerce(v, &elem)?);
        }
        let n = out.len() as u64;
        let arr = mk(K::Array(out), Ty::array(elem.clone(), n), span);
        let f = super::lower::std_fn(self.cx.db, &["vec", "from_array"]).ok_or_else(|| crate::Error::new(span, String::from("std has no vec::from_array")))?;
        let ty = Ty::Adt(vec_def, Rc::from([elem.clone()]));
        Ok(mk(K::Call(Callee::Item(f, Rc::from([elem, Ty::Const(n)])), vec![arr]), ty, span))
    }

    // ---- struct literals ----

    fn check_struct_lit(&mut self, path: &ast::Path, fields: &[(Name, ast::Expr, Span)], base: Option<&ast::Expr>, hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        let (ctor, substs) = self.resolve_struct_path(path, span)?;
        let (adt, vi) = self.adt_of(ctor);
        let info = self.cx.adt(adt)?;
        let ty = Ty::Adt(adt, substs.clone().into());
        if let Some(h) = hint {
            if let Ty::Adt(d, _) = self.shallow(h) {
                if d == adt {
                    let h = h.clone();
                    self.unify(&h, &ty);
                }
            }
        }
        let variant = &info.variants[vi];
        let mut values: Vec<Option<tir::Expr>> = (0..variant.fields.len()).map(|_| None).collect();
        let mut order = Vec::new();
        for (n, x, fspan) in fields {
            let fi = match info.field_index(vi, n) {
                Some(i) => i,
                None => {
                    return err(*fspan, format!("struct `{}` has no field named `{}`", variant.name, n));
                }
            };
            if values[fi].is_some() {
                return err(*fspan, format!("field `{}` specified more than once", n));
            }
            let fty = variant.fields[fi].1.subst(&substs);
            let fty = self.shallow(&fty);
            values[fi] = Some(self.check_expr_coerce(x, &fty)?);
            order.push(fi);
        }
        let mut stmts = Vec::new();
        if let Some(b) = base {
            let bv = self.check_expr_coerce(b, &ty)?;
            let tmp = self.new_local(Rc::from("__hrc_base"), ty.clone());
            stmts.push(tir::Stmt::Let(
                tir::Pat {
                    kind: tir::PatKind::Bind(tmp, None, None),
                    ty: ty.clone(),
                },
                Some(bv),
                None,
            ));
            for (fi, v) in values.iter_mut().enumerate() {
                if v.is_none() {
                    let fty = variant.fields[fi].1.subst(&substs);
                    *v = Some(mk(K::Field(Box::new(mk(K::Local(tmp), ty.clone(), span)), fi), fty, span));
                }
            }
        }
        let missing: Vec<&str> = values.iter().enumerate().filter(|(_, v)| v.is_none()).map(|(i, _)| &*variant.fields[i].0).collect();
        if !missing.is_empty() {
            let list: Vec<String> = missing.iter().map(|m| format!("`{}`", m)).collect();
            return err(span, format!("missing field{} {} in initializer of `{}`", if missing.len() == 1 { "" } else { "s" }, list.join(", "), variant.name));
        }
        let vals: Vec<tir::Expr> = values.into_iter().map(|v| v.unwrap()).collect();
        let adt_expr = mk(K::Adt(ctor, vals), ty.clone(), span);
        if stmts.is_empty() {
            Ok(adt_expr)
        } else {
            Ok(mk(K::Block(stmts, Box::new(adt_expr)), ty, span))
        }
    }

    /// The struct or variant named by a struct literal / pattern path.
    pub fn resolve_struct_path(&mut self, path: &ast::Path, span: Span) -> Result<(DefId, Vec<Ty>)> {
        let names: Vec<Name> = path.segs.iter().map(|s| s.name.clone()).collect();
        if !path.global && path.qself.is_none() {
            // `Self { .. }` / `Self::Variant { .. }`
            if &*names[0] == "Self" {
                if let Some(st) = self.scope.self_ty.clone() {
                    let st = self.resolve(&st.subst(&self.substs));
                    if let Ty::Adt(def, substs) = &st {
                        if names.len() == 1 {
                            return Ok((*def, substs.to_vec()));
                        }
                        if let Some(v) = self.cx.db.def(*def).children.iter().copied().find(|c| &*self.cx.db.def(*c).name == &*names[1]) {
                            return Ok((v, substs.to_vec()));
                        }
                    }
                }
            }
        }
        let (m, i) = self.cx.db.resolve_module_prefix(self.module, &names, path.global);
        if i < names.len() {
            if let Some(Res::Def(def)) = self.cx.db.lookup_seg(m, i, &names[i], Ns::Type) {
                match self.cx.db.def(def).kind.clone() {
                    DefKind::Struct(_) if i + 1 == names.len() => {
                        let substs = self.item_substs(def, Vec::new(), path.segs[i].args.as_ref(), span)?;
                        return Ok((def, substs));
                    }
                    DefKind::Enum(_) if i + 2 == names.len() => {
                        if let Some(v) = self.cx.db.def(def).children.iter().copied().find(|c| &*self.cx.db.def(*c).name == &*names[i + 1]) {
                            let args = path.segs[i].args.as_ref().or(path.segs[i + 1].args.as_ref());
                            let substs = self.item_substs(def, Vec::new(), args, span)?;
                            return Ok((v, substs));
                        }
                    }
                    DefKind::TypeAlias(..) if i + 1 == names.len() => {
                        let t = ast::Type {
                            kind: ast::TypeKind::Path(path.clone()),
                            span,
                        };
                        let ty = self.lower_body_ty(&t)?;
                        if let Ty::Adt(d, s) = self.resolve(&ty) {
                            return Ok((d, s.to_vec()));
                        }
                    }
                    _ => {}
                }
            }
            // A variant imported by `use Enum::*` / the prelude.
            if i + 1 == names.len() {
                if let Some(Res::Def(def)) = self.cx.db.lookup_seg(m, i, &names[i], Ns::Value) {
                    if let DefKind::Variant(_) = self.cx.db.def(def).kind {
                        let enum_ = self.cx.db.def(def).parent.unwrap();
                        let substs = self.item_substs(enum_, Vec::new(), path.segs[i].args.as_ref(), span)?;
                        return Ok((def, substs));
                    }
                }
            }
        }
        err(span, format!("cannot find struct, variant or union type `{}` in this scope", names.last().unwrap()))
    }

    // ---- blocks ----

    pub fn check_block(&mut self, b: &ast::Block, hint: Option<&Ty>) -> Result<tir::Expr> {
        let span = b.span;
        let saved_module = self.module;
        if let Some(m) = self.cx.db.module_of_block(b) {
            self.module = m;
        }
        self.scopes.push(Vec::new());
        let r = self.check_block_inner(b, hint, span);
        self.scopes.pop();
        self.module = saved_module;
        r
    }

    fn check_block_inner(&mut self, b: &ast::Block, hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        let mut stmts = Vec::new();
        let mut diverges = false;
        for s in &b.stmts {
            match s {
                ast::Stmt::Let { pat, ty, init, else_, span: lspan } => {
                    let declared = match ty {
                        Some(t) => Some(self.lower_body_ty(t)?),
                        None => None,
                    };
                    let init_v = match (init, &declared) {
                        (Some(x), Some(t)) => Some(self.check_expr_coerce(x, t)?),
                        (Some(x), None) => Some(self.check_expr(x, None)?),
                        (None, _) => None,
                    };
                    let pty = match (&declared, &init_v) {
                        (Some(t), _) => t.clone(),
                        (None, Some(v)) => v.ty.clone(),
                        (None, None) => self.fresh(),
                    };
                    if let Some(v) = &init_v {
                        if matches!(self.shallow(&v.ty), Ty::Never) {
                            diverges = true;
                        }
                    }
                    let else_v = match else_ {
                        Some(eb) => {
                            let x = self.check_block(eb, Some(&Ty::Never))?;
                            let xt = self.shallow(&x.ty);
                            if !matches!(xt, Ty::Never) {
                                return err(eb.span, String::from("`else` clause of `let...else` does not diverge"));
                            }
                            Some(x)
                        }
                        None => None,
                    };
                    let (p, binds) = self.check_pat_top(pat, &pty)?;
                    for (n, l) in binds {
                        self.bind_name(n, l);
                    }
                    let _ = lspan;
                    stmts.push(tir::Stmt::Let(p, init_v, else_v));
                }
                ast::Stmt::Expr(x, _semi) => {
                    let v = self.check_expr(x, None)?;
                    if matches!(self.shallow(&v.ty), Ty::Never) {
                        diverges = true;
                    }
                    stmts.push(tir::Stmt::Expr(v));
                }
                ast::Stmt::Item(_) | ast::Stmt::Empty => {}
            }
        }
        let tail = match &b.expr {
            Some(x) => {
                let v = self.check_expr(x, hint)?;
                v
            }
            None => {
                let t = if diverges { Ty::Never } else { crate::ty::unit() };
                mk(K::Unit, t, span)
            }
        };
        let ty = tail.ty.clone();
        if stmts.is_empty() {
            return Ok(tail);
        }
        Ok(mk(K::Block(stmts, Box::new(tail)), ty, span))
    }

    fn check_labeled_block(&mut self, b: &ast::Block, label: &Name, hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        let id = self.cx.new_label();
        let bt = match hint {
            Some(h) if !h.is_var() => h.clone(),
            _ => self.fresh(),
        };
        self.loops.push(LoopCtx {
            label: Some(label.clone()),
            id,
            break_ty: Some(bt.clone()),
            is_block: true,
            has_break: false,
        });
        let body = self.check_block(b, Some(&bt));
        let lc = self.loops.pop().unwrap();
        let body = body?;
        let body = if matches!(self.shallow(&body.ty), Ty::Never) && lc.has_break {
            body
        } else {
            self.coerce(body, &bt)?
        };
        Ok(mk(K::LabeledBlock(Box::new(body), id), bt, span))
    }

    // ---- control flow ----

    /// The result type of branches: coerced to the expected type when it is
    /// known, otherwise to the first non-diverging branch.
    pub fn join(&mut self, branches: Vec<tir::Expr>, expected: Option<&Ty>, span: Span) -> Result<(Vec<tir::Expr>, Ty)> {
        let expected = expected.map(|e| self.shallow(e)).filter(|e| !e.is_var() && !matches!(e, Ty::Never));
        let mut target: Option<Ty> = expected.clone();
        let mut out = Vec::new();
        for b in branches {
            let bt = self.shallow(&b.ty);
            if matches!(bt, Ty::Never) {
                out.push(b);
                continue;
            }
            match &target {
                None => {
                    target = Some(b.ty.clone());
                    out.push(b);
                }
                Some(t) => {
                    let t = t.clone();
                    let c = self.coerce(b, &t)?;
                    out.push(c);
                }
            }
        }
        let _ = span;
        Ok((out, target.unwrap_or(Ty::Never)))
    }

    fn contains_let(e: &ast::Expr) -> bool {
        match &e.kind {
            E::Let(..) => true,
            E::Binary(BinOp::And, a, b) => Self::contains_let(a) || Self::contains_let(b),
            E::Paren(a) => Self::contains_let(a),
            _ => false,
        }
    }

    fn flatten_and<'e>(e: &'e ast::Expr, out: &mut Vec<&'e ast::Expr>) {
        match &e.kind {
            E::Binary(BinOp::And, a, b) if Self::contains_let(e) => {
                Self::flatten_and(a, out);
                Self::flatten_and(b, out);
            }
            _ => out.push(e),
        }
    }

    fn check_if(&mut self, c: &ast::Expr, then: &ast::Block, els: Option<&ast::Expr>, hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        if Self::contains_let(c) {
            return self.check_if_let(c, then, els, hint, span);
        }
        let cond = self.check_bool(c)?;
        match els {
            None => {
                let t = self.check_block(then, Some(&crate::ty::unit()))?;
                let t = if matches!(self.shallow(&t.ty), Ty::Never) {
                    t
                } else {
                    self.coerce(t, &crate::ty::unit())?
                };
                Ok(mk(K::If(Box::new(cond), Box::new(t), Box::new(unit_expr(span))), crate::ty::unit(), span))
            }
            Some(e) => {
                let t = self.check_block(then, hint)?;
                let h2 = hint.cloned().or_else(|| Some(t.ty.clone()));
                let f = self.check_expr(e, h2.as_ref())?;
                let (mut bs, ty) = self.join(vec![t, f], hint, span)?;
                let f = bs.pop().unwrap();
                let t = bs.pop().unwrap();
                Ok(mk(K::If(Box::new(cond), Box::new(t), Box::new(f)), ty, span))
            }
        }
    }

    /// `if let` chains: a labeled block whose success path breaks out with
    /// the `then` value; falling through yields the `else` value.
    fn check_if_let(&mut self, c: &ast::Expr, then: &ast::Block, els: Option<&ast::Expr>, hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        let label = self.cx.new_label();
        let mut parts = Vec::new();
        Self::flatten_and(c, &mut parts);
        let rt = match (els, hint) {
            (None, _) => crate::ty::unit(),
            (Some(_), Some(h)) if !h.is_var() => h.clone(),
            _ => self.fresh(),
        };
        let depth = self.scopes.len();
        let success = self.chain(&parts, &mut |f: &mut Self| {
            let v = f.check_block(then, Some(&rt))?;
            let v = f.coerce(v, &rt)?;
            Ok(mk(K::Break(label, Box::new(v)), Ty::Never, span))
        })?;
        self.scopes.truncate(depth);
        let else_v = match els {
            Some(e) => {
                let v = self.check_expr(e, Some(&rt))?;
                self.coerce(v, &rt)?
            }
            None => unit_expr(span),
        };
        let body = mk(K::Block(vec![tir::Stmt::Expr(success)], Box::new(else_v)), rt.clone(), span);
        Ok(mk(K::LabeledBlock(Box::new(body), label), rt, span))
    }

    /// Checks a chain of conditions; `inner` runs when all succeed.
    fn chain(&mut self, parts: &[&ast::Expr], inner: &mut dyn FnMut(&mut Self) -> Result<tir::Expr>) -> Result<tir::Expr> {
        let Some((first, rest)) = parts.split_first() else {
            return inner(self);
        };
        let span = first.span;
        match &first.kind {
            E::Let(pat, x) => {
                let v = self.check_expr(x, None)?;
                let vt = v.ty.clone();
                self.scopes.push(Vec::new());
                let (p, binds) = self.check_pat_top(pat, &vt)?;
                for (n, l) in binds {
                    self.bind_name(n, l);
                }
                let body = self.chain(rest, inner)?;
                self.scopes.pop();
                let arms = vec![
                    tir::Arm {
                        pat: p,
                        guard: None,
                        body,
                    },
                    tir::Arm {
                        pat: tir::Pat {
                            kind: tir::PatKind::Wild,
                            ty: vt,
                        },
                        guard: None,
                        body: unit_expr(span),
                    },
                ];
                Ok(mk(K::Match(Box::new(v), arms), crate::ty::unit(), span))
            }
            _ => {
                let c = self.check_bool(first)?;
                let body = self.chain(rest, inner)?;
                Ok(mk(K::If(Box::new(c), Box::new(body), Box::new(unit_expr(span))), crate::ty::unit(), span))
            }
        }
    }

    fn check_match(&mut self, s: &ast::Expr, arms: &[ast::Arm], hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        let scrut = self.check_expr(s, None)?;
        let st = scrut.ty.clone();
        let mut pats = Vec::new();
        let mut bodies = Vec::new();
        let mut guards = Vec::new();
        let rt_hint = hint.cloned();
        let mut first_ty: Option<Ty> = None;
        for a in arms {
            self.scopes.push(Vec::new());
            let (p, binds) = self.check_pat_top(&a.pat, &st)?;
            for (n, l) in binds {
                self.bind_name(n, l);
            }
            let g = match &a.guard {
                Some(g) => {
                    if Self::contains_let(g) {
                        return err(g.span, String::from("`if let` guards are not supported"));
                    }
                    Some(self.check_bool(g)?)
                }
                None => None,
            };
            let h = rt_hint.clone().or_else(|| first_ty.clone());
            let b = self.check_expr(&a.body, h.as_ref())?;
            if first_ty.is_none() && !matches!(self.shallow(&b.ty), Ty::Never) {
                first_ty = Some(b.ty.clone());
            }
            self.scopes.pop();
            pats.push(p);
            guards.push(g);
            bodies.push(b);
        }
        let (bodies, ty) = self.join(bodies, hint, span)?;
        let arms: Vec<tir::Arm> = pats
            .into_iter()
            .zip(guards)
            .zip(bodies)
            .map(|((pat, guard), body)| tir::Arm { pat, guard, body })
            .collect();
        Ok(mk(K::Match(Box::new(scrut), arms), ty, span))
    }

    fn check_while(&mut self, c: &ast::Expr, b: &ast::Block, label: Option<Name>, span: Span) -> Result<tir::Expr> {
        let id = self.cx.new_label();
        self.loops.push(LoopCtx {
            label,
            id,
            break_ty: None,
            is_block: false,
            has_break: false,
        });
        let r = (|| -> Result<tir::Expr> {
            let unit = crate::ty::unit();
            if Self::contains_let(c) {
                let mut parts = Vec::new();
                Self::flatten_and(c, &mut parts);
                let depth = self.scopes.len();
                let success = self.chain(&parts, &mut |f: &mut Self| {
                    let v = f.check_block(b, Some(&unit))?;
                    Ok(mk(K::Block(vec![tir::Stmt::Expr(v)], Box::new(mk(K::Continue(id), Ty::Never, span))), Ty::Never, span))
                })?;
                self.scopes.truncate(depth);
                let body = mk(
                    K::Block(vec![tir::Stmt::Expr(success)], Box::new(mk(K::Break(id, Box::new(unit_expr(span))), Ty::Never, span))),
                    Ty::Never,
                    span,
                );
                return Ok(body);
            }
            let cond = self.check_bool(c)?;
            let v = self.check_block(b, Some(&unit))?;
            let brk = mk(K::Break(id, Box::new(unit_expr(span))), Ty::Never, span);
            Ok(mk(K::If(Box::new(cond), Box::new(v), Box::new(brk)), unit.clone(), span))
        })();
        self.loops.pop();
        let body = r?;
        Ok(mk(K::Loop(Box::new(body), id), crate::ty::unit(), span))
    }

    fn check_loop(&mut self, b: &ast::Block, label: Option<Name>, hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        let id = self.cx.new_label();
        let bt = match hint {
            Some(h) if !h.is_var() => h.clone(),
            _ => self.fresh(),
        };
        self.loops.push(LoopCtx {
            label,
            id,
            break_ty: Some(bt.clone()),
            is_block: false,
            has_break: false,
        });
        let body = self.check_block(b, Some(&crate::ty::unit()));
        let lc = self.loops.pop().unwrap();
        let body = body?;
        let ty = if lc.has_break { bt } else { Ty::Never };
        Ok(mk(K::Loop(Box::new(body), id), ty, span))
    }

    fn find_loop(&mut self, label: Option<&Name>, span: Span, is_continue: bool) -> Result<u32> {
        for l in self.loops.iter_mut().rev() {
            let matches = match label {
                Some(n) => l.label.as_ref() == Some(n),
                None => !l.is_block,
            };
            if matches {
                if is_continue && l.is_block {
                    return err(span, String::from("`continue` pointing to a labeled block"));
                }
                if !is_continue {
                    l.has_break = true;
                }
                return Ok(l.id);
            }
        }
        match label {
            Some(n) => err(span, format!("use of undeclared label `'{}`", n)),
            None => err(span, format!("`{}` outside of a loop", if is_continue { "continue" } else { "break" })),
        }
    }

    fn check_break(&mut self, label: Option<&Name>, v: Option<&ast::Expr>, span: Span) -> Result<tir::Expr> {
        let id = self.find_loop(label, span, false)?;
        let bt = self.loops.iter().find(|l| l.id == id).and_then(|l| l.break_ty.clone());
        let val = match (v, bt) {
            (Some(x), Some(t)) => self.check_expr_coerce(x, &t)?,
            (Some(x), None) => return err(x.span, String::from("`break` with value from a `while` or `for` loop")),
            (None, Some(t)) => {
                let u = unit_expr(span);
                if !self.unify(&t, &crate::ty::unit()) {
                    return err(span, String::from("mismatched types: `break` without a value in a loop that has a value"));
                }
                u
            }
            (None, None) => unit_expr(span),
        };
        Ok(mk(K::Break(id, Box::new(val)), Ty::Never, span))
    }

    /// `e?` on `Option` and `Result`.
    fn check_try(&mut self, x: &ast::Expr, span: Span) -> Result<tir::Expr> {
        let v = self.check_expr(x, None)?;
        let t = self.resolve(&v.ty);
        let (is_option, args) = match &t {
            Ty::Adt(d, a) if self.cx.db.is_lang(*d, "Option") => (true, a.clone()),
            Ty::Adt(d, a) if self.cx.db.is_lang(*d, "Result") => (false, a.clone()),
            _ => return err(span, format!("the `?` operator can only be applied to values that implement `Try`; `{}` does not", self.name(&t))),
        };
        let _ = args;
        let stashed = self.stash(v);
        let ok_name = "__hrc_v";
        let err_name = "__hrc_e";
        let x = if is_option {
            d::match_(
                stashed,
                vec![
                    d::arm(
                        ast::Pat {
                            kind: ast::PatKind::TupleStruct(d::std_path(&["option", "Option", "Some"], span), vec![d::ident_pat(ok_name, false, span)]),
                            span,
                        },
                        None,
                        d::var(ok_name, span),
                    ),
                    d::arm(
                        ast::Pat {
                            kind: ast::PatKind::Path(d::std_path(&["option", "Option", "None"], span)),
                            span,
                        },
                        None,
                        d::e(E::Return(Some(Box::new(d::none(span)))), span),
                    ),
                ],
                span,
            )
        } else {
            let conv = d::call(d::std_expr(&["convert", "From", "from"], span), vec![d::var(err_name, span)], span);
            let ret = d::call(d::std_expr(&["result", "Result", "Err"], span), vec![conv], span);
            d::match_(
                stashed,
                vec![
                    d::arm(
                        ast::Pat {
                            kind: ast::PatKind::TupleStruct(d::std_path(&["result", "Result", "Ok"], span), vec![d::ident_pat(ok_name, false, span)]),
                            span,
                        },
                        None,
                        d::var(ok_name, span),
                    ),
                    d::arm(
                        ast::Pat {
                            kind: ast::PatKind::TupleStruct(d::std_path(&["result", "Result", "Err"], span), vec![d::ident_pat(err_name, false, span)]),
                            span,
                        },
                        None,
                        d::e(E::Return(Some(Box::new(ret))), span),
                    ),
                ],
                span,
            )
        };
        self.check_expr(&x, None)
    }

    // ---- closures ----

    fn check_closure(&mut self, params: &[ast::ClosureParam], ret: Option<&ast::Type>, body: &ast::Expr, is_move: bool, hint: Option<&Ty>, span: Span) -> Result<tir::Expr> {
        let expected = match hint {
            Some(h) => {
                let h = self.shallow(h);
                match &h {
                    Ty::Infer(_) => self.fn_obligation_sig(&h),
                    Ty::FnPtr(..) | Ty::Dyn(_) => self.callable_sig(&h),
                    _ => None,
                }
            }
            None => None,
        };
        let id = self.cx.closures.len() as u32;
        self.cx.closures.push(None);
        let mut ptys = Vec::new();
        for (i, p) in params.iter().enumerate() {
            let t = match &p.ty {
                Some(t) => self.lower_body_ty(t)?,
                None => match &expected {
                    Some((ps, _)) if i < ps.len() => ps[i].clone(),
                    _ => self.fresh(),
                },
            };
            ptys.push(t);
        }
        if let Some((ps, _)) = &expected {
            for (a, b) in ptys.iter().zip(ps.iter()) {
                let (a, b) = (a.clone(), b.clone());
                self.unify(&a, &b);
            }
        }
        let rt = match ret {
            Some(t) => self.lower_body_ty(t)?,
            None => match &expected {
                Some((_, r)) => r.clone(),
                None => self.fresh(),
            },
        };
        self.closure_sigs.insert(id, (ptys.clone(), rt.clone()));
        let saved_ret = core::mem::replace(&mut self.ret_ty, rt.clone());
        let saved_loops = core::mem::take(&mut self.loops);
        self.closures.push(ClosureCtx {
            id,
            captures: Vec::new(),
            is_move,
            saved_ret,
            saved_loops,
        });
        self.scopes.push(Vec::new());
        let r = (|| -> Result<(Vec<tir::LocalId>, Vec<tir::Stmt>, tir::Expr)> {
            let mut pids = Vec::new();
            let mut prologue = Vec::new();
            for (p, t) in params.iter().zip(ptys.iter()) {
                match &p.pat.kind {
                    ast::PatKind::Ident { name, by_ref: false, sub: None, .. } => {
                        let l = self.new_local(name.clone(), t.clone());
                        self.bind_name(name.clone(), l);
                        pids.push(l);
                    }
                    _ => {
                        let l = self.new_local(Rc::from("__arg"), t.clone());
                        pids.push(l);
                        let (pat, binds) = self.check_pat_top(&p.pat, t)?;
                        for (n, id) in binds {
                            self.bind_name(n, id);
                        }
                        prologue.push(tir::Stmt::Let(pat, Some(mk(K::Local(l), t.clone(), p.pat.span)), None));
                    }
                }
            }
            let b = self.check_expr_coerce(body, &rt)?;
            Ok((pids, prologue, b))
        })();
        self.scopes.pop();
        let cc = self.closures.pop().unwrap();
        self.ret_ty = cc.saved_ret;
        self.loops = cc.saved_loops;
        let (pids, prologue, b) = r?;
        let b = if prologue.is_empty() {
            b
        } else {
            let t = b.ty.clone();
            mk(K::Block(prologue, Box::new(b)), t, span)
        };
        // Capture expressions, evaluated where the closure is created.
        let mut caps = Vec::new();
        let mut cap_tys = Vec::new();
        for (l, by_ref) in &cc.captures {
            let place = self.local_place(*l, span);
            let t = place.ty.clone();
            if *by_ref {
                cap_tys.push((Ty::reference(true, t.clone()), true));
                caps.push(mk(K::AddrOf(true, Box::new(place)), Ty::reference(true, t), span));
            } else {
                cap_tys.push((t.clone(), false));
                caps.push(place);
            }
        }
        self.closure_bodies.push((
            id,
            tir::Closure {
                params: pids,
                ret: rt,
                captures: cap_tys,
                body: b,
                func: 0,
            },
        ));
        Ok(mk(K::Closure(id, caps), Ty::Closure(id), span))
    }
}

pub fn parse_float(text: &str) -> Option<f64> {
    text.parse::<f64>().ok()
}

impl crate::resolve::Db {
    /// An inherent associated function of a type (`RangeInclusive::new`).
    pub fn child_fn_of_impl(&self, ty_def: DefId, name: &str) -> Option<DefId> {
        for &imp in &self.impls {
            if let DefKind::Impl(i) = &self.def(imp).kind {
                if i.trait_.is_some() {
                    continue;
                }
                if let ast::TypeKind::Path(p) = &i.self_ty.kind {
                    if &**p.last() == &*self.def(ty_def).name {
                        if let Some(c) = self.child(imp, name) {
                            return Some(c);
                        }
                    }
                }
            }
        }
        None
    }
}
