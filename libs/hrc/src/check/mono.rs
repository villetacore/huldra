//! Finishing a function: resolving every type in its IR, checking
//! literals and casts, resolving calls to instances (trait methods to
//! impls), building vtables; and the monomorphization driver.

use super::infer::{FnCtx, Selection};
use super::{Ctx, InstState, TraitRef};
use crate::resolve::{DefId, DefKind};
use crate::tir::{self, Builtin, Callee, ExprKind as K, PatKind};
use crate::ty::Ty;
use crate::{err, Result, Span};
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

impl<'c, 'a> FnCtx<'c, 'a> {
    fn concrete(&mut self, t: &Ty, span: Span) -> Result<Ty> {
        let mut t = self.resolve(t);
        if t.needs_infer() {
            // Normalizing may have selected impls whose bounds are not
            // processed yet.
            self.select_obligations(false)?;
            t = self.resolve(&t);
        }
        let mut bad = None;
        t.walk(&mut |x| {
            if bad.is_none() && matches!(x, Ty::Infer(_) | Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Proj(..) | Ty::Opaque(..) | Ty::Param(_)) {
                bad = Some(x.clone());
            }
        });
        match bad {
            None => {
                self.record_layouts(&t)?;
                Ok(t)
            }
            Some(Ty::Proj(..)) => err(span, format!("cannot resolve the associated type in `{}`", self.name(&t))),
            Some(_) => err(span, format!("type annotations needed for `{}`", self.name(&t))),
        }
    }

    /// Records normalized field types of ADT instances with associated
    /// types in their fields.
    fn record_layouts(&mut self, t: &Ty) -> Result<()> {
        let mut adts = Vec::new();
        t.walk(&mut |x| {
            if let Ty::Adt(..) = x {
                adts.push(x.clone());
            }
        });
        for a in adts {
            let Ty::Adt(def, substs) = &a else { continue };
            if self.cx.adt_fields.contains_key(&a) || self.cx.db.is_lang(*def, "Box") {
                continue;
            }
            let info = self.cx.adt(*def)?;
            let mut has_proj = false;
            for v in &info.variants {
                for (_, ft) in &v.fields {
                    ft.walk(&mut |x| {
                        if let Ty::Proj(..) = x {
                            has_proj = true;
                        }
                    });
                }
            }
            if !has_proj {
                // Still record nested instances reachable through fields.
                self.cx.adt_fields.insert(a.clone(), Vec::new());
                for v in &info.variants {
                    for (_, ft) in &v.fields {
                        let f = ft.subst(substs);
                        self.record_layouts(&f)?;
                    }
                }
                continue;
            }
            let mut out = Vec::new();
            for v in &info.variants {
                let mut fs = Vec::new();
                for (_, ft) in &v.fields {
                    let f = self.resolve(&ft.subst(substs));
                    fs.push(f);
                }
                out.push(fs);
            }
            self.cx.adt_fields.insert(a.clone(), out.clone());
            for fs in &out {
                for f in fs {
                    self.record_layouts(f)?;
                }
            }
        }
        Ok(())
    }

    pub fn zonk(&mut self, e: &mut tir::Expr) -> Result<()> {
        let span = e.span;
        e.ty = self.concrete(&e.ty, span)?;
        match &mut e.kind {
            K::Int(v) => {
                if let Ty::Int(it) = &e.ty {
                    let x = *v as i128;
                    let ok = if it.signed() {
                        let max = it.max_value() as i128;
                        x >= -max - 1 && x <= max
                    } else {
                        x >= 0 && (x as u128) <= it.max_value()
                    };
                    if !ok {
                        return err(span, format!("literal out of range for `{}`", it.name()));
                    }
                } else if let Ty::Float(_) = &e.ty {
                    let f = *v as i128 as f64;
                    e.kind = K::Float(f);
                }
            }
            K::Call(c, args) => {
                for a in args.iter_mut() {
                    self.zonk(a)?;
                }
                let c2 = self.resolve_callee(c, span)?;
                *c = c2;
            }
            K::Reify(c) => {
                let c2 = self.resolve_callee(c, span)?;
                *c = c2;
            }
            K::Closure(_, caps) => {
                for c in caps.iter_mut() {
                    self.zonk(c)?;
                }
            }
            K::Unsize(x) => {
                self.zonk(x)?;
                let src = x.ty.clone();
                let dst = e.ty.clone();
                self.unsize_vtable(&src, &dst, span)?;
            }
            K::Cast(x) => {
                self.zonk(x)?;
                let from = x.ty.clone();
                if !self.cast_ok(&from, &e.ty) {
                    return err(span, format!("non-primitive cast: `{}` as `{}`", self.name(&from), self.name(&e.ty)));
                }
            }
            K::Unary(_, x) | K::Deref(x) | K::AddrOf(_, x) | K::Field(x, _) | K::Repeat(x, _) | K::Return(x) | K::Break(_, x) | K::Loop(x, _) | K::LabeledBlock(x, _) => {
                self.zonk(x)?
            }
            K::Binary(_, a, b) | K::And(a, b) | K::Or(a, b) | K::Index(a, b) | K::Assign(a, b) | K::AssignOp(_, a, b) => {
                self.zonk(a)?;
                self.zonk(b)?;
            }
            K::Slice(a, lo, hi) => {
                self.zonk(a)?;
                if let Some(x) = lo {
                    self.zonk(x)?;
                }
                if let Some(x) = hi {
                    self.zonk(x)?;
                }
            }
            K::Adt(_, xs) | K::Tuple(xs) | K::Array(xs) => {
                for x in xs.iter_mut() {
                    self.zonk(x)?;
                }
            }
            K::Block(stmts, tail) => {
                for s in stmts.iter_mut() {
                    match s {
                        tir::Stmt::Let(p, init, els) => {
                            self.zonk_pat(p, span)?;
                            if let Some(x) = init {
                                self.zonk(x)?;
                            }
                            if let Some(x) = els {
                                self.zonk(x)?;
                            }
                        }
                        tir::Stmt::Expr(x) => self.zonk(x)?,
                    }
                }
                self.zonk(tail)?;
            }
            K::If(c, t, f) => {
                self.zonk(c)?;
                self.zonk(t)?;
                self.zonk(f)?;
            }
            K::Match(s, arms) => {
                self.zonk(s)?;
                for a in arms.iter_mut() {
                    self.zonk_pat(&mut a.pat, span)?;
                    if let Some(g) = &mut a.guard {
                        self.zonk(g)?;
                    }
                    self.zonk(&mut a.body)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn zonk_pat(&mut self, p: &mut tir::Pat, span: Span) -> Result<()> {
        p.ty = self.concrete(&p.ty, span)?;
        match &mut p.kind {
            PatKind::Bind(_, _, Some(s)) | PatKind::Deref(s) => self.zonk_pat(s, span)?,
            PatKind::Const(x) => self.zonk(x)?,
            PatKind::Range(lo, hi, _) => {
                if let Some(x) = lo {
                    self.zonk(x)?;
                }
                if let Some(x) = hi {
                    self.zonk(x)?;
                }
            }
            PatKind::Adt(_, fs) => {
                for (_, f) in fs.iter_mut() {
                    self.zonk_pat(f, span)?;
                }
            }
            PatKind::Or(ps) => {
                for x in ps.iter_mut() {
                    self.zonk_pat(x, span)?;
                }
            }
            PatKind::Slice(a, r, b) => {
                for x in a.iter_mut().chain(b.iter_mut()) {
                    self.zonk_pat(x, span)?;
                }
                if let Some(r) = r {
                    self.zonk_pat(r, span)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn cast_ok(&self, from: &Ty, to: &Ty) -> bool {
        let num = |t: &Ty| matches!(t, Ty::Int(_) | Ty::Float(_));
        if from == to {
            return true;
        }
        match (from, to) {
            (a, b) if num(a) && num(b) => true,
            (Ty::Bool | Ty::Char, Ty::Int(_)) => true,
            (Ty::Int(crate::ty::IntTy::U8), Ty::Char) => true,
            (Ty::Adt(d, _), Ty::Int(_)) => {
                matches!(self.cx.db.def(*d).kind, DefKind::Enum(_))
            }
            (Ty::Ptr(..) | Ty::Ref(..) | Ty::FnPtr(..) | Ty::FnDef(..), Ty::Ptr(..)) => true,
            (Ty::Ptr(..) | Ty::FnPtr(..) | Ty::FnDef(..), Ty::Int(_)) => true,
            (Ty::Int(_), Ty::Ptr(..)) => true,
            (Ty::FnDef(..), Ty::FnPtr(..)) => true,
            _ => false,
        }
    }

    fn resolve_callee(&mut self, c: &Callee, span: Span) -> Result<Callee> {
        Ok(match c {
            Callee::Item(def, substs) => {
                let substs: Vec<Ty> = substs.iter().map(|t| self.concrete(t, span)).collect::<Result<_>>()?;
                let def = *def;
                match &self.cx.db.def(def).kind {
                    DefKind::Struct(_) | DefKind::Variant(_) => Callee::Ctor(def, substs.into()),
                    DefKind::ExternFn(_, abi) if &**abi == "rust-intrinsic" => {
                        let name = self.cx.db.def(def).name.clone();
                        Callee::Builtin(Builtin::Intrinsic(name), substs.into())
                    }
                    DefKind::ExternFn(..) => Callee::Extern(def),
                    _ => Callee::Inst(self.cx.instance(def, substs.into())),
                }
            }
            Callee::Trait(def, substs) => {
                let substs: Vec<Ty> = substs.iter().map(|t| self.concrete(t, span)).collect::<Result<_>>()?;
                self.resolve_trait_item(*def, &substs, span)?
            }
            Callee::Value(x) => {
                let mut x = (**x).clone();
                self.zonk(&mut x)?;
                Callee::Value(Box::new(x))
            }
            Callee::Closure(x, id) => {
                let mut x = (**x).clone();
                self.zonk(&mut x)?;
                Callee::Closure(Box::new(x), *id)
            }
            other => other.clone(),
        })
    }

    /// A trait method (or constant) for concrete substs: the impl's item,
    /// the trait's default, a builtin, or a virtual call.
    pub fn resolve_trait_item(&mut self, m: DefId, substs: &[Ty], span: Span) -> Result<Callee> {
        let tr = self.cx.db.def(m).parent.unwrap();
        let g = self.cx.generics_of(tr);
        let n = g.count();
        let self_ty = substs[0].clone();
        let tref = TraitRef {
            trait_: tr,
            args: substs[1..n].to_vec().into(),
        };
        let name = self.cx.db.def(m).name.clone();
        match self.select(&self_ty, &tref, span)? {
            Selection::Impl(imp, isubsts) => {
                // Impl parameters fixed only by its bounds (`B` in
                // `impl<B, I, F: FnMut(I::Item) -> B> Iterator for Map<I, F>`).
                self.select_obligations(false)?;
                let isubsts: Vec<Ty> = isubsts.iter().map(|t| self.concrete(t, span)).collect::<Result<_>>()?;
                if let Some(c) = self.cx.db.child(imp, &name) {
                    let mut s = isubsts;
                    s.extend(substs[n..].iter().cloned());
                    return Ok(Callee::Inst(self.cx.instance(c, s.into())));
                }
                Ok(Callee::Inst(self.cx.instance(m, substs.to_vec().into())))
            }
            Selection::Dyn => {
                let principal = match &self_ty {
                    Ty::Dyn(d) => d.trait_,
                    _ => unreachable!(),
                };
                let slots = self.cx.vtable_slots(principal)?;
                match slots.iter().position(|s| *s == m) {
                    Some(i) => Ok(Callee::Virtual(principal, i as u32)),
                    None => err(span, format!("the method `{}` cannot be called on a trait object", name)),
                }
            }
            Selection::Builtin => {
                if self.cx.db.is_lang(tr, "Clone") {
                    let copy = TraitRef {
                        trait_: self.cx.db.lang("Copy"),
                        args: Rc::from([]),
                    };
                    let mut parts = Vec::new();
                    if self.evaluate(&self_ty, &copy, 0)? != super::infer::M::Yes {
                        let elems: Vec<Ty> = match &self_ty {
                            Ty::Tuple(ts) => ts.to_vec(),
                            Ty::Array(t, _) => alloc::vec![(**t).clone()],
                            _ => Vec::new(),
                        };
                        for t in elems {
                            parts.push(self.resolve_trait_item(m, &[t], span)?);
                        }
                    }
                    return Ok(Callee::Builtin(Builtin::Clone(parts), Rc::from([self_ty])));
                }
                err(span, format!("cannot call `{}` on `{}`", name, self.name(&self_ty)))
            }
            _ => err(
                span,
                format!(
                    "the trait bound `{}` is not satisfied",
                    self.cx.trait_ref_name(&self_ty, &tref)
                ),
            ),
        }
    }

    /// Builds the vtable for an unsizing coercion to a trait object.
    fn unsize_vtable(&mut self, src: &Ty, dst: &Ty, span: Span) -> Result<()> {
        let inner = |t: &Ty| -> Option<Ty> {
            match t {
                Ty::Ref(_, x) | Ty::Ptr(_, x) => Some((**x).clone()),
                Ty::Adt(_, a) if !a.is_empty() => Some(a[0].clone()),
                _ => None,
            }
        };
        let (Some(s), Some(d)) = (inner(src), inner(dst)) else { return Ok(()) };
        let dy = match &d {
            Ty::Dyn(dy) => dy.clone(),
            _ => return Ok(()),
        };
        if self.is_fn_trait(dy.trait_) {
            // Closures and functions are called directly; no methods.
            let vt = self.cx.vtable(dy.trait_, s.clone());
            if let Ty::FnDef(d, substs) = &s {
                let c = self.resolve_callee(&Callee::Item(*d, substs.clone()), span)?;
                if let Callee::Inst(i) = c {
                    self.cx.vtables[vt].fn_inst = Some(i);
                }
            }
            return Ok(());
        }
        let existing = self.cx.vtables.len();
        let vt = self.cx.vtable(dy.trait_, s.clone());
        if vt < existing {
            return Ok(());
        }
        let slots = self.cx.vtable_slots(dy.trait_)?;
        let principal = TraitRef {
            trait_: dy.trait_,
            args: dy.args.clone(),
        };
        let supers = self.cx.elaborate(&principal, &s)?;
        let mut methods = Vec::new();
        for m in slots {
            let g = self.cx.generics_of(m);
            if g.own_count() > 0 {
                methods.push(None);
                continue;
            }
            let tr = self.cx.db.def(m).parent.unwrap();
            let targs = supers.iter().find(|t| t.trait_ == tr).map(|t| t.args.clone()).unwrap_or_else(|| Rc::from([]));
            let mut substs = alloc::vec![s.clone()];
            substs.extend(targs.iter().cloned());
            match self.resolve_trait_item(m, &substs, span)? {
                Callee::Inst(i) => methods.push(Some(i)),
                _ => methods.push(None),
            }
        }
        self.cx.vtables[vt].methods = methods;
        Ok(())
    }
}

impl<'a> Ctx<'a> {
    /// Methods of a trait and its supertraits, in vtable order.
    pub fn vtable_slots(&mut self, tr: DefId) -> Result<Vec<DefId>> {
        let tref = TraitRef {
            trait_: tr,
            args: self.generics_of(tr).identity()[1..].to_vec().into(),
        };
        let all = self.elaborate(&tref, &Ty::Param(0))?;
        let mut out = Vec::new();
        for t in all {
            for &c in &self.db.def(t.trait_).children {
                if let DefKind::Fn(f) = &self.db.def(c).kind {
                    if f.self_param.is_some() {
                        out.push(c);
                    }
                }
            }
        }
        Ok(out)
    }
}

/// Checks queued instances until none are left.
pub fn run(cx: &mut Ctx) -> Result<()> {
    let mut next = 0;
    loop {
        // Process in creation order for deterministic output.
        while next < cx.instances.len() {
            if let InstState::Pending = cx.instances[next].state {
                cx.check_instance(next as u32, Span::default())?;
            }
            next += 1;
        }
        if next >= cx.instances.len() {
            break;
        }
    }
    cx.queue.clear();
    Ok(())
}

pub fn finish(mut cx: Ctx, main: tir::InstId, main_result: Option<tir::InstId>) -> Result<tir::Program> {
    let mut layouts = alloc::collections::BTreeMap::new();
    let mut externs = alloc::collections::BTreeMap::new();
    for i in 0..cx.db.defs.len() {
        let d = i as DefId;
        match &cx.db.def(d).kind {
            DefKind::Struct(_) | DefKind::Enum(_) => {
                let info = cx.adt(d)?;
                layouts.insert(
                    d,
                    tir::Layout {
                        variants: info.variants.iter().map(|v| v.fields.iter().map(|(_, t)| t.clone()).collect()).collect(),
                        discs: info.variants.iter().map(|v| v.disc).collect(),
                    },
                );
            }
            DefKind::ExternFn(_, abi) if &**abi != "rust-intrinsic" => {
                let sig = cx.sig(d)?;
                externs.insert(d, (sig.params.clone(), sig.ret.clone()));
            }
            _ => {}
        }
    }
    let mut fns = Vec::new();
    for inst in cx.instances {
        match inst.state {
            InstState::Done(f, _) => fns.push(f),
            _ => {
                return err(Span::default(), String::from("internal error: an instance was not checked"));
            }
        }
    }
    let mut closures = Vec::new();
    for (i, c) in cx.closures.into_iter().enumerate() {
        match c {
            Some(c) => closures.push(c),
            None => {
                // A closure in a function that was abandoned (e.g. a constant
                // folded away): give it an empty body.
                let _ = i;
                closures.push(tir::Closure {
                    params: Vec::new(),
                    ret: crate::ty::unit(),
                    captures: Vec::new(),
                    body: tir::Expr {
                        kind: K::Unreachable,
                        ty: Ty::Never,
                        span: Span::default(),
                    },
                    func: 0,
                });
            }
        }
    }
    Ok(tir::Program {
        fns,
        closures,
        statics: cx.statics,
        vtables: cx.vtables,
        main,
        main_result,
        layouts,
        externs,
        adt_fields: cx.adt_fields.into_iter().filter(|(_, v)| !v.is_empty()).collect(),
    })
}
