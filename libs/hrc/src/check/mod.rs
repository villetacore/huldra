//! Type checking and monomorphization.
//!
//! Functions are checked per instantiation ("templates"): starting from
//! `main`, every function, method and closure is type-checked with its
//! generic arguments substituted, which yields fully typed IR ([`tir`]).
//! Trait methods are resolved to impls when the instance is finished.

pub mod derive;
mod desugar;
mod expr;
mod infer;
mod lower;
mod method;
mod mono;
mod pat;

use crate::ast::{self, Name};
use crate::resolve::{Db, DefId, DefKind};
use crate::tir;
use crate::ty::Ty;
use crate::{err, Error, Result, Sources, Span};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

pub use infer::FnCtx;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TraitRef {
    pub trait_: DefId,
    /// Arguments without Self.
    pub args: Rc<[Ty]>,
}

impl TraitRef {
    pub fn subst(&self, s: &[Ty]) -> TraitRef {
        TraitRef {
            trait_: self.trait_,
            args: crate::ty::subst_list(&self.args, s),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Pred {
    Trait(Ty, TraitRef),
    /// `<ty as trait>::name == value`
    Proj(Ty, TraitRef, Name, Ty),
}

impl Pred {
    pub fn subst(&self, s: &[Ty]) -> Pred {
        match self {
            Pred::Trait(t, tr) => Pred::Trait(t.subst(s), tr.subst(s)),
            Pred::Proj(t, tr, n, v) => Pred::Proj(t.subst(s), tr.subst(s), n.clone(), v.subst(s)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GenericParamInfo {
    pub name: Name,
    pub is_const: bool,
    /// The AST parameter (for defaults); synthetic `impl Trait` ones have
    /// their bounds here instead.
    pub ast: Option<ast::GenericParam>,
    pub impl_bounds: Vec<ast::Bound>,
}

#[derive(Clone, Debug)]
pub struct Generics {
    /// All parameters visible in the item: the parent's first.
    pub params: Vec<GenericParamInfo>,
    /// Index of the first parameter of the item itself.
    pub own_start: usize,
    /// Where clauses of the item and its parent.
    pub where_: Vec<(ast::Type, Vec<ast::Bound>)>,
    pub has_self: bool,
}

impl Generics {
    pub fn count(&self) -> usize {
        self.params.len()
    }

    pub fn own_count(&self) -> usize {
        self.params.len() - self.own_start
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.params.iter().rposition(|p| &*p.name == name)
    }

    pub fn identity(&self) -> Rc<[Ty]> {
        (0..self.params.len()).map(|i| Ty::Param(i as u32)).collect::<Vec<_>>().into()
    }
}

#[derive(Clone, Debug)]
pub struct FnSig {
    pub generics: Rc<Generics>,
    pub params: Vec<Ty>,
    pub ret: Ty,
    pub preds: Vec<Pred>,
    pub self_kind: Option<ast::SelfKind>,
    pub variadic: bool,
    pub track_caller: bool,
}

#[derive(Clone, Debug)]
pub enum FieldsKind {
    Named,
    Tuple,
    Unit,
}

#[derive(Clone, Debug)]
pub struct VariantInfo {
    /// The struct or variant item.
    pub def: DefId,
    pub name: Name,
    pub fields: Vec<(Name, Ty)>,
    pub kind: FieldsKind,
    pub disc: i128,
    /// Fields hidden outside the standard library (private std fields).
    pub hidden: Vec<bool>,
}

#[derive(Clone, Debug)]
pub struct AdtInfo {
    pub generics: Rc<Generics>,
    pub variants: Vec<VariantInfo>,
    pub is_enum: bool,
}

impl AdtInfo {
    pub fn field_index(&self, v: usize, name: &str) -> Option<usize> {
        self.variants[v].fields.iter().position(|(n, _)| &**n == name)
    }
}

#[derive(Clone, Debug)]
pub struct ImplHeader {
    pub generics: Rc<Generics>,
    pub self_ty: Ty,
    pub trait_ref: Option<TraitRef>,
    pub preds: Vec<Pred>,
}

#[derive(Clone, Debug)]
pub struct TraitInfo {
    pub generics: Rc<Generics>,
    /// Supertraits (Self is `Param(0)`).
    pub supers: Vec<TraitRef>,
    /// Bounds on associated types and the trait's where clauses.
    pub preds: Vec<Pred>,
}

/// One monomorphized function (or constant).
pub struct Instance {
    pub def: DefId,
    pub substs: Rc<[Ty]>,
    pub state: InstState,
}

pub enum InstState {
    Pending,
    Checking,
    Done(tir::Function, Ty),
}

/// The global checking context.
pub struct Ctx<'a> {
    pub db: &'a Db,
    pub sources: &'a Sources,
    generics: BTreeMap<DefId, Rc<Generics>>,
    sigs: BTreeMap<DefId, Rc<FnSig>>,
    adts: BTreeMap<DefId, Rc<AdtInfo>>,
    impl_headers: BTreeMap<DefId, Rc<ImplHeader>>,
    traits: BTreeMap<DefId, Rc<TraitInfo>>,
    /// Trait impls by trait; inherent impls separately.
    impls_by_trait: BTreeMap<DefId, Vec<DefId>>,
    inherent_impls: Vec<DefId>,
    pub instances: Vec<Instance>,
    inst_map: BTreeMap<(DefId, Rc<[Ty]>), tir::InstId>,
    pub closures: Vec<Option<tir::Closure>>,
    pub vtables: Vec<tir::Vtable>,
    vtable_map: BTreeMap<(DefId, Ty), usize>,
    pub statics: Vec<tir::Static>,
    static_map: BTreeMap<DefId, usize>,
    /// Instances whose bodies still need monomorphization.
    queue: Vec<tir::InstId>,
    /// Labels allocated across all functions.
    pub labels: u32,
    pub assoc_cache: BTreeMap<DefId, Ty>,
    /// Closure signatures, once their creating function is checked.
    pub closure_sigs: Vec<Option<(Vec<Ty>, Ty)>>,
    /// Normalized field types (see `tir::Program::adt_fields`).
    pub adt_fields: BTreeMap<Ty, Vec<Vec<Ty>>>,
}

impl<'a> Ctx<'a> {
    pub fn new(db: &'a Db, sources: &'a Sources) -> Ctx<'a> {
        Ctx {
            db,
            sources,
            generics: BTreeMap::new(),
            sigs: BTreeMap::new(),
            adts: BTreeMap::new(),
            impl_headers: BTreeMap::new(),
            traits: BTreeMap::new(),
            impls_by_trait: BTreeMap::new(),
            inherent_impls: Vec::new(),
            instances: Vec::new(),
            inst_map: BTreeMap::new(),
            closures: Vec::new(),
            vtables: Vec::new(),
            vtable_map: BTreeMap::new(),
            statics: Vec::new(),
            static_map: BTreeMap::new(),
            queue: Vec::new(),
            labels: 0,
            assoc_cache: BTreeMap::new(),
            closure_sigs: Vec::new(),
            adt_fields: BTreeMap::new(),
        }
    }

    pub fn ty_name(&self, t: &Ty) -> String {
        crate::ty::display(t, &|d| self.db.path_name(d))
    }

    pub fn trait_ref_name(&self, self_ty: &Ty, tr: &TraitRef) -> String {
        let mut s = format!("{}: {}", self.ty_name(self_ty), self.db.def(tr.trait_).name);
        if !tr.args.is_empty() {
            let a: Vec<String> = tr.args.iter().map(|t| self.ty_name(t)).collect();
            s.push('<');
            s.push_str(&a.join(", "));
            s.push('>');
        }
        s
    }

    // ---- item information ----

    /// Generic parameters of an item (including its impl's or trait's).
    pub fn generics_of(&mut self, def: DefId) -> Rc<Generics> {
        if let Some(g) = self.generics.get(&def) {
            return g.clone();
        }
        let d = self.db.def(def).clone();
        let parent = match (&d.kind, d.parent) {
            (DefKind::Variant(_), _) => None,
            (_, Some(p)) => Some(self.generics_of(p)),
            _ => None,
        };
        let mut g = match &parent {
            Some(p) => Generics {
                params: p.params.clone(),
                own_start: p.params.len(),
                where_: p.where_.clone(),
                has_self: p.has_self,
            },
            None => Generics {
                params: Vec::new(),
                own_start: 0,
                where_: Vec::new(),
                has_self: false,
            },
        };
        let own: Option<&ast::Generics> = match &d.kind {
            DefKind::Fn(f) | DefKind::ExternFn(f, _) => Some(&f.generics),
            DefKind::Struct(s) => Some(&s.generics),
            DefKind::Enum(e) => Some(&e.generics),
            DefKind::Trait(t) => {
                g.has_self = true;
                g.params.push(GenericParamInfo {
                    name: Rc::from("Self"),
                    is_const: false,
                    ast: None,
                    impl_bounds: Vec::new(),
                });
                g.own_start = 1;
                Some(&t.generics)
            }
            DefKind::Impl(i) => Some(&i.generics),
            DefKind::TypeAlias(gen, _) => Some(gen),
            _ => None,
        };
        if let Some(own) = own {
            for p in &own.params {
                g.params.push(GenericParamInfo {
                    name: p.name.clone(),
                    is_const: p.const_ty.is_some(),
                    ast: Some(p.clone()),
                    impl_bounds: Vec::new(),
                });
            }
            g.where_.extend(own.where_.iter().cloned());
        }
        // `impl Trait` in argument position becomes an anonymous parameter.
        if let DefKind::Fn(f) = &d.kind {
            for p in &f.params {
                collect_impl_trait(&p.ty, &mut |bounds| {
                    let n = g.params.len();
                    g.params.push(GenericParamInfo {
                        name: Rc::from(format!("impl#{}", n).as_str()),
                        is_const: false,
                        ast: None,
                        impl_bounds: bounds.to_vec(),
                    });
                });
            }
        }
        let g = Rc::new(g);
        self.generics.insert(def, g.clone());
        g
    }

    /// The scope for lowering types in an item.
    pub fn item_scope(&mut self, def: DefId) -> Result<lower::Scope> {
        let d = self.db.def(def).clone();
        let generics = self.generics_of(def);
        let mut sc = lower::Scope {
            module: d.module,
            generics,
            self_ty: None,
            trait_def: None,
            impl_def: None,
        };
        let container = match d.kind {
            DefKind::Impl(_) | DefKind::Trait(_) => Some(def),
            _ => d.parent,
        };
        if let Some(c) = container {
            match &self.db.def(c).kind {
                DefKind::Impl(_) => {
                    let h = self.impl_header(c)?;
                    sc.self_ty = Some(h.self_ty.clone());
                    sc.impl_def = Some(c);
                }
                DefKind::Trait(_) => {
                    sc.self_ty = Some(Ty::Param(0));
                    sc.trait_def = Some(c);
                }
                _ => {}
            }
        }
        if let DefKind::Struct(_) | DefKind::Enum(_) = d.kind {
            let g = sc.generics.clone();
            sc.self_ty = Some(Ty::Adt(def, g.identity()));
        }
        Ok(sc)
    }

    pub fn impl_header(&mut self, imp: DefId) -> Result<Rc<ImplHeader>> {
        if let Some(h) = self.impl_headers.get(&imp) {
            return Ok(h.clone());
        }
        let ast = match &self.db.def(imp).kind {
            DefKind::Impl(i) => i.clone(),
            _ => unreachable!(),
        };
        let generics = self.generics_of(imp);
        let mut sc = lower::Scope {
            module: self.db.def(imp).module,
            generics: generics.clone(),
            self_ty: None,
            trait_def: None,
            impl_def: None,
        };
        let self_ty = self.lower_ty(&sc, &ast.self_ty, &mut lower::NoInfer)?;
        sc.self_ty = Some(self_ty.clone());
        let trait_ref = match &ast.trait_ {
            Some(p) => {
                let (tr, _bindings) = self.lower_trait_ref(&sc, p, &self_ty, &mut lower::NoInfer)?;
                Some(tr)
            }
            None => None,
        };
        let h = Rc::new(ImplHeader {
            generics: generics.clone(),
            self_ty: self_ty.clone(),
            trait_ref,
            preds: Vec::new(),
        });
        // Insert before lowering predicates (they may refer to the impl).
        self.impl_headers.insert(imp, h.clone());
        let preds = self.lower_preds(&sc)?;
        let h = Rc::new(ImplHeader {
            generics,
            self_ty,
            trait_ref: h.trait_ref.clone(),
            preds,
        });
        self.impl_headers.insert(imp, h.clone());
        Ok(h)
    }

    pub fn trait_info(&mut self, tr: DefId) -> Result<Rc<TraitInfo>> {
        if let Some(t) = self.traits.get(&tr) {
            return Ok(t.clone());
        }
        let ast = match &self.db.def(tr).kind {
            DefKind::Trait(t) => t.clone(),
            _ => return err(self.db.def(tr).span, format!("`{}` is not a trait", self.db.def(tr).name)),
        };
        let generics = self.generics_of(tr);
        let info = Rc::new(TraitInfo {
            generics: generics.clone(),
            supers: Vec::new(),
            preds: Vec::new(),
        });
        self.traits.insert(tr, info);
        let sc = self.item_scope(tr)?;
        let mut supers = Vec::new();
        for b in &ast.supertraits {
            if let Some((t, _)) = self.lower_trait_ref_opt(&sc, b, &Ty::Param(0), &mut lower::NoInfer)? {
                supers.push(t);
            }
        }
        let mut preds = Vec::new();
        for c in self.db.def(tr).children.clone() {
            if let DefKind::AssocType(bounds, _) = &self.db.def(c).kind {
                let name = self.db.def(c).name.clone();
                let subject = Ty::Proj(tr, core::iter::once(Ty::Param(0)).chain(generics.identity().iter().skip(1).cloned()).collect::<Vec<_>>().into(), name);
                for b in bounds {
                    self.lower_bound(&sc, &subject, b, &mut preds, &mut lower::NoInfer)?;
                }
            }
        }
        let info = Rc::new(TraitInfo { generics, supers, preds });
        self.traits.insert(tr, info.clone());
        Ok(info)
    }

    /// Supertraits of a trait reference, transitively (including itself).
    pub fn elaborate(&mut self, tr: &TraitRef, self_ty: &Ty) -> Result<Vec<TraitRef>> {
        let mut out = alloc::vec![tr.clone()];
        let mut i = 0;
        while i < out.len() && out.len() < 64 {
            let t = out[i].clone();
            let info = self.trait_info(t.trait_)?;
            let mut s: Vec<Ty> = alloc::vec![self_ty.clone()];
            s.extend(t.args.iter().cloned());
            for sup in &info.supers {
                let st = sup.subst(&s);
                if !out.iter().any(|o| o.trait_ == st.trait_) {
                    out.push(st);
                }
            }
            i += 1;
        }
        Ok(out)
    }

    pub fn adt(&mut self, def: DefId) -> Result<Rc<AdtInfo>> {
        if let Some(a) = self.adts.get(&def) {
            return Ok(a.clone());
        }
        let d = self.db.def(def).clone();
        let generics = self.generics_of(def);
        let sc = self.item_scope(def)?;
        // Insert a placeholder for recursive types.
        let lower_fields = |cx: &mut Ctx, data: &ast::VariantData| -> Result<(Vec<(Name, Ty)>, FieldsKind)> {
            Ok(match data {
                ast::VariantData::Named(f) => {
                    let mut out = Vec::new();
                    for (n, t) in f {
                        out.push((n.clone(), cx.lower_ty(&sc, t, &mut lower::NoInfer)?));
                    }
                    (out, FieldsKind::Named)
                }
                ast::VariantData::Tuple(f) => {
                    let mut out = Vec::new();
                    for (i, t) in f.iter().enumerate() {
                        out.push((Rc::from(format!("{}", i).as_str()), cx.lower_ty(&sc, t, &mut lower::NoInfer)?));
                    }
                    (out, FieldsKind::Tuple)
                }
                ast::VariantData::Unit => (Vec::new(), FieldsKind::Unit),
            })
        };
        let info = match &d.kind {
            DefKind::Struct(s) => {
                let (fields, kind) = lower_fields(self, &s.data)?;
                AdtInfo {
                    generics,
                    variants: alloc::vec![VariantInfo {
                        def,
                        name: d.name.clone(),
                        hidden: (0..fields.len()).map(|i| d.in_std && !s.public.get(i).copied().unwrap_or(true)).collect(),
                        fields,
                        kind,
                        disc: 0,
                    }],
                    is_enum: false,
                }
            }
            DefKind::Enum(e) => {
                let mut variants = Vec::new();
                let mut next: i128 = 0;
                for (i, v) in e.variants.iter().enumerate() {
                    let (fields, kind) = lower_fields(self, &v.data)?;
                    let disc = match &v.disc {
                        Some(x) => self.const_eval(d.module, x)?,
                        None => next,
                    };
                    next = disc + 1;
                    variants.push(VariantInfo {
                        def: self.db.def(def).children[i],
                        name: v.name.clone(),
                        hidden: alloc::vec![false; fields.len()],
                        fields,
                        kind,
                        disc,
                    });
                }
                AdtInfo {
                    generics,
                    variants,
                    is_enum: true,
                }
            }
            _ => return err(d.span, format!("`{}` is not a struct or enum", d.name)),
        };
        let info = Rc::new(info);
        self.adts.insert(def, info.clone());
        Ok(info)
    }

    pub fn sig(&mut self, def: DefId) -> Result<Rc<FnSig>> {
        if let Some(s) = self.sigs.get(&def) {
            return Ok(s.clone());
        }
        let d = self.db.def(def).clone();
        let f = match &d.kind {
            DefKind::Fn(f) | DefKind::ExternFn(f, _) => f.clone(),
            _ => return err(d.span, format!("`{}` is not a function", d.name)),
        };
        let sc = self.item_scope(def)?;
        let mut params = Vec::new();
        if let Some((kind, _)) = f.self_param {
            let self_ty = sc.self_ty.clone().unwrap_or(Ty::Error);
            let t = match (&f.self_ty, kind) {
                (Some(t), _) => self.lower_ty(&sc, t, &mut lower::NoInfer)?,
                (None, ast::SelfKind::Value) => self_ty,
                (None, ast::SelfKind::Ref) => Ty::reference(false, self_ty),
                (None, ast::SelfKind::RefMut) => Ty::reference(true, self_ty),
            };
            params.push(t);
        }
        let mut counter = sc.generics.params.iter().position(|p| p.name.starts_with("impl#")).unwrap_or(0);
        for p in &f.params {
            let t = self.lower_ty_impl_arg(&sc, &p.ty, &mut counter)?;
            params.push(t);
        }
        let ret = match &f.ret {
            Some(t) => {
                if let ast::TypeKind::ImplTrait(_) = t.kind {
                    Ty::Opaque(def, sc.generics.identity())
                } else {
                    self.lower_ty_ret(&sc, t, def)?
                }
            }
            None => crate::ty::unit(),
        };
        let preds = self.lower_preds(&sc)?;
        let sig = Rc::new(FnSig {
            generics: sc.generics.clone(),
            params,
            ret,
            preds,
            self_kind: f.self_param.map(|s| s.0),
            variadic: f.variadic,
            track_caller: d.attrs.has("track_caller"),
        });
        self.sigs.insert(def, sig.clone());
        Ok(sig)
    }

    /// Impls of a trait.
    pub fn impls_of(&mut self, tr: DefId) -> Result<Vec<DefId>> {
        if self.impls_by_trait.is_empty() && self.inherent_impls.is_empty() {
            for &i in &self.db.impls.clone() {
                let h = self.impl_header(i)?;
                match &h.trait_ref {
                    Some(t) => self.impls_by_trait.entry(t.trait_).or_default().push(i),
                    None => self.inherent_impls.push(i),
                }
            }
        }
        Ok(self.impls_by_trait.get(&tr).cloned().unwrap_or_default())
    }

    pub fn inherent_impls(&mut self) -> Result<Vec<DefId>> {
        self.impls_of(u32::MAX)?;
        Ok(self.inherent_impls.clone())
    }

    /// Evaluates an integer constant expression (array lengths, enum
    /// discriminants).
    pub fn const_eval(&mut self, module: crate::resolve::ModId, e: &ast::Expr) -> Result<i128> {
        use ast::ExprKind as E;
        Ok(match &e.kind {
            E::Lit(ast::Lit::Int(v, _)) => *v as i128,
            E::Lit(ast::Lit::Bool(b)) => *b as i128,
            E::Lit(ast::Lit::Char(c)) => *c as i128,
            E::Lit(ast::Lit::Byte(b)) => *b as i128,
            E::Paren(a) | E::Cast(a, _) => self.const_eval(module, a)?,
            E::Unary(ast::UnOp::Neg, a) => -self.const_eval(module, a)?,
            E::Unary(ast::UnOp::Not, a) => !self.const_eval(module, a)?,
            E::Block(b, _) if b.stmts.is_empty() && b.expr.is_some() => self.const_eval(module, b.expr.as_ref().unwrap())?,
            E::Binary(op, a, b) => {
                let x = self.const_eval(module, a)?;
                let y = self.const_eval(module, b)?;
                use ast::BinOp::*;
                match op {
                    Add => x + y,
                    Sub => x - y,
                    Mul => x * y,
                    Div if y != 0 => x / y,
                    Rem if y != 0 => x % y,
                    BitAnd => x & y,
                    BitOr => x | y,
                    BitXor => x ^ y,
                    Shl => x << (y as u32 & 127),
                    Shr => x >> (y as u32 & 127),
                    _ => return err(e.span, String::from("unsupported operation in a constant")),
                }
            }
            E::Path(p) => {
                let names: Vec<Name> = p.segs.iter().map(|s| s.name.clone()).collect();
                let (m, i) = self.db.resolve_module_prefix(module, &names, p.global);
                if i + 1 != names.len() {
                    return err(e.span, String::from("this path is not supported in a constant"));
                }
                match self.db.lookup_seg(m, i, &names[i], crate::resolve::Ns::Value) {
                    Some(crate::resolve::Res::Def(d)) => match &self.db.def(d).kind {
                        DefKind::Const(_, ce) => {
                            let ce = ce.clone();
                            let dm = self.db.def(d).module;
                            self.const_eval(dm, &ce)?
                        }
                        _ => return err(e.span, format!("`{}` is not a constant", names[i])),
                    },
                    _ => return err(e.span, format!("cannot find value `{}` in this scope", names[i])),
                }
            }
            _ => return err(e.span, String::from("this expression is not supported in a constant")),
        })
    }

    // ---- instances ----

    /// The instance id of a function (or constant) with concrete substs.
    pub fn instance(&mut self, def: DefId, substs: Rc<[Ty]>) -> tir::InstId {
        let key = (def, substs.clone());
        if let Some(i) = self.inst_map.get(&key) {
            return *i;
        }
        let id = self.instances.len() as tir::InstId;
        self.instances.push(Instance {
            def,
            substs,
            state: InstState::Pending,
        });
        self.inst_map.insert(key, id);
        self.queue.push(id);
        id
    }

    /// Checks an instance now (if not done yet); returns its return type.
    pub fn check_instance(&mut self, id: tir::InstId, span: Span) -> Result<Ty> {
        match &self.instances[id as usize].state {
            InstState::Done(_, t) => return Ok(t.clone()),
            InstState::Checking => {
                return err(span, String::from("cannot infer an `impl Trait` type that depends on itself"));
            }
            InstState::Pending => {}
        }
        self.instances[id as usize].state = InstState::Checking;
        let def = self.instances[id as usize].def;
        let substs = self.instances[id as usize].substs.clone();
        let (f, ret) = infer::check_fn(self, id, def, substs)?;
        self.instances[id as usize].state = InstState::Done(f, ret.clone());
        Ok(ret)
    }

    /// The vtable of a type for a trait.
    pub fn vtable(&mut self, tr: DefId, ty: Ty) -> usize {
        if let Some(v) = self.vtable_map.get(&(tr, ty.clone())) {
            return *v;
        }
        let id = self.vtables.len();
        self.vtables.push(tir::Vtable {
            trait_: tr,
            ty: ty.clone(),
            methods: Vec::new(),
            fn_inst: None,
        });
        self.vtable_map.insert((tr, ty), id);
        id
    }

    pub fn static_index(&mut self, def: DefId) -> Result<usize> {
        if let Some(i) = self.static_map.get(&def) {
            return Ok(*i);
        }
        let i = self.statics.len();
        self.static_map.insert(def, i);
        self.statics.push(tir::Static {
            def,
            ty: Ty::Error,
            init: tir::Expr {
                kind: tir::ExprKind::Unit,
                ty: Ty::Error,
                span: Span::default(),
            },
            locals: Vec::new(),
        });
        let (ty, init, locals) = infer::check_static(self, def)?;
        self.statics[i] = tir::Static { def, ty, init, locals };
        Ok(i)
    }

    pub fn new_label(&mut self) -> u32 {
        self.labels += 1;
        self.labels
    }
}

fn collect_impl_trait(t: &ast::Type, f: &mut dyn FnMut(&[ast::Bound])) {
    use ast::TypeKind as K;
    match &t.kind {
        K::ImplTrait(b) => f(b),
        K::Ref(_, t) | K::Ptr(_, t) | K::Slice(t) | K::Array(t, _) => collect_impl_trait(t, f),
        K::Tuple(ts) => ts.iter().for_each(|t| collect_impl_trait(t, f)),
        K::Path(p) => {
            for s in &p.segs {
                if let Some(a) = &s.args {
                    a.types.iter().for_each(|t| collect_impl_trait(t, f));
                }
            }
        }
        _ => {}
    }
}

/// Checks the program and monomorphizes everything reachable from `main`.
pub fn check_program(db: &Db, sources: &Sources, main_file: u16) -> Result<tir::Program> {
    let mut cx = Ctx::new(db, sources);
    let main = match db.mods[db.crate_root as usize].values.get("main") {
        Some(&d) if matches!(db.def(d).kind, DefKind::Fn(_)) => d,
        _ => {
            return err(
                Span {
                    file: main_file,
                    line: 1,
                    col: 1,
                },
                String::from("`main` function not found in crate"),
            )
        }
    };
    let sig = cx.sig(main)?;
    if sig.generics.count() > 0 || !sig.params.is_empty() {
        return err(db.def(main).span, String::from("`main` function has wrong type: it must take no arguments and have no generic parameters"));
    }
    let main_id = cx.instance(main, Rc::from([]));
    mono::run(&mut cx)?;
    // `main() -> Result<(), E>`: report errors with `Debug`.
    let ret = match &cx.instances[main_id as usize].state {
        InstState::Done(_, t) => t.clone(),
        _ => Ty::Error,
    };
    let mut main_result = None;
    if !ret.is_unit() {
        match &ret {
            Ty::Adt(d, args) if db.is_lang(*d, "Result") => {
                let report = lower::std_fn(db, &["rt", "report_main_error"]).ok_or_else(|| Error::new(Span::default(), String::from("std has no rt::report_main_error")))?;
                let id = cx.instance(report, Rc::from([args[0].clone(), args[1].clone()]));
                main_result = Some(id);
                mono::run(&mut cx)?;
            }
            _ => {
                return err(
                    db.def(main).span,
                    format!("`main` has invalid return type `{}`", cx.ty_name(&ret)),
                )
            }
        }
    }
    mono::finish(cx, main_id, main_result)
}
