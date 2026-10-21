//! Lowering of syntactic types, trait references and bounds.

use super::{Ctx, Generics, Pred, TraitRef};
use crate::ast::{self, Name, TypeKind};
use crate::resolve::{Db, DefId, DefKind, ModId, Ns, Res};
use crate::ty::{DynTy, FloatTy, IntTy, Ty};
use crate::{err, Result, Span};
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

/// Where names in a type are resolved.
#[derive(Clone)]
pub struct Scope {
    pub module: ModId,
    pub generics: Rc<Generics>,
    pub self_ty: Option<Ty>,
    /// Inside a trait definition (`Self::Item` is a projection).
    pub trait_def: Option<DefId>,
    /// Inside an impl (`Self::Item` is the impl's associated type).
    pub impl_def: Option<DefId>,
}

/// Supplies inference variables for `_` and omitted generic arguments.
pub trait VarSource {
    fn fresh(&mut self) -> Option<Ty>;
}

pub struct NoInfer;

impl VarSource for NoInfer {
    fn fresh(&mut self) -> Option<Ty> {
        None
    }
}

pub enum ImplMode<'c> {
    Forbid,
    Arg(&'c mut usize),
    Ret(DefId),
}

pub fn prim_type(name: &str) -> Option<Ty> {
    if let Some(i) = IntTy::from_name(name) {
        return Some(Ty::Int(i));
    }
    Some(match name {
        "f32" => Ty::Float(FloatTy::F32),
        "f64" => Ty::Float(FloatTy::F64),
        "bool" => Ty::Bool,
        "char" => Ty::Char,
        "str" => Ty::Str,
        _ => return None,
    })
}

/// A function (or other value) of the standard library by path.
pub fn std_fn(db: &Db, path: &[&str]) -> Option<DefId> {
    let mut cur = db.std_root;
    for (i, s) in path.iter().enumerate() {
        let last = i + 1 == path.len();
        if last {
            return match db.lookup(cur, s, Ns::Value) {
                Some(Res::Def(d)) => Some(d),
                _ => None,
            };
        }
        match db.lookup(cur, s, Ns::Type)? {
            Res::Mod(m) => cur = m,
            Res::Def(d) => {
                // `Type::item`
                if i + 2 == path.len() {
                    return db.child(d, path[i + 1]);
                }
                return None;
            }
        }
    }
    None
}

impl<'a> Ctx<'a> {
    pub fn lower_ty(&mut self, sc: &Scope, t: &ast::Type, vs: &mut dyn VarSource) -> Result<Ty> {
        self.lower(sc, t, vs, &mut ImplMode::Forbid)
    }

    pub fn lower_ty_impl_arg(&mut self, sc: &Scope, t: &ast::Type, counter: &mut usize) -> Result<Ty> {
        self.lower(sc, t, &mut NoInfer, &mut ImplMode::Arg(counter))
    }

    pub fn lower_ty_ret(&mut self, sc: &Scope, t: &ast::Type, def: DefId) -> Result<Ty> {
        self.lower(sc, t, &mut NoInfer, &mut ImplMode::Ret(def))
    }

    fn lower(&mut self, sc: &Scope, t: &ast::Type, vs: &mut dyn VarSource, mode: &mut ImplMode) -> Result<Ty> {
        Ok(match &t.kind {
            TypeKind::Path(p) => self.lower_path(sc, p, vs, mode)?,
            TypeKind::Ref(m, inner) => Ty::Ref(*m, Rc::new(self.lower(sc, inner, vs, mode)?)),
            TypeKind::Ptr(m, inner) => Ty::Ptr(*m, Rc::new(self.lower(sc, inner, vs, mode)?)),
            TypeKind::Tuple(ts) => {
                let mut out = Vec::new();
                for t in ts {
                    out.push(self.lower(sc, t, vs, mode)?);
                }
                Ty::Tuple(out.into())
            }
            TypeKind::Array(elem, len) => {
                let e = self.lower(sc, elem, vs, mode)?;
                let n = self.lower_const_arg(sc, len, vs)?;
                Ty::Array(Rc::new(e), Rc::new(n))
            }
            TypeKind::Slice(e) => Ty::Slice(Rc::new(self.lower(sc, e, vs, mode)?)),
            TypeKind::Fn(ps, r) => {
                let mut out = Vec::new();
                for p in ps {
                    out.push(self.lower(sc, p, vs, mode)?);
                }
                Ty::FnPtr(out.into(), Rc::new(self.lower(sc, r, vs, mode)?))
            }
            TypeKind::Never => Ty::Never,
            TypeKind::Infer => match vs.fresh() {
                Some(v) => v,
                None => return err(t.span, String::from("the placeholder `_` is not allowed within types on item signatures")),
            },
            TypeKind::ImplTrait(bounds) => match mode {
                ImplMode::Arg(counter) => {
                    let i = **counter;
                    **counter += 1;
                    let _ = bounds;
                    Ty::Param(i as u32)
                }
                ImplMode::Ret(def) => Ty::Opaque(*def, sc.generics.identity()),
                ImplMode::Forbid => {
                    return err(t.span, String::from("`impl Trait` is only allowed in function parameters and return types"));
                }
            },
            TypeKind::Dyn(bounds) => self.lower_dyn(sc, bounds, vs, t.span)?,
        })
    }

    /// An array length or const generic argument.
    fn lower_const_arg(&mut self, sc: &Scope, e: &ast::Expr, vs: &mut dyn VarSource) -> Result<Ty> {
        if let ast::ExprKind::Path(p) = &e.kind {
            if let Some(n) = p.as_ident() {
                if let Some(i) = sc.generics.index_of(n) {
                    return Ok(Ty::Param(i as u32));
                }
            }
            if let Some(n) = p.as_ident() {
                if &**n == "_" {
                    if let Some(v) = vs.fresh() {
                        return Ok(v);
                    }
                }
            }
        }
        let v = self.const_eval(sc.module, e)?;
        if v < 0 {
            return err(e.span, String::from("array length must be non-negative"));
        }
        Ok(Ty::Const(v as u64))
    }

    fn lower_dyn(&mut self, sc: &Scope, bounds: &[ast::Bound], vs: &mut dyn VarSource, span: Span) -> Result<Ty> {
        for b in bounds {
            let tr = self.resolve_trait_path(sc, b)?;
            let name = &*self.db.def(tr).name;
            if matches!(name, "Send" | "Sync" | "Unpin" | "UnwindSafe" | "RefUnwindSafe") && bounds.len() > 1 {
                continue;
            }
            let (tref, bindings) = self.lower_trait_ref(sc, b, &Ty::Error, vs)?;
            return Ok(Ty::Dyn(Rc::new(DynTy {
                trait_: tref.trait_,
                args: tref.args,
                bindings: bindings.into(),
            })));
        }
        err(span, String::from("at least one trait is required for an object type"))
    }

    fn lower_generic_args(
        &mut self,
        sc: &Scope,
        def: DefId,
        args: Option<&ast::GenericArgs>,
        vs: &mut dyn VarSource,
        span: Span,
        self_ty: Option<&Ty>,
    ) -> Result<Vec<Ty>> {
        let g = self.generics_of(def);
        let start = g.own_start;
        let own = g.count() - start;
        let given: &[ast::Type] = args.map(|a| &a.types[..]).unwrap_or(&[]);
        if given.len() > own {
            return err(
                span,
                format!(
                    "{} takes {} generic argument{} but {} generic argument{} supplied",
                    self.db.def(def).name,
                    own,
                    if own == 1 { "" } else { "s" },
                    given.len(),
                    if given.len() == 1 { " was" } else { "s were" }
                ),
            );
        }
        let mut out: Vec<Ty> = Vec::new();
        if let Some(s) = self_ty {
            out.push(s.clone());
        }
        for i in 0..own {
            let p = &g.params[start + i];
            if let Some(t) = given.get(i) {
                let ty = if p.is_const {
                    match &t.kind {
                        TypeKind::Array(_, e) => self.lower_const_arg(sc, e, vs)?,
                        TypeKind::Path(path) => {
                            let e = ast::Expr {
                                kind: ast::ExprKind::Path(path.clone()),
                                span: t.span,
                            };
                            self.lower_const_arg(sc, &e, vs)?
                        }
                        TypeKind::Infer => match vs.fresh() {
                            Some(v) => v,
                            None => return err(t.span, String::from("the placeholder `_` is not allowed here")),
                        },
                        _ => return err(t.span, String::from("expected a constant argument")),
                    }
                } else {
                    self.lower_ty(sc, t, vs)?
                };
                out.push(ty);
                continue;
            }
            // A default (which may refer to earlier parameters and Self).
            if let Some(d) = p.ast.as_ref().and_then(|a| a.default.clone()) {
                let dsc = self.item_scope(def)?;
                let t = self.lower_ty(&dsc, &d, &mut NoInfer)?;
                let t = t.subst(&out);
                out.push(t);
                continue;
            }
            match vs.fresh() {
                Some(v) => out.push(v),
                None => {
                    return err(
                        span,
                        format!(
                            "missing generics for `{}`",
                            self.db.def(def).name
                        ),
                    )
                }
            }
        }
        if self_ty.is_some() {
            Ok(out)
        } else {
            Ok(out)
        }
    }

    fn resolve_type_res(&self, sc: &Scope, p: &ast::Path) -> (Option<Res>, usize) {
        let names: Vec<Name> = p.segs.iter().map(|s| s.name.clone()).collect();
        let (m, i) = self.db.resolve_module_prefix(sc.module, &names, p.global);
        if i >= names.len() {
            return (Some(Res::Mod(m)), i);
        }
        (self.db.lookup_seg(m, i, &names[i], Ns::Type), i)
    }

    pub fn resolve_trait_path(&mut self, sc: &Scope, p: &ast::Path) -> Result<DefId> {
        match self.resolve_type_res(sc, p) {
            (Some(Res::Def(d)), i) if i + 1 == p.segs.len() => match self.db.def(d).kind {
                DefKind::Trait(_) => Ok(d),
                _ => err(p.span, format!("expected trait, found `{}`", self.db.def(d).name)),
            },
            _ => err(p.span, format!("cannot find trait `{}` in this scope", p.last())),
        }
    }

    pub fn lower_path(&mut self, sc: &Scope, p: &ast::Path, vs: &mut dyn VarSource, mode: &mut ImplMode) -> Result<Ty> {
        let _ = mode;
        if let Some(q) = &p.qself {
            let base = self.lower_ty(sc, &q.ty, vs)?;
            let first = &p.segs[0];
            let mut t = match &q.trait_ {
                Some(tp) => {
                    let (tr, _) = self.lower_trait_ref(sc, tp, &base, vs)?;
                    let mut args = Vec::from([base]);
                    args.extend(tr.args.iter().cloned());
                    Ty::Proj(tr.trait_, args.into(), first.name.clone())
                }
                None => self.assoc_type(sc, base, &first.name, p.span, false)?,
            };
            for s in &p.segs[1..] {
                t = self.assoc_type(sc, t, &s.name, p.span, false)?;
            }
            return Ok(t);
        }
        let first = &p.segs[0];
        let mut rest_start = 1;
        let mut base: Option<Ty> = None;
        if !p.global && first.args.is_none() {
            if let Some(i) = sc.generics.index_of(&first.name) {
                if &*first.name != "Self" || sc.trait_def.is_some() {
                    base = Some(Ty::Param(i as u32));
                }
            }
            if base.is_none() && &*first.name == "Self" {
                match &sc.self_ty {
                    Some(t) => base = Some(t.clone()),
                    None => return err(p.span, String::from("`Self` is only available in impls, traits, and type definitions")),
                }
            }
        }
        if base.is_none() {
            let (res, i) = self.resolve_type_res(sc, p);
            let seg = &p.segs[i.min(p.segs.len() - 1)];
            rest_start = i + 1;
            base = Some(match res {
                Some(Res::Def(d)) => match self.db.def(d).kind.clone() {
                    DefKind::Struct(_) | DefKind::Enum(_) => {
                        let args = self.lower_generic_args(sc, d, seg.args.as_ref(), vs, p.span, None)?;
                        Ty::Adt(d, args.into())
                    }
                    DefKind::TypeAlias(_, ty) => {
                        let args = self.lower_generic_args(sc, d, seg.args.as_ref(), vs, p.span, None)?;
                        let asc = self.item_scope(d)?;
                        let t = self.lower_ty(&asc, &ty, &mut NoInfer)?;
                        t.subst(&args)
                    }
                    DefKind::Trait(_) => {
                        // A bare trait object (`Box<Fn(i32)>`).
                        let mut q = p.clone();
                        q.segs.truncate(i + 1);
                        self.lower_dyn(sc, core::slice::from_ref(&q), vs, p.span)?
                    }
                    _ => return err(p.span, format!("expected type, found `{}`", self.db.def(d).name)),
                },
                // `std::f64` is also a module; a single segment is the type.
                Some(Res::Mod(_)) if i == 0 && p.segs.len() == 1 && prim_type(&seg.name).is_some() => prim_type(&seg.name).unwrap(),
                Some(Res::Mod(_)) => return err(p.span, format!("expected type, found module `{}`", seg.name)),
                None => {
                    if i == 0 && !p.global {
                        match prim_type(&seg.name) {
                            Some(t) => t,
                            None => return err(p.span, format!("cannot find type `{}` in this scope", seg.name)),
                        }
                    } else {
                        return err(p.span, format!("cannot find type `{}` in this scope", seg.name));
                    }
                }
            });
        }
        let mut t = base.unwrap();
        let mut via_self = rest_start == 1 && &*first.name == "Self" && !p.global;
        for s in &p.segs[rest_start.min(p.segs.len())..] {
            t = self.assoc_type(sc, t, &s.name, p.span, via_self)?;
            via_self = false;
        }
        Ok(t)
    }

    /// `T::Name` for a type `T`.
    pub fn assoc_type(&mut self, sc: &Scope, base: Ty, name: &Name, span: Span, via_self: bool) -> Result<Ty> {
        // Inside an impl: its own associated types.
        if let (Some(imp), Some(st)) = (sc.impl_def, &sc.self_ty) {
            if via_self && &base == st {
                if let Some(c) = self.db.child(imp, name) {
                    if let DefKind::ImplType(t) = &self.db.def(c).kind {
                        let t = t.clone();
                        let isc = self.item_scope(imp)?;
                        return self.lower_ty(&isc, &t, &mut NoInfer);
                    }
                }
                let h = self.impl_header(imp)?;
                if let Some(tr) = &h.trait_ref {
                    if let Some(tref) = self.find_assoc_in(tr, &base, name)? {
                        let mut args = Vec::from([base]);
                        args.extend(tref.args.iter().cloned());
                        return Ok(Ty::Proj(tref.trait_, args.into(), name.clone()));
                    }
                }
            }
        }
        // Inside a trait: `Self::Item`.
        if let (Some(tr), Ty::Param(0)) = (sc.trait_def, &base) {
            let g = self.generics_of(tr);
            let tref = TraitRef {
                trait_: tr,
                args: g.identity()[1..].to_vec().into(),
            };
            if let Some(found) = self.find_assoc_in(&tref, &base, name)? {
                let mut args = Vec::from([base]);
                args.extend(found.args.iter().cloned());
                return Ok(Ty::Proj(found.trait_, args.into(), name.clone()));
            }
        }
        // A generic parameter: look through its bounds.
        if let Ty::Param(i) = base {
            let pname = sc.generics.params.get(i as usize).map(|p| p.name.clone());
            if let Some(pname) = pname {
                let mut bounds: Vec<ast::Bound> = Vec::new();
                let pinfo = sc.generics.params[i as usize].clone();
                if let Some(a) = &pinfo.ast {
                    bounds.extend(a.bounds.iter().cloned());
                }
                bounds.extend(pinfo.impl_bounds.iter().cloned());
                for (t, bs) in &sc.generics.where_ {
                    if let TypeKind::Path(p) = &t.kind {
                        if p.as_ident() == Some(&pname) {
                            bounds.extend(bs.iter().cloned());
                        }
                    }
                }
                for b in &bounds {
                    let Ok(trd) = self.resolve_trait_path(sc, b) else { continue };
                    let _ = trd;
                    let (tref, _) = self.lower_trait_ref(sc, b, &base, &mut NoInfer)?;
                    if let Some(found) = self.find_assoc_in(&tref, &base, name)? {
                        let mut args = Vec::from([base.clone()]);
                        args.extend(found.args.iter().cloned());
                        return Ok(Ty::Proj(found.trait_, args.into(), name.clone()));
                    }
                }
            }
        }
        err(
            span,
            format!(
                "ambiguous associated type `{}` on `{}`",
                name,
                self.ty_name(&base)
            ),
        )
    }

    /// The trait (among `tr` and its supertraits) declaring `name`.
    fn find_assoc_in(&mut self, tr: &TraitRef, self_ty: &Ty, name: &str) -> Result<Option<TraitRef>> {
        for t in self.elaborate(tr, self_ty)? {
            if let Some(c) = self.db.child(t.trait_, name) {
                if let DefKind::AssocType(..) = self.db.def(c).kind {
                    return Ok(Some(t));
                }
            }
        }
        Ok(None)
    }

    /// A trait reference with its associated type bindings.
    pub fn lower_trait_ref(&mut self, sc: &Scope, p: &ast::Path, self_ty: &Ty, vs: &mut dyn VarSource) -> Result<(TraitRef, Vec<(Name, Ty)>)> {
        let tr = self.resolve_trait_path(sc, p)?;
        let seg = p.segs.last().unwrap();
        let args = self.lower_generic_args(sc, tr, seg.args.as_ref(), vs, p.span, Some(self_ty))?;
        let mut bindings = Vec::new();
        if let Some(a) = &seg.args {
            for (n, t) in &a.bindings {
                bindings.push((n.clone(), self.lower_ty(sc, t, vs)?));
            }
        }
        Ok((
            TraitRef {
                trait_: tr,
                args: args[1..].to_vec().into(),
            },
            bindings,
        ))
    }

    pub fn lower_trait_ref_opt(&mut self, sc: &Scope, p: &ast::Path, self_ty: &Ty, vs: &mut dyn VarSource) -> Result<Option<(TraitRef, Vec<(Name, Ty)>)>> {
        if p.segs.len() == 1 && &*p.segs[0].name == "Sized" {
            return Ok(None);
        }
        Ok(Some(self.lower_trait_ref(sc, p, self_ty, vs)?))
    }

    pub fn lower_bound(&mut self, sc: &Scope, subject: &Ty, b: &ast::Bound, out: &mut Vec<Pred>, vs: &mut dyn VarSource) -> Result<()> {
        if let Some((tr, bindings)) = self.lower_trait_ref_opt(sc, b, subject, vs)? {
            for (n, t) in bindings {
                // `Iterator<Item = T>`: the binding may be on a supertrait.
                let owner = self.find_assoc_in(&tr, subject, &n)?.unwrap_or_else(|| tr.clone());
                out.push(Pred::Proj(subject.clone(), owner, n, t));
            }
            out.push(Pred::Trait(subject.clone(), tr));
        }
        Ok(())
    }

    /// All bounds and where clauses of an item (and its parent).
    pub fn lower_preds(&mut self, sc: &Scope) -> Result<Vec<Pred>> {
        let mut out = Vec::new();
        let g = sc.generics.clone();
        for (i, p) in g.params.iter().enumerate() {
            let subject = Ty::Param(i as u32);
            if let Some(a) = &p.ast {
                for b in &a.bounds {
                    self.lower_bound(sc, &subject, b, &mut out, &mut NoInfer)?;
                }
            }
            for b in &p.impl_bounds {
                self.lower_bound(sc, &subject, b, &mut out, &mut NoInfer)?;
            }
        }
        for (t, bs) in &g.where_ {
            let subject = self.lower_ty(sc, t, &mut NoInfer)?;
            for b in bs {
                self.lower_bound(sc, &subject, b, &mut out, &mut NoInfer)?;
            }
        }
        Ok(out)
    }
}
