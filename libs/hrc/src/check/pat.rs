//! Patterns, with default binding modes ("match ergonomics").

use super::expr::ValueRes;
use super::infer::FnCtx;
use super::FieldsKind;
use crate::ast::{self, Name, PatKind as P};
use crate::resolve::{DefId, DefKind, Ns, Res};
use crate::tir::{self, PatKind as T};
use crate::ty::Ty;
use crate::{err, Result, Span};
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

struct Binds {
    list: Vec<(Name, tir::LocalId)>,
    /// Later alternatives of an or-pattern reuse the first one's locals.
    reuse: Option<Vec<(Name, tir::LocalId)>>,
}

impl<'c, 'a> FnCtx<'c, 'a> {
    /// Checks a pattern against a type; returns it and its bindings.
    pub fn check_pat_top(&mut self, p: &ast::Pat, expected: &Ty) -> Result<(tir::Pat, Vec<(Name, tir::LocalId)>)> {
        let mut b = Binds {
            list: Vec::new(),
            reuse: None,
        };
        let pat = self.pat(p, expected, None, &mut b)?;
        Ok((pat, b.list))
    }

    fn bind_pat(&mut self, name: &Name, ty: Ty, b: &mut Binds, span: Span) -> Result<tir::LocalId> {
        if let Some(prev) = &b.reuse {
            if let Some((_, id)) = prev.iter().find(|(n, _)| n == name) {
                let id = *id;
                let lt = self.locals[id as usize].ty.clone();
                self.expect_eq(&lt, &ty, span)?;
                b.list.push((name.clone(), id));
                return Ok(id);
            }
            return err(span, format!("variable `{}` is not bound in all patterns", name));
        }
        if b.list.iter().any(|(n, _)| n == name) {
            return err(span, format!("identifier `{}` is bound more than once in the same pattern", name));
        }
        let id = self.new_local(name.clone(), ty);
        b.list.push((name.clone(), id));
        Ok(id)
    }

    /// Whether a pattern matches through references automatically.
    fn is_structural(&self, p: &ast::Pat) -> bool {
        match &p.kind {
            P::Wild | P::Ref(..) | P::Rest => false,
            P::Ident { name, by_ref, mutable, sub } => !*by_ref && !*mutable && sub.is_none() && self.is_unit_path_name(name),
            P::Or(alts) => alts.iter().any(|a| self.is_structural(a)),
            P::Lit(e) => !matches!(&e.kind, ast::ExprKind::Lit(ast::Lit::Str(_)) | ast::ExprKind::Lit(ast::Lit::ByteStr(_))),
            _ => true,
        }
    }

    fn pat(&mut self, p: &ast::Pat, expected: &Ty, def_bm: Option<bool>, b: &mut Binds) -> Result<tir::Pat> {
        let span = p.span;
        let mut expected = self.shallow(expected);
        let mut def_bm = def_bm;
        // Peel references for non-reference patterns.
        let mut derefs = Vec::new();
        if self.is_structural(p) {
            while let Ty::Ref(m, inner) = &expected {
                derefs.push(expected.clone());
                def_bm = Some(match def_bm {
                    Some(false) => false,
                    _ => *m,
                });
                expected = self.shallow(inner);
            }
        }
        // A `&str` literal pattern against `&&str`.
        if let P::Lit(e) = &p.kind {
            if matches!(&e.kind, ast::ExprKind::Lit(ast::Lit::Str(_))) {
                while let Ty::Ref(_, inner) = &expected {
                    let it = self.shallow(inner);
                    if matches!(it, Ty::Ref(..)) {
                        derefs.push(expected.clone());
                        expected = it;
                    } else {
                        break;
                    }
                }
            }
        }
        let inner = self.pat_inner(p, &expected, def_bm, b, span)?;
        let mut out = inner;
        for t in derefs.into_iter().rev() {
            out = tir::Pat {
                kind: T::Deref(Box::new(out)),
                ty: t,
            };
        }
        Ok(out)
    }

    fn pat_inner(&mut self, p: &ast::Pat, expected: &Ty, def_bm: Option<bool>, b: &mut Binds, span: Span) -> Result<tir::Pat> {
        let mk = |kind: T, ty: &Ty| tir::Pat { kind, ty: ty.clone() };
        match &p.kind {
            P::Wild | P::Rest => Ok(mk(T::Wild, expected)),
            P::Ident { name, by_ref, mutable, sub } => {
                if !*by_ref && !*mutable && sub.is_none() && self.is_unit_path_name(name) {
                    let path = ast::Path::single(name.clone(), span);
                    return self.path_pat(&path, expected, span);
                }
                let mode = if *by_ref { Some(*mutable) } else if *mutable { None } else { def_bm };
                let lty = match mode {
                    Some(m) => Ty::reference(m, expected.clone()),
                    None => expected.clone(),
                };
                let sub = match sub {
                    Some(s) => Some(Box::new(self.pat(s, expected, def_bm, b)?)),
                    None => None,
                };
                let id = self.bind_pat(name, lty, b, span)?;
                Ok(mk(T::Bind(id, mode, sub), expected))
            }
            P::Lit(e) => {
                let v = self.check_expr(e, Some(expected))?;
                let vt = v.ty.clone();
                self.expect_eq(expected, &vt, span)?;
                Ok(mk(T::Const(Box::new(v)), expected))
            }
            P::Range(lo, hi, incl) => {
                let lo = match lo {
                    Some(x) => {
                        let v = self.check_expr(x, Some(expected))?;
                        let vt = v.ty.clone();
                        self.expect_eq(expected, &vt, span)?;
                        Some(Box::new(v))
                    }
                    None => None,
                };
                let hi = match hi {
                    Some(x) => {
                        let v = self.check_expr(x, Some(expected))?;
                        let vt = v.ty.clone();
                        self.expect_eq(expected, &vt, span)?;
                        Some(Box::new(v))
                    }
                    None => None,
                };
                let t = self.shallow(expected);
                if !matches!(t, Ty::Int(_) | Ty::IntVar(_) | Ty::Char | Ty::Float(_) | Ty::FloatVar(_)) {
                    return err(span, format!("only `char` and numeric types are allowed in range patterns, found `{}`", self.name(&t)));
                }
                Ok(mk(T::Range(lo, hi, *incl), expected))
            }
            P::Tuple(pats) => {
                let rest = pats.iter().position(|x| matches!(x.kind, P::Rest));
                let n_fixed = pats.len() - rest.is_some() as usize;
                let elems: Vec<Ty> = match self.shallow(expected) {
                    Ty::Tuple(ts) => ts.to_vec(),
                    t if t.is_var() && rest.is_none() => {
                        let ts: Vec<Ty> = (0..pats.len()).map(|_| self.fresh()).collect();
                        let tt = Ty::Tuple(ts.clone().into());
                        self.unify(&t, &tt);
                        ts
                    }
                    t => return err(span, format!("mismatched types: expected `{}`, found a tuple", self.name(&t))),
                };
                if (rest.is_none() && elems.len() != pats.len()) || elems.len() < n_fixed {
                    return err(span, format!("mismatched types: expected a tuple with {} elements, found one with {} elements", elems.len(), n_fixed));
                }
                let fields = self.fields_with_rest(pats, &elems, rest, def_bm, b)?;
                Ok(mk(T::Adt(None, fields), expected))
            }
            P::TupleStruct(path, pats) => {
                let (ctor, substs) = match self.resolve_value_path(path, span)? {
                    ValueRes::Item(d, s) if matches!(self.cx.db.def(d).kind, DefKind::Struct(_) | DefKind::Variant(_)) => (d, s),
                    ValueRes::Unit(d, s) => (d, s),
                    _ => return err(span, format!("expected tuple struct or tuple variant, found `{}`", path.last())),
                };
                let (adt, vi) = self.adt_of(ctor);
                let info = self.cx.adt(adt)?;
                let aty = Ty::Adt(adt, substs.clone().into());
                self.expect_eq(expected, &aty, span)?;
                let ftys: Vec<Ty> = info.variants[vi].fields.iter().map(|(_, t)| t.subst(&substs)).collect();
                if let FieldsKind::Named = info.variants[vi].kind {
                    return err(span, format!("expected tuple struct or tuple variant, found struct variant `{}`", path.last()));
                }
                let rest = pats.iter().position(|x| matches!(x.kind, P::Rest));
                let n_fixed = pats.len() - rest.is_some() as usize;
                if (rest.is_none() && ftys.len() != pats.len()) || ftys.len() < n_fixed {
                    return err(
                        span,
                        format!(
                            "this pattern has {} field{}, but the corresponding tuple variant has {} field{}",
                            pats.len(),
                            if pats.len() == 1 { "" } else { "s" },
                            ftys.len(),
                            if ftys.len() == 1 { "" } else { "s" }
                        ),
                    );
                }
                let fields = self.fields_with_rest(pats, &ftys, rest, def_bm, b)?;
                Ok(mk(T::Adt(Some(ctor), fields), expected))
            }
            P::Struct(path, fields, _rest) => {
                let (ctor, substs) = self.resolve_struct_path(path, span)?;
                let (adt, vi) = self.adt_of(ctor);
                let info = self.cx.adt(adt)?;
                let aty = Ty::Adt(adt, substs.clone().into());
                self.expect_eq(expected, &aty, span)?;
                let mut out = Vec::new();
                for (n, fp) in fields {
                    let fi = match info.field_index(vi, n) {
                        Some(i) => i,
                        None => return err(fp.span, format!("variant `{}` does not have a field named `{}`", info.variants[vi].name, n)),
                    };
                    let fty = info.variants[vi].fields[fi].1.subst(&substs);
                    out.push((fi, self.pat(fp, &fty, def_bm, b)?));
                }
                Ok(mk(T::Adt(Some(ctor), out), expected))
            }
            P::Path(path) => self.path_pat(path, expected, span),
            P::Ref(inner, m) => {
                let t = self.shallow(expected);
                let it = match &t {
                    Ty::Ref(_, x) => (**x).clone(),
                    t if t.is_var() => {
                        let v = self.fresh();
                        let r = Ty::reference(*m, v.clone());
                        self.unify(t, &r);
                        v
                    }
                    _ => return err(span, format!("mismatched types: expected `{}`, found `&_`", self.name(&t))),
                };
                let ip = self.pat(inner, &it, None, b)?;
                Ok(mk(T::Deref(Box::new(ip)), expected))
            }
            P::Box(inner) => {
                let t = self.shallow(expected);
                let it = match &t {
                    Ty::Adt(d, a) if self.cx.db.is_lang(*d, "Box") => a[0].clone(),
                    _ => return err(span, String::from("box pattern on a non-Box type")),
                };
                let ip = self.pat(inner, &it, def_bm, b)?;
                Ok(mk(T::Deref(Box::new(ip)), expected))
            }
            P::Or(alts) => {
                let mut out = Vec::new();
                let start = b.list.len();
                let saved = b.reuse.take();
                for (i, a) in alts.iter().enumerate() {
                    if i == 0 {
                        out.push(self.pat(a, expected, def_bm, b)?);
                    } else {
                        let first: Vec<(Name, tir::LocalId)> = b.list[start..].to_vec();
                        let mut nb = Binds {
                            list: Vec::new(),
                            reuse: Some(first.clone()),
                        };
                        out.push(self.pat(a, expected, def_bm, &mut nb)?);
                        if nb.list.len() != first.len() {
                            return err(a.span, String::from("variable is not bound in all patterns"));
                        }
                    }
                }
                b.reuse = saved;
                Ok(mk(T::Or(out), expected))
            }
            P::Slice(pats) => {
                let t = self.shallow(expected);
                let (elem, len) = match &t {
                    Ty::Array(e, _) => ((**e).clone(), t.array_len()),
                    Ty::Slice(e) => ((**e).clone(), None),
                    _ => {
                        return err(span, format!("expected an array or slice, found `{}`", self.name(&t)));
                    }
                };
                let rest = pats.iter().position(|x| matches!(x.kind, P::Rest) || matches!(&x.kind, P::Ident { sub: Some(s), .. } if matches!(s.kind, P::Rest)));
                let (pre, suf) = match rest {
                    Some(r) => (&pats[..r], &pats[r + 1..]),
                    None => (&pats[..], &pats[pats.len()..]),
                };
                if let Some(n) = len {
                    let fixed = (pre.len() + suf.len()) as u64;
                    if (rest.is_none() && n != fixed) || n < fixed {
                        return err(span, format!("pattern requires {} elements but array has {}", fixed, n));
                    }
                }
                let mut prefix = Vec::new();
                for x in pre {
                    prefix.push(self.pat(x, &elem, def_bm, b)?);
                }
                let rest_pat = match rest {
                    Some(r) => match &pats[r].kind {
                        P::Ident { name, mutable, .. } => {
                            let rt = match len {
                                Some(n) => Ty::array(elem.clone(), n - (pre.len() + suf.len()) as u64),
                                None => Ty::Slice(Rc::new(elem.clone())),
                            };
                            let mode = match (def_bm, len) {
                                (Some(m), _) => Some(m),
                                (None, None) => return err(span, String::from("cannot bind a slice by value; match on a reference")),
                                (None, Some(_)) => None,
                            };
                            let lty = match mode {
                                Some(m) => Ty::reference(m, rt.clone()),
                                None => rt.clone(),
                            };
                            let _ = mutable;
                            let id = self.bind_pat(name, lty, b, span)?;
                            Some(Box::new(tir::Pat {
                                kind: T::Bind(id, mode, None),
                                ty: rt,
                            }))
                        }
                        _ => Some(Box::new(tir::Pat {
                            kind: T::Wild,
                            ty: Ty::Slice(Rc::new(elem.clone())),
                        })),
                    },
                    None => None,
                };
                let mut suffix = Vec::new();
                for x in suf {
                    suffix.push(self.pat(x, &elem, def_bm, b)?);
                }
                Ok(mk(T::Slice(prefix, rest_pat, suffix), expected))
            }
        }
    }

    fn fields_with_rest(&mut self, pats: &[ast::Pat], tys: &[Ty], rest: Option<usize>, def_bm: Option<bool>, b: &mut Binds) -> Result<Vec<(usize, tir::Pat)>> {
        let mut out = Vec::new();
        match rest {
            None => {
                for (i, (p, t)) in pats.iter().zip(tys.iter()).enumerate() {
                    out.push((i, self.pat(p, t, def_bm, b)?));
                }
            }
            Some(r) => {
                for i in 0..r {
                    out.push((i, self.pat(&pats[i], &tys[i], def_bm, b)?));
                }
                let after = pats.len() - r - 1;
                for k in 0..after {
                    let fi = tys.len() - after + k;
                    out.push((fi, self.pat(&pats[r + 1 + k], &tys[fi], def_bm, b)?));
                }
            }
        }
        Ok(out)
    }

    fn path_pat(&mut self, path: &ast::Path, expected: &Ty, span: Span) -> Result<tir::Pat> {
        match self.resolve_value_path(path, span)? {
            ValueRes::Unit(ctor, substs) => {
                let (adt, _) = self.adt_of(ctor);
                let aty = Ty::Adt(adt, substs.into());
                self.expect_eq(expected, &aty, span)?;
                Ok(tir::Pat {
                    kind: T::Adt(Some(ctor), Vec::new()),
                    ty: expected.clone(),
                })
            }
            r @ (ValueRes::Item(..) | ValueRes::TraitItem(..) | ValueRes::Static(_)) => {
                if let ValueRes::Item(d, _) = &r {
                    if let DefKind::Struct(_) | DefKind::Variant(_) | DefKind::Fn(_) = self.cx.db.def(*d).kind {
                        return err(span, format!("expected unit struct, unit variant or constant, found `{}`", path.last()));
                    }
                }
                let v = self.value_res_expr(r, span)?;
                let vt = v.ty.clone();
                self.expect_eq(expected, &vt, span)?;
                Ok(tir::Pat {
                    kind: T::Const(Box::new(v)),
                    ty: expected.clone(),
                })
            }
            _ => err(span, format!("expected unit struct, unit variant or constant, found `{}`", path.last())),
        }
    }

    /// Whether `name` in a pattern refers to a constant-like item.
    pub fn pattern_item(&self, name: &str) -> Option<DefId> {
        match self.cx.db.lookup_scope(self.module, name, Ns::Value) {
            Some(Res::Def(d)) => Some(d),
            _ => None,
        }
    }
}

