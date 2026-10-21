//! Method lookup with auto-deref and auto-ref, coercions, and turning
//! overloaded `Deref`/`Index` places into their `Mut` versions.

use super::expr::mk;
use super::infer::{FnCtx, Selection, M};
use super::{Pred, TraitRef};
use crate::ast::{self, Name};
use crate::resolve::{DefId, DefKind};
use crate::tir::{self, Callee, ExprKind as K};
use crate::ty::{IntTy, Ty};
use crate::{err, Result, Span};
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// `*` of a reference or `Box`.
    Builtin,
    /// `Deref::deref`.
    Overloaded,
    /// `[T; N]` to `[T]`.
    Unsize,
}

struct Pick {
    def: DefId,
    /// Substs of the impl (inherent) or [Self, trait args] (trait).
    parent: Vec<Ty>,
    step: usize,
    autoref: Option<bool>,
}

impl<'c, 'a> FnCtx<'c, 'a> {
    /// The types reachable by dereferencing, with how each was reached.
    pub fn autoderef_steps(&mut self, t: &Ty, span: Span) -> Result<Vec<(Ty, Option<Step>)>> {
        let mut cur = self.shallow(t);
        let mut out = vec![(cur.clone(), None)];
        for _ in 0..10 {
            let next = match &cur {
                Ty::Ref(_, inner) => Some(((**inner).clone(), Step::Builtin)),
                Ty::Adt(d, a) if self.cx.db.is_lang(*d, "Box") => Some((a[0].clone(), Step::Builtin)),
                Ty::Adt(..) => {
                    let deref = self.cx.db.lang("Deref");
                    let tref = TraitRef {
                        trait_: deref,
                        args: Rc::from([]),
                    };
                    if self.evaluate(&cur, &tref, 0)? == M::No {
                        None
                    } else {
                        match self.select(&cur, &tref, span)? {
                            Selection::Impl(..) => {
                                let target = Ty::Proj(deref, Rc::from([cur.clone()]), Rc::from("Target"));
                                let target = self.resolve(&target);
                                Some((target, Step::Overloaded))
                            }
                            _ => None,
                        }
                    }
                }
                _ => None,
            };
            match next {
                Some((t, s)) => {
                    cur = self.shallow(&t);
                    out.push((cur.clone(), Some(s)));
                }
                None => break,
            }
        }
        if let Ty::Array(e, _) = &cur {
            out.push((Ty::Slice(e.clone()), Some(Step::Unsize)));
        }
        Ok(out)
    }

    /// Applies the deref steps (the first entry is the expression itself).
    pub fn apply_steps(&mut self, mut e: tir::Expr, steps: &[(Ty, Option<Step>)], mutable: bool, span: Span) -> Result<tir::Expr> {
        for (t, s) in steps.iter().skip(1) {
            let t = t.clone();
            e = match s {
                Some(Step::Builtin) => mk(K::Deref(Box::new(e)), t, span),
                Some(Step::Overloaded) => {
                    let (tr, m) = if mutable { ("DerefMut", "deref_mut") } else { ("Deref", "deref") };
                    let trd = self.cx.db.lang(tr);
                    let md = self.cx.db.child(trd, m).unwrap();
                    let st = e.ty.clone();
                    if mutable {
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
                    }
                    let r = mk(K::AddrOf(mutable, Box::new(e)), Ty::reference(mutable, st.clone()), span);
                    let call = mk(K::Call(Callee::Trait(md, Rc::from([st])), vec![r]), Ty::reference(mutable, t.clone()), span);
                    mk(K::Deref(Box::new(call)), t, span)
                }
                Some(Step::Unsize) => {
                    let at = e.ty.clone();
                    let r = mk(K::AddrOf(mutable, Box::new(e)), Ty::reference(mutable, at), span);
                    let s = mk(K::Unsize(Box::new(r)), Ty::reference(mutable, t.clone()), span);
                    mk(K::Deref(Box::new(s)), t, span)
                }
                None => e,
            };
        }
        Ok(e)
    }

    pub fn check_method_call(
        &mut self,
        recv: &ast::Expr,
        name: &Name,
        gargs: Option<&ast::GenericArgs>,
        args: &[ast::Expr],
        hint: Option<&Ty>,
        span: Span,
    ) -> Result<tir::Expr> {
        let r = self.check_expr(recv, None)?;
        let mut rt = self.shallow(&r.ty);
        if let Ty::IntVar(_) = rt {
            self.unify(&rt, &Ty::Int(IntTy::I32));
            rt = self.shallow(&rt);
        }
        if let Ty::FloatVar(_) = rt {
            self.unify(&rt, &Ty::Float(crate::ty::FloatTy::F64));
            rt = self.shallow(&rt);
        }
        if rt.is_var() {
            self.select_obligations(false)?;
            rt = self.shallow(&rt);
            if rt.is_var() {
                return err(recv.span, format!("type annotations needed: cannot call method `{}` on a value of unknown type", name));
            }
        }
        let steps = self.autoderef_steps(&rt, span)?;
        let mut pick = None;
        'outer: for (k, (t, _)) in steps.iter().enumerate() {
            let t = self.resolve(t);
            if t.is_var() {
                break;
            }
            for autoref in [None, Some(false), Some(true)] {
                // Unsized steps must be borrowed.
                if autoref.is_none() && t.is_unsized() {
                    continue;
                }
                let recv_ty = match autoref {
                    None => t.clone(),
                    Some(m) => Ty::reference(m, t.clone()),
                };
                if let Some(p) = self.probe_inherent(&recv_ty, name, k, autoref)? {
                    pick = Some(p);
                    break 'outer;
                }
                if let Some(p) = self.probe_trait(&recv_ty, name, k, autoref, span)? {
                    pick = Some(p);
                    break 'outer;
                }
            }
        }
        let pick = match pick {
            Some(p) => p,
            None => {
                let t = self.resolve(&r.ty);
                // A field holding a closure?
                if let Ty::Adt(d, _) = self.peel_refs(&t) {
                    if let Ok(info) = self.cx.adt(d) {
                        if !info.is_enum && info.field_index(0, name).is_some() {
                            return err(span, format!("no method named `{}` found; to call the function stored in `{}`, surround the field access with parentheses: `(x.{})(...)`", name, name, name));
                        }
                    }
                }
                return err(span, format!("no method named `{}` found for `{}` in the current scope", name, self.name(&t)));
            }
        };
        // Build the receiver.
        let mutable = pick.autoref == Some(true);
        let mut place = self.apply_steps(r, &steps[..=pick.step], mutable, span)?;
        if mutable {
            self.mutate_place(&mut place);
        }
        let recv_expr = match pick.autoref {
            None => place,
            Some(m) => {
                let t = place.ty.clone();
                mk(K::AddrOf(m, Box::new(place)), Ty::reference(m, t), span)
            }
        };
        let substs = self.method_substs(pick.def, pick.parent, gargs, span)?;
        let sig = self.cx.sig(pick.def)?;
        let rty = sig.params[0].subst(&substs);
        let rexp = recv_expr.ty.clone();
        if !self.unify(&rty, &rexp) {
            let (a, b) = (self.resolve(&rty), self.resolve(&rexp));
            return err(span, format!("mismatched receiver: expected `{}`, found `{}`", self.name(&a), self.name(&b)));
        }
        self.call_def(pick.def, substs, Some(recv_expr), args, hint, span)
    }

    fn peel_refs(&mut self, t: &Ty) -> Ty {
        let mut t = self.shallow(t);
        while let Ty::Ref(_, inner) = &t {
            t = self.shallow(inner);
        }
        t
    }

    fn method_substs(&mut self, def: DefId, mut parent: Vec<Ty>, gargs: Option<&ast::GenericArgs>, span: Span) -> Result<Vec<Ty>> {
        let g = self.cx.generics_of(def);
        while parent.len() < g.own_start {
            let v = self.fresh();
            parent.push(v);
        }
        let given: Vec<ast::Type> = gargs.map(|a| a.types.clone()).unwrap_or_default();
        let own = g.count() - g.own_start;
        if given.len() > own {
            return err(span, format!("method takes {} generic arguments but {} were supplied", own, given.len()));
        }
        for i in 0..own {
            let t = match given.get(i) {
                Some(t) => self.lower_body_ty(t)?,
                None => self.fresh(),
            };
            parent.push(t);
        }
        Ok(parent)
    }

    fn inherent_methods(&mut self, name: &str) -> Result<Vec<(DefId, DefId)>> {
        let mut out = Vec::new();
        for imp in self.cx.inherent_impls()? {
            if let Some(c) = self.cx.db.child(imp, name) {
                if let DefKind::Fn(f) = &self.cx.db.def(c).kind {
                    if f.self_param.is_some() {
                        out.push((imp, c));
                    }
                }
            }
        }
        Ok(out)
    }

    fn probe_inherent(&mut self, recv_ty: &Ty, name: &str, step: usize, autoref: Option<bool>) -> Result<Option<Pick>> {
        let mut cands = Vec::new();
        for (imp, m) in self.inherent_methods(name)? {
            let sig = self.cx.sig(m)?;
            let mut binds = vec![None; sig.generics.count()];
            let r = self.match_ty(&sig.params[0], recv_ty, &mut binds);
            if r == M::No {
                continue;
            }
            // Bounds of the impl must hold (`impl<T: Display> W<T>`).
            let h = self.cx.impl_header(imp)?;
            let mut ok = r;
            for p in &h.preds {
                if let Pred::Trait(t, tr) = p {
                    let t2 = subst_opt(t, &binds);
                    let tr2 = TraitRef {
                        trait_: tr.trait_,
                        args: tr.args.iter().map(|a| subst_opt(a, &binds)).collect::<Vec<_>>().into(),
                    };
                    ok = ok.and(self.evaluate(&t2, &tr2, 1)?);
                }
            }
            if ok == M::No {
                continue;
            }
            let n = h.generics.count();
            cands.push((m, binds[..n].to_vec(), ok));
        }
        if cands.is_empty() {
            return Ok(None);
        }
        if cands.len() > 1 {
            let yes: Vec<usize> = (0..cands.len()).filter(|&i| cands[i].2 == M::Yes).collect();
            if yes.len() == 1 {
                let c = cands.swap_remove(yes[0]);
                cands = vec![c];
            }
        }
        let (m, binds, _) = cands.swap_remove(0);
        let parent: Vec<Ty> = binds.into_iter().map(|b| b.unwrap_or_else(|| self.fresh())).collect();
        Ok(Some(Pick { def: m, parent, step, autoref }))
    }

    fn probe_trait(&mut self, recv_ty: &Ty, name: &str, step: usize, autoref: Option<bool>, span: Span) -> Result<Option<Pick>> {
        let methods = self.cx.db.trait_methods.get(name).cloned().unwrap_or_default();
        let mut cands: Vec<(DefId, Vec<Ty>, M)> = Vec::new();
        for m in methods {
            let sig = self.cx.sig(m)?;
            if sig.self_kind.is_none() {
                continue;
            }
            let tr = self.cx.db.def(m).parent.unwrap();
            if !self.trait_visible(tr) {
                continue;
            }
            let tg = self.cx.generics_of(tr);
            let mut binds = vec![None; sig.generics.count()];
            if self.match_ty(&sig.params[0], recv_ty, &mut binds) == M::No {
                continue;
            }
            let self_ty = match &binds[0] {
                Some(t) => t.clone(),
                None => continue,
            };
            let self_ty = self.resolve(&self_ty);
            // Trait parameters the receiver does not determine are inferred.
            let mut targs: Vec<Ty> = Vec::new();
            for i in 1..tg.count() {
                let t = match binds[i].clone() {
                    Some(t) => t,
                    None => self.fresh(),
                };
                targs.push(t);
            }
            let tref = TraitRef {
                trait_: tr,
                args: targs.clone().into(),
            };
            let ok = self.evaluate(&self_ty, &tref, 0)?;
            if ok == M::No {
                continue;
            }
            let mut parent = vec![self_ty];
            parent.extend(targs);
            cands.push((m, parent, ok));
        }
        if cands.is_empty() {
            return Ok(None);
        }
        if cands.len() > 1 {
            let yes: Vec<usize> = (0..cands.len()).filter(|&i| cands[i].2 == M::Yes).collect();
            if yes.len() == 1 {
                let c = cands.swap_remove(yes[0]);
                cands = vec![c];
            } else {
                // The same method through supertraits (`Iterator` methods via
                // a subtrait) is fine; distinct traits are ambiguous.
                let first = self.cx.db.def(cands[0].0).parent;
                if !cands.iter().all(|c| self.cx.db.def(c.0).parent == first) {
                    let names: Vec<String> = cands.iter().map(|c| format!("`{}`", self.cx.db.def(self.cx.db.def(c.0).parent.unwrap()).name)).collect();
                    return err(span, format!("multiple applicable items in scope: `{}` is a method of {}", name, names.join(" and ")));
                }
            }
        }
        let (m, parent, _) = cands.swap_remove(0);
        let tr = self.cx.db.def(m).parent.unwrap();
        let tref = TraitRef {
            trait_: tr,
            args: parent[1..].to_vec().into(),
        };
        self.register(Pred::Trait(parent[0].clone(), tref), span);
        Ok(Some(Pick { def: m, parent, step, autoref }))
    }

    /// Traits whose methods user code may call: those in scope, and every
    /// trait of the program itself (the standard library sees all).
    fn trait_visible(&self, tr: DefId) -> bool {
        let db = self.cx.db;
        if db.def(self.def).in_std || !db.def(tr).in_std {
            return true;
        }
        db.trait_in_scope(self.module, tr)
    }

    // ---- coercions ----

    /// Coerces an expression to a type (at coercion sites).
    pub fn coerce(&mut self, e: tir::Expr, target: &Ty) -> Result<tir::Expr> {
        let src = self.shallow(&e.ty);
        let tgt = self.shallow(target);
        let span = e.span;
        if matches!(src, Ty::Never) {
            return Ok(e);
        }
        if src.is_var() || tgt.is_var() {
            return self.unify_or_err(e, &tgt);
        }
        match (&src, &tgt) {
            (Ty::Ref(m1, a), Ty::Ref(m2, b)) if *m1 || !*m2 => {
                let (a, b, m2) = ((**a).clone(), (**b).clone(), *m2);
                return self.coerce_ref(e, &a, &b, m2, &tgt);
            }
            (Ty::Ref(_, a), Ty::Ptr(m, b)) => {
                let (a, b) = ((**a).clone(), (**b).clone());
                if self.unify(&a, &b) {
                    return Ok(mk(K::Cast(Box::new(e)), Ty::Ptr(*m, Rc::new(b)), span));
                }
            }
            (Ty::Ptr(true, a), Ty::Ptr(false, b)) => {
                let (a, b) = ((**a).clone(), (**b).clone());
                if self.unify(&a, &b) {
                    return Ok(mk(K::Cast(Box::new(e)), tgt.clone(), span));
                }
            }
            (Ty::Adt(d1, a), Ty::Adt(d2, b)) if d1 == d2 && self.cx.db.is_lang(*d1, "Box") => {
                let (a, b) = (self.shallow(&a[0]), self.shallow(&b[0]));
                if let Some(x) = self.try_unsize(e.clone(), &a, &b, &tgt, span)? {
                    return Ok(x);
                }
            }
            (Ty::FnDef(d, s), Ty::FnPtr(ps, r)) => {
                let (d, s) = (*d, s.clone());
                if let Some((sps, sr)) = self.callable_sig(&src) {
                    if sps.len() == ps.len() {
                        for (x, y) in sps.iter().zip(ps.iter()) {
                            self.unify(x, y);
                        }
                        self.unify(&sr, r);
                        let trait_item = matches!(self.cx.db.def(d).parent.map(|p| &self.cx.db.def(p).kind), Some(DefKind::Trait(_)));
                        let callee = if trait_item { Callee::Trait(d, s) } else { Callee::Item(d, s) };
                        return Ok(mk(K::Reify(callee), tgt.clone(), span));
                    }
                }
            }
            (Ty::Closure(id), Ty::FnPtr(ps, r)) => {
                let id = *id;
                if let Some((sps, sr)) = self.callable_sig(&src) {
                    if sps.len() == ps.len() {
                        for (x, y) in sps.iter().zip(ps.iter()) {
                            self.unify(x, y);
                        }
                        self.unify(&sr, r);
                        let env = e.clone();
                        return Ok(mk(K::Reify(Callee::Closure(Box::new(env), id)), tgt.clone(), span));
                    }
                }
            }
            _ => {}
        }
        self.unify_or_err(e, &tgt)
    }

    fn unify_or_err(&mut self, e: tir::Expr, tgt: &Ty) -> Result<tir::Expr> {
        let src = e.ty.clone();
        if self.unify(tgt, &src) {
            return Ok(e);
        }
        let (a, b) = (self.resolve(tgt), self.resolve(&src));
        err(e.span, format!("mismatched types: expected `{}`, found `{}`", self.name(&a), self.name(&b)))
    }

    /// `&T` to `&U`: reborrow, deref coercion or unsizing.
    fn coerce_ref(&mut self, e: tir::Expr, a: &Ty, b: &Ty, m2: bool, tgt: &Ty) -> Result<tir::Expr> {
        let span = e.span;
        let a = self.shallow(a);
        let b = self.shallow(b);
        if self.compat(&a, &b) != M::No && !(matches!(b, Ty::Dyn(_)) && !matches!(a, Ty::Dyn(_))) {
            if self.unify(&a, &b) {
                return Ok(retype(e, tgt.clone()));
            }
        }
        // Unsizing at the first step.
        if let Some(x) = self.try_unsize(e.clone(), &a, &b, tgt, span)? {
            return Ok(x);
        }
        // Deref coercion: `&String` to `&str`, `&Vec<T>` to `&[T]`, `&&T` to `&T`.
        let base = mk(K::Deref(Box::new(e.clone())), a.clone(), span);
        let steps = self.autoderef_steps(&a, span)?;
        for k in 1..steps.len() {
            let st = self.shallow(&steps[k].0);
            if self.compat(&st, &b) == M::Yes || (self.compat(&st, &b) == M::Maybe && !matches!(b, Ty::Dyn(_))) {
                if self.unify(&st, &b) {
                    let place = self.apply_steps(base, &steps[..=k], m2, span)?;
                    return Ok(mk(K::AddrOf(m2, Box::new(place)), tgt.clone(), span));
                }
            }
        }
        self.unify_or_err(e, tgt)
    }

    /// `[T; N]` to `[T]` and `T` to `dyn Trait` behind `&` or `Box`.
    fn try_unsize(&mut self, e: tir::Expr, a: &Ty, b: &Ty, tgt: &Ty, span: Span) -> Result<Option<tir::Expr>> {
        match (a, b) {
            (Ty::Array(x, _), Ty::Slice(y)) => {
                let (x, y) = ((**x).clone(), (**y).clone());
                if self.unify(&x, &y) {
                    return Ok(Some(mk(K::Unsize(Box::new(e)), tgt.clone(), span)));
                }
            }
            (src, Ty::Dyn(dy)) if !matches!(src, Ty::Dyn(_)) && !src.is_var() => {
                let dy = dy.clone();
                if self.is_fn_trait(dy.trait_) {
                    if let Some((ps, r)) = self.callable_sig(src) {
                        if let Some(Ty::Tuple(want)) = dy.args.first().map(|t| self.peek(t)) {
                            if want.len() == ps.len() {
                                for (p, w) in ps.iter().zip(want.iter()) {
                                    self.unify(p, w);
                                }
                            }
                        }
                        if let Some((_, out)) = dy.bindings.iter().find(|(n, _)| &**n == "Output") {
                            self.unify(&r, out);
                        }
                        return Ok(Some(mk(K::Unsize(Box::new(e)), tgt.clone(), span)));
                    }
                    return Ok(None);
                }
                let tref = TraitRef {
                    trait_: dy.trait_,
                    args: dy.args.clone(),
                };
                if self.evaluate(src, &tref, 0)? == M::No {
                    return err(span, format!("the trait bound `{}` is not satisfied", self.cx.trait_ref_name(src, &tref)));
                }
                self.register(Pred::Trait(src.clone(), tref.clone()), span);
                for (n, t) in dy.bindings.iter() {
                    let owner = tref.clone();
                    self.register(Pred::Proj(src.clone(), owner, n.clone(), t.clone()), span);
                }
                return Ok(Some(mk(K::Unsize(Box::new(e)), tgt.clone(), span)));
            }
            _ => {}
        }
        Ok(None)
    }

    // ---- mutable places ----

    /// Turns overloaded `Deref`/`Index` in a place into `DerefMut`/`IndexMut`.
    pub fn mutate_place(&mut self, e: &mut tir::Expr) {
        let span = e.span;
        match &mut e.kind {
            K::Deref(inner) => {
                if let K::Call(Callee::Trait(m, s), args) = &mut inner.kind {
                    let db = self.cx.db;
                    let deref = db.lang("Deref");
                    let index = db.lang("Index");
                    let parent = db.def(*m).parent;
                    let to = if parent == Some(deref) && &*db.def(*m).name == "deref" {
                        Some(("DerefMut", "deref_mut"))
                    } else if parent == Some(index) && &*db.def(*m).name == "index" {
                        Some(("IndexMut", "index_mut"))
                    } else {
                        None
                    };
                    if let Some((tr, name)) = to {
                        let trd = db.lang(tr);
                        *m = db.child(trd, name).unwrap();
                        let st = s[0].clone();
                        let targs: Rc<[Ty]> = s[1..].to_vec().into();
                        if let Some(first) = args.first_mut() {
                            if let K::AddrOf(mm, base) = &mut first.kind {
                                *mm = true;
                                self.mutate_place(base);
                            }
                            first.ty = Ty::reference(true, st.clone());
                        }
                        if let Ty::Ref(_, t) = &inner.ty {
                            inner.ty = Ty::Ref(true, t.clone());
                        }
                        self.register(
                            Pred::Trait(
                                st,
                                TraitRef {
                                    trait_: trd,
                                    args: targs,
                                },
                            ),
                            span,
                        );
                        return;
                    }
                }
                if let K::Unsize(x) = &mut inner.kind {
                    if let K::AddrOf(mm, base) = &mut x.kind {
                        *mm = true;
                        self.mutate_place(base);
                    }
                    return;
                }
                if let K::Slice(base, ..) = &mut inner.kind {
                    self.mutate_place(base);
                }
            }
            K::Field(b, _) | K::Index(b, _) => self.mutate_place(b),
            K::Block(_, tail) => self.mutate_place(tail),
            _ => {}
        }
    }
}

fn retype(mut e: tir::Expr, t: Ty) -> tir::Expr {
    e.ty = t;
    e
}

fn subst_opt(t: &Ty, binds: &[Option<Ty>]) -> Ty {
    t.map(&mut |x| match x {
        Ty::Param(i) => binds.get(*i as usize).cloned().flatten(),
        _ => None,
    })
}

