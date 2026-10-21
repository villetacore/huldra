//! The item database: every item of the standard library and the program
//! gets a [`DefId`]; modules map names to items in two namespaces (types
//! and values) and resolve `use` declarations lazily.

use crate::ast::*;
use crate::{err, Error, Result, Span};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

pub type DefId = u32;
pub type ModId = u32;

#[derive(Clone, Debug)]
pub enum DefKind {
    Mod(ModId),
    Fn(Rc<FnDecl>),
    ExternFn(Rc<FnDecl>, Name),
    Struct(Rc<StructDef>),
    Enum(Rc<EnumDef>),
    /// A variant of `parent` (an enum), by index.
    Variant(usize),
    Trait(Rc<TraitDef>),
    Impl(Rc<ImplDef>),
    Const(Type, Rc<Expr>),
    Static(bool, Type, Rc<Expr>),
    TypeAlias(Generics, Type),
    /// `type Name: Bounds = Default;` in a trait.
    AssocType(Vec<Bound>, Option<Type>),
    /// `type Name = T;` in an impl.
    ImplType(Type),
    /// `const N: T = e;` in a trait (default optional) or impl.
    AssocConst(Type, Option<Rc<Expr>>),
}

#[derive(Clone, Debug)]
pub struct Def {
    pub name: Name,
    pub kind: DefKind,
    /// The module whose names are visible in the item.
    pub module: ModId,
    /// The impl or trait of an associated item, the enum of a variant.
    pub parent: Option<DefId>,
    pub span: Span,
    pub attrs: Attrs,
    /// Associated items (impls and traits) by name.
    pub children: Vec<DefId>,
    pub in_std: bool,
}

#[derive(Clone, Debug)]
pub struct UseDecl {
    pub path: Vec<Name>,
    pub alias: Name,
    pub span: Span,
}

#[derive(Clone, Debug, Default)]
pub struct Module {
    pub name: Name,
    pub parent: Option<ModId>,
    /// Anonymous block modules see their parent's names.
    pub transparent: bool,
    pub types: BTreeMap<Name, DefId>,
    pub values: BTreeMap<Name, DefId>,
    pub mods: BTreeMap<Name, ModId>,
    pub uses: Vec<UseDecl>,
    pub globs: Vec<(Vec<Name>, Span)>,
    pub krate: ModId,
    pub in_std: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ns {
    Type,
    Value,
}

/// What a path resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Res {
    Def(DefId),
    Mod(ModId),
}

/// Items the compiler knows by name.
#[derive(Default, Debug, Clone)]
pub struct Lang {
    pub items: BTreeMap<&'static str, DefId>,
}

pub struct Db {
    pub defs: Vec<Def>,
    pub mods: Vec<Module>,
    pub impls: Vec<DefId>,
    pub std_root: ModId,
    pub crate_root: ModId,
    pub prelude: Option<ModId>,
    /// Item scopes of blocks that contain items, by block address.
    pub block_mods: BTreeMap<usize, ModId>,
    pub lang: Lang,
    /// Trait methods by name (for method lookup).
    pub trait_methods: BTreeMap<Name, Vec<DefId>>,
    resolving: RefCell<Vec<(ModId, Name)>>,
}

const LANG_PATHS: &[(&str, &str)] = &[
    ("Option", "option::Option"),
    ("Some", "option::Option::Some"),
    ("None", "option::Option::None"),
    ("Result", "result::Result"),
    ("Ok", "result::Result::Ok"),
    ("Err", "result::Result::Err"),
    ("String", "string::String"),
    ("Vec", "vec::Vec"),
    ("Box", "boxed::Box"),
    ("Formatter", "fmt::Formatter"),
    ("Arguments", "fmt::Arguments"),
    ("Location", "panic::Location"),
    ("Range", "ops::Range"),
    ("RangeInclusive", "ops::RangeInclusive"),
    ("RangeFrom", "ops::RangeFrom"),
    ("RangeTo", "ops::RangeTo"),
    ("RangeToInclusive", "ops::RangeToInclusive"),
    ("RangeFull", "ops::RangeFull"),
    ("Copy", "marker::Copy"),
    ("Sized", "marker::Sized"),
    ("Clone", "clone::Clone"),
    ("Debug", "fmt::Debug"),
    ("Display", "fmt::Display"),
    ("LowerHex", "fmt::LowerHex"),
    ("UpperHex", "fmt::UpperHex"),
    ("Octal", "fmt::Octal"),
    ("Binary", "fmt::Binary"),
    ("LowerExp", "fmt::LowerExp"),
    ("UpperExp", "fmt::UpperExp"),
    ("PartialEq", "cmp::PartialEq"),
    ("PartialOrd", "cmp::PartialOrd"),
    ("Add", "ops::Add"),
    ("Sub", "ops::Sub"),
    ("Mul", "ops::Mul"),
    ("Div", "ops::Div"),
    ("Rem", "ops::Rem"),
    ("BitAnd", "ops::BitAnd"),
    ("BitOr", "ops::BitOr"),
    ("BitXor", "ops::BitXor"),
    ("Shl", "ops::Shl"),
    ("Shr", "ops::Shr"),
    ("Neg", "ops::Neg"),
    ("Not", "ops::Not"),
    ("AddAssign", "ops::AddAssign"),
    ("SubAssign", "ops::SubAssign"),
    ("MulAssign", "ops::MulAssign"),
    ("DivAssign", "ops::DivAssign"),
    ("RemAssign", "ops::RemAssign"),
    ("BitAndAssign", "ops::BitAndAssign"),
    ("BitOrAssign", "ops::BitOrAssign"),
    ("BitXorAssign", "ops::BitXorAssign"),
    ("ShlAssign", "ops::ShlAssign"),
    ("ShrAssign", "ops::ShrAssign"),
    ("Index", "ops::Index"),
    ("IndexMut", "ops::IndexMut"),
    ("Deref", "ops::Deref"),
    ("DerefMut", "ops::DerefMut"),
    ("FnOnce", "ops::FnOnce"),
    ("FnMut", "ops::FnMut"),
    ("Fn", "ops::Fn"),
    ("Iterator", "iter::Iterator"),
    ("IntoIterator", "iter::IntoIterator"),
    ("From", "convert::From"),
];

impl Db {
    pub fn def(&self, id: DefId) -> &Def {
        &self.defs[id as usize]
    }

    pub fn lang(&self, name: &str) -> DefId {
        match self.lang.items.get(name) {
            Some(d) => *d,
            None => panic!("missing lang item {}", name),
        }
    }

    pub fn lang_opt(&self, name: &str) -> Option<DefId> {
        self.lang.items.get(name).copied()
    }

    pub fn is_lang(&self, id: DefId, name: &str) -> bool {
        self.lang.items.get(name) == Some(&id)
    }

    /// `std::fmt::Display`-like path of an item, for messages.
    pub fn path_name(&self, id: DefId) -> String {
        let d = self.def(id);
        match (&d.kind, d.parent) {
            (DefKind::Variant(_), Some(p)) => format!("{}::{}", self.def(p).name, d.name),
            _ => String::from(&*d.name),
        }
    }

    /// The child of an impl or trait with this name.
    pub fn child(&self, parent: DefId, name: &str) -> Option<DefId> {
        self.def(parent).children.iter().copied().find(|c| &*self.def(*c).name == name)
    }

    pub fn module_of_block(&self, b: &Block) -> Option<ModId> {
        self.block_mods.get(&(b as *const Block as usize)).copied()
    }

    // ---- name lookup ----

    /// Looks a name up in a module: its items, `use`s, globs, and for
    /// transparent (block) modules the parent.
    pub fn lookup(&self, m: ModId, name: &str, ns: Ns) -> Option<Res> {
        let mut cur = m;
        loop {
            if let Some(r) = self.lookup_here(cur, name, ns, 0) {
                return Some(r);
            }
            let module = &self.mods[cur as usize];
            if module.transparent {
                cur = module.parent?;
            } else {
                break;
            }
        }
        None
    }

    /// Lookup in a module and the prelude.
    pub fn lookup_scope(&self, m: ModId, name: &str, ns: Ns) -> Option<Res> {
        if let Some(r) = self.lookup(m, name, ns) {
            return Some(r);
        }
        if ns == Ns::Type {
            match name {
                "std" | "core" | "alloc" => return Some(Res::Mod(self.std_root)),
                _ => {}
            }
        }
        if let Some(p) = self.prelude {
            if let Some(r) = self.lookup_here(p, name, ns, 0) {
                return Some(r);
            }
        }
        None
    }

    fn lookup_here(&self, m: ModId, name: &str, ns: Ns, depth: u32) -> Option<Res> {
        if depth > 24 {
            return None;
        }
        let module = &self.mods[m as usize];
        match ns {
            Ns::Type => {
                if let Some(sub) = module.mods.get(name) {
                    return Some(Res::Mod(*sub));
                }
                if let Some(d) = module.types.get(name) {
                    return Some(Res::Def(*d));
                }
            }
            Ns::Value => {
                if let Some(d) = module.values.get(name) {
                    return Some(Res::Def(*d));
                }
            }
        }
        // Guard against `use` cycles.
        let key = (m, Rc::from(name));
        if self.resolving.borrow().contains(&key) {
            return None;
        }
        self.resolving.borrow_mut().push(key);
        let r = (|| {
            for u in &module.uses {
                if &*u.alias == name {
                    if let Some(r) = self.resolve_use_path(m, &u.path, ns, depth + 1) {
                        return Some(r);
                    }
                }
            }
            for (path, _) in &module.globs {
                let target = if path.is_empty() {
                    None
                } else {
                    self.resolve_use_path(m, path, Ns::Type, depth + 1)
                };
                match target {
                    Some(Res::Mod(t)) => {
                        if t != m {
                            if let Some(r) = self.lookup_here(t, name, ns, depth + 1) {
                                return Some(r);
                            }
                        }
                    }
                    Some(Res::Def(e)) => {
                        if let DefKind::Enum(_) = self.def(e).kind {
                            if let Some(v) = self.def(e).children.iter().find(|c| &*self.def(**c).name == name) {
                                return Some(Res::Def(*v));
                            }
                        }
                    }
                    None => {}
                }
            }
            None
        })();
        self.resolving.borrow_mut().pop();
        r
    }

    /// A `use` path: relative to the module (uniform paths), a crate name,
    /// `crate`, `self` or `super`.
    fn resolve_use_path(&self, m: ModId, path: &[Name], ns: Ns, depth: u32) -> Option<Res> {
        let (mut cur, rest) = self.path_start(m, path, depth)?;
        if rest.is_empty() {
            return Some(Res::Mod(cur));
        }
        for (i, seg) in rest.iter().enumerate() {
            let last = i == rest.len() - 1;
            if last {
                if let Some(r) = self.lookup_here(cur, seg, ns, depth + 1) {
                    return Some(r);
                }
                // An enum or module imported in the other namespace.
                return None;
            }
            match self.lookup_here(cur, seg, Ns::Type, depth + 1)? {
                Res::Mod(n) => cur = n,
                Res::Def(d) => {
                    // `Enum::Variant` or `Type::assoc` (only variants in `use`).
                    if let DefKind::Enum(_) = self.def(d).kind {
                        if i + 2 == rest.len() {
                            let v = self.def(d).children.iter().find(|c| &*self.def(**c).name == &*rest[i + 1])?;
                            return Some(Res::Def(*v));
                        }
                    }
                    return None;
                }
            }
        }
        None
    }

    /// The module a path starts from and the remaining segments.
    fn path_start<'p>(&self, m: ModId, path: &'p [Name], depth: u32) -> Option<(ModId, &'p [Name])> {
        let first = &*path[0];
        Some(match first {
            "crate" => (self.mods[m as usize].krate, &path[1..]),
            "self" => (self.named_module(m), &path[1..]),
            "super" => {
                let mut cur = self.named_module(m);
                let mut rest = path;
                while !rest.is_empty() && &*rest[0] == "super" {
                    cur = self.mods[cur as usize].parent?;
                    cur = self.named_module(cur);
                    rest = &rest[1..];
                }
                if !rest.is_empty() && &*rest[0] == "self" {
                    rest = &rest[1..];
                }
                (cur, rest)
            }
            "std" | "core" | "alloc" if self.lookup_here(m, first, Ns::Type, depth + 1).is_none() => (self.std_root, &path[1..]),
            _ => {
                // A name in scope: an item of this module (or a block's parent).
                let mut cur = m;
                loop {
                    match self.lookup_here(cur, first, Ns::Type, depth + 1) {
                        Some(Res::Mod(n)) => return Some((n, &path[1..])),
                        Some(Res::Def(_)) => return Some((cur, path)),
                        None => {}
                    }
                    let module = &self.mods[cur as usize];
                    if module.transparent {
                        cur = module.parent?;
                    } else {
                        break;
                    }
                }
                if let Some(p) = self.prelude {
                    if let Some(Res::Def(_)) = self.lookup_here(p, first, Ns::Type, depth + 1) {
                        return Some((p, path));
                    }
                }
                (m, path)
            }
        })
    }

    /// The nearest non-block module.
    pub fn named_module(&self, mut m: ModId) -> ModId {
        while self.mods[m as usize].transparent {
            m = self.mods[m as usize].parent.unwrap();
        }
        m
    }

    /// Resolves a module path prefix (`a::b` in `a::b::c`) for expressions
    /// and types: returns the module and the index of the first segment
    /// that is not a module.
    pub fn resolve_module_prefix(&self, m: ModId, path: &[Name], global: bool) -> (ModId, usize) {
        if path.is_empty() {
            return (m, 0);
        }
        let first = &*path[0];
        let (mut cur, mut i) = match first {
            "crate" => (self.mods[m as usize].krate, 1),
            "self" if path.len() > 1 => (self.named_module(m), 1),
            "super" => {
                let mut cur = self.named_module(m);
                let mut i = 0;
                while i < path.len() && &*path[i] == "super" {
                    cur = match self.mods[cur as usize].parent {
                        Some(p) => self.named_module(p),
                        None => cur,
                    };
                    i += 1;
                }
                (cur, i)
            }
            _ if global => match first {
                "std" | "core" | "alloc" => (self.std_root, 1),
                _ => (self.crate_root, 0),
            },
            _ => match self.lookup_scope(m, first, Ns::Type) {
                Some(Res::Mod(n)) if path.len() > 1 => (n, 1),
                _ => return (m, 0),
            },
        };
        while i + 1 < path.len() {
            match self.lookup_here(cur, &path[i], Ns::Type, 0) {
                Some(Res::Mod(n)) => {
                    cur = n;
                    i += 1;
                }
                _ => break,
            }
        }
        (cur, i)
    }

    /// Looks up the segment at `i` after `resolve_module_prefix`: in the
    /// module itself when a prefix was consumed, otherwise in scope.
    pub fn lookup_seg(&self, m: ModId, consumed: usize, name: &str, ns: Ns) -> Option<Res> {
        if consumed > 0 {
            self.lookup_here(m, name, ns, 0)
        } else {
            self.lookup_scope(m, name, ns)
        }
    }

    /// Whether a trait's methods can be called in a module: it is in the
    /// prelude, nameable there, or imported with `use ... as _`.
    pub fn trait_in_scope(&self, m: ModId, tr: DefId) -> bool {
        let name = &self.def(tr).name;
        if let Some(p) = self.prelude {
            if self.lookup_here(p, name, Ns::Type, 0) == Some(Res::Def(tr)) {
                return true;
            }
        }
        if self.lookup(m, name, Ns::Type) == Some(Res::Def(tr)) {
            return true;
        }
        let mut cur = m;
        loop {
            let module = &self.mods[cur as usize];
            for u in &module.uses {
                if &*u.alias == "_" && self.resolve_use_path(cur, &u.path, Ns::Type, 0) == Some(Res::Def(tr)) {
                    return true;
                }
            }
            // Glob imports of modules (`use std::io::prelude::*`).
            for (path, _) in &module.globs {
                if let Some(Res::Mod(g)) = self.resolve_use_path(cur, path, Ns::Type, 0) {
                    if self.lookup_here(g, name, Ns::Type, 0) == Some(Res::Def(tr)) {
                        return true;
                    }
                }
            }
            match (module.transparent, module.parent) {
                (true, Some(p)) => cur = p,
                _ => return false,
            }
        }
    }

    pub fn variants(&self, enum_: DefId) -> &[DefId] {
        &self.def(enum_).children
    }

    pub fn variant_index(&self, v: DefId) -> usize {
        match self.def(v).kind {
            DefKind::Variant(i) => i,
            _ => 0,
        }
    }
}

// ---- building ----

struct Builder {
    defs: Vec<Def>,
    mods: Vec<Module>,
    impls: Vec<DefId>,
    block_mods: BTreeMap<usize, ModId>,
    in_std: bool,
}

impl Builder {
    fn new_mod(&mut self, name: Name, parent: Option<ModId>, transparent: bool, krate: Option<ModId>) -> ModId {
        let id = self.mods.len() as ModId;
        self.mods.push(Module {
            name,
            parent,
            transparent,
            krate: krate.unwrap_or(id),
            in_std: self.in_std,
            ..Default::default()
        });
        id
    }

    fn add_def(&mut self, name: Name, kind: DefKind, module: ModId, parent: Option<DefId>, span: Span, attrs: Attrs) -> DefId {
        let id = self.defs.len() as DefId;
        self.defs.push(Def {
            name,
            kind,
            module,
            parent,
            span,
            attrs,
            children: Vec::new(),
            in_std: self.in_std,
        });
        id
    }

    fn define(&mut self, m: ModId, ns: Ns, name: &Name, id: DefId, span: Span) -> Result<()> {
        if &**name == "_" {
            return Ok(());
        }
        let map = match ns {
            Ns::Type => &mut self.mods[m as usize].types,
            Ns::Value => &mut self.mods[m as usize].values,
        };
        if map.contains_key(name) {
            return err(
                span,
                format!(
                    "the name `{}` is defined multiple times",
                    name
                ),
            );
        }
        map.insert(name.clone(), id);
        Ok(())
    }

    fn items(&mut self, m: ModId, items: Vec<Item>) -> Result<()> {
        let mut derived = Vec::new();
        for item in &items {
            if !item.attrs.derives.is_empty() {
                derived.extend(crate::check::derive::derive(item)?);
            }
        }
        for item in items.into_iter().chain(derived) {
            self.item(m, item)?;
        }
        Ok(())
    }

    fn no_attrs() -> Attrs {
        Attrs {
            derives: Vec::new(),
            names: Vec::new(),
        }
    }

    fn item(&mut self, m: ModId, item: Item) -> Result<()> {
        let span = item.span;
        let attrs = item.attrs.clone();
        match item.kind {
            ItemKind::Fn(f) => {
                self.scan_block_opt(m, f.body.as_deref())?;
                let id = self.add_def(f.name.clone(), DefKind::Fn(f.clone()), m, None, span, attrs);
                self.define(m, Ns::Value, &f.name, id, span)?;
            }
            ItemKind::Struct(s) => {
                let id = self.add_def(s.name.clone(), DefKind::Struct(s.clone()), m, None, span, attrs);
                self.define(m, Ns::Type, &s.name, id, span)?;
                if !matches!(s.data, VariantData::Named(_)) {
                    self.define(m, Ns::Value, &s.name, id, span)?;
                }
            }
            ItemKind::Enum(e) => {
                let id = self.add_def(e.name.clone(), DefKind::Enum(e.clone()), m, None, span, attrs);
                self.define(m, Ns::Type, &e.name, id, span)?;
                for (i, v) in e.variants.iter().enumerate() {
                    let vid = self.add_def(v.name.clone(), DefKind::Variant(i), m, Some(id), v.span, Self::no_attrs());
                    self.defs[id as usize].children.push(vid);
                }
            }
            ItemKind::Trait(t) => {
                let id = self.add_def(t.name.clone(), DefKind::Trait(t.clone()), m, None, span, attrs);
                self.define(m, Ns::Type, &t.name, id, span)?;
                for it in &t.items {
                    let cid = match it {
                        TraitItem::Fn(f, a) => {
                            self.scan_block_opt(m, f.body.as_deref())?;
                            self.add_def(f.name.clone(), DefKind::Fn(f.clone()), m, Some(id), f.span, a.clone())
                        }
                        TraitItem::Type(n, b, d) => self.add_def(n.clone(), DefKind::AssocType(b.clone(), d.clone()), m, Some(id), span, Self::no_attrs()),
                        TraitItem::Const(n, ty, d) => self.add_def(n.clone(), DefKind::AssocConst(ty.clone(), d.clone()), m, Some(id), span, Self::no_attrs()),
                    };
                    self.defs[id as usize].children.push(cid);
                }
            }
            ItemKind::Impl(imp) => {
                let id = self.add_def(Rc::from("<impl>"), DefKind::Impl(imp.clone()), m, None, span, attrs);
                self.impls.push(id);
                for it in &imp.items {
                    let cid = match it {
                        ImplItem::Fn(f, a) => {
                            self.scan_block_opt(m, f.body.as_deref())?;
                            self.add_def(f.name.clone(), DefKind::Fn(f.clone()), m, Some(id), f.span, a.clone())
                        }
                        ImplItem::Type(n, t) => self.add_def(n.clone(), DefKind::ImplType(t.clone()), m, Some(id), span, Self::no_attrs()),
                        ImplItem::Const(n, t, e) => self.add_def(n.clone(), DefKind::AssocConst(t.clone(), Some(e.clone())), m, Some(id), span, Self::no_attrs()),
                    };
                    self.defs[id as usize].children.push(cid);
                }
            }
            ItemKind::Const(n, t, e) => {
                self.scan_expr(m, &e)?;
                let id = self.add_def(n.clone(), DefKind::Const(t, e), m, None, span, attrs);
                self.define(m, Ns::Value, &n, id, span)?;
            }
            ItemKind::Static(n, mutable, t, e) => {
                let id = self.add_def(n.clone(), DefKind::Static(mutable, t, e), m, None, span, attrs);
                self.define(m, Ns::Value, &n, id, span)?;
            }
            ItemKind::TypeAlias(n, g, t) => {
                let id = self.add_def(n.clone(), DefKind::TypeAlias(g, t), m, None, span, attrs);
                self.define(m, Ns::Type, &n, id, span)?;
            }
            ItemKind::Mod(n, items) => {
                let krate = self.mods[m as usize].krate;
                let sub = self.new_mod(n.clone(), Some(m), false, Some(krate));
                if self.mods[m as usize].mods.insert(n.clone(), sub).is_some() {
                    return err(span, format!("the name `{}` is defined multiple times", n));
                }
                let id = self.add_def(n, DefKind::Mod(sub), m, None, span, attrs);
                let _ = id;
                self.items(sub, items)?;
            }
            ItemKind::ModFile(n) => return err(span, format!("module `{}` was not loaded", n)),
            ItemKind::Use(tree) => self.use_tree(m, &tree, &[]),
            ItemKind::Extern(abi, fns) => {
                for f in fns {
                    let id = self.add_def(f.name.clone(), DefKind::ExternFn(f.clone(), abi.clone()), m, None, f.span, attrs.clone());
                    self.define(m, Ns::Value, &f.name, id, f.span)?;
                }
            }
            ItemKind::MacroRules(_) => {}
        }
        Ok(())
    }

    fn use_tree(&mut self, m: ModId, tree: &UseTree, prefix: &[Name]) {
        let module = &mut self.mods[m as usize];
        match tree {
            UseTree::Path(p, alias, span) => {
                let mut full: Vec<Name> = prefix.to_vec();
                full.extend(p.iter().cloned());
                // `use a::b::{self}` imports `b`.
                if full.len() > 1 && &*full[full.len() - 1] == "self" {
                    full.pop();
                }
                // `use Trait as _;` stays with the alias `_` (never looked up
                // by name; it puts the trait's methods in scope).
                let alias = alias.clone().unwrap_or_else(|| full.last().unwrap().clone());
                module.uses.push(UseDecl {
                    path: full,
                    alias,
                    span: *span,
                });
            }
            UseTree::Glob(p, span) => {
                let mut full: Vec<Name> = prefix.to_vec();
                full.extend(p.iter().cloned());
                module.globs.push((full, *span));
            }
            UseTree::Group(p, trees) => {
                let mut full: Vec<Name> = prefix.to_vec();
                full.extend(p.iter().cloned());
                for t in trees {
                    self.use_tree(m, t, &full);
                }
            }
        }
    }

    // Items declared inside function bodies get a module per block.

    fn scan_block_opt(&mut self, m: ModId, b: Option<&Block>) -> Result<()> {
        match b {
            Some(b) => self.scan_block(m, b),
            None => Ok(()),
        }
    }

    fn scan_block(&mut self, m: ModId, b: &Block) -> Result<()> {
        let has_items = b.stmts.iter().any(|s| matches!(s, Stmt::Item(_)));
        let scope = if has_items {
            let krate = self.mods[m as usize].krate;
            let sub = self.new_mod(Rc::from("{block}"), Some(m), true, Some(krate));
            self.block_mods.insert(b as *const Block as usize, sub);
            let items: Vec<Item> = b
                .stmts
                .iter()
                .filter_map(|s| match s {
                    Stmt::Item(i) => Some(i.clone()),
                    _ => None,
                })
                .collect();
            self.items(sub, items)?;
            sub
        } else {
            m
        };
        for s in &b.stmts {
            match s {
                Stmt::Let { init, else_, .. } => {
                    if let Some(e) = init {
                        self.scan_expr(scope, e)?;
                    }
                    if let Some(b) = else_ {
                        self.scan_block(scope, b)?;
                    }
                }
                Stmt::Expr(e, _) => self.scan_expr(scope, e)?,
                _ => {}
            }
        }
        if let Some(e) = &b.expr {
            self.scan_expr(scope, e)?;
        }
        Ok(())
    }

    fn scan_expr(&mut self, m: ModId, e: &Expr) -> Result<()> {
        use ExprKind as E;
        match &e.kind {
            E::Block(b, _) | E::Unsafe(b) | E::Loop(b, _) => self.scan_block(m, b)?,
            E::If(c, b, els) => {
                self.scan_expr(m, c)?;
                self.scan_block(m, b)?;
                if let Some(e) = els {
                    self.scan_expr(m, e)?;
                }
            }
            E::While(c, b, _) => {
                self.scan_expr(m, c)?;
                self.scan_block(m, b)?;
            }
            E::For(_, it, b, _) => {
                self.scan_expr(m, it)?;
                self.scan_block(m, b)?;
            }
            E::Match(s, arms) => {
                self.scan_expr(m, s)?;
                for a in arms {
                    self.scan_expr(m, &a.body)?;
                    if let Some(g) = &a.guard {
                        self.scan_expr(m, g)?;
                    }
                }
            }
            E::Closure { body, .. } => self.scan_expr(m, body)?,
            E::Unary(_, a) | E::Ref(_, a) | E::Cast(a, _) | E::Field(a, _) | E::TupleField(a, _) | E::Try(a) | E::Paren(a) | E::Let(_, a) => self.scan_expr(m, a)?,
            E::Binary(_, a, b) | E::Assign(a, b) | E::AssignOp(_, a, b) | E::Index(a, b) | E::ArrayRepeat(a, b) | E::VecRepeat(a, b) => {
                self.scan_expr(m, a)?;
                self.scan_expr(m, b)?;
            }
            E::Call(f, args) => {
                self.scan_expr(m, f)?;
                for a in args {
                    self.scan_expr(m, a)?;
                }
            }
            E::MethodCall { recv, params, .. } => {
                self.scan_expr(m, recv)?;
                for a in params {
                    self.scan_expr(m, a)?;
                }
            }
            E::Tuple(es) | E::Array(es) | E::VecLit(es) => {
                for a in es {
                    self.scan_expr(m, a)?;
                }
            }
            E::StructLit { fields, base, .. } => {
                for (_, a, _) in fields {
                    self.scan_expr(m, a)?;
                }
                if let Some(b) = base {
                    self.scan_expr(m, b)?;
                }
            }
            E::Return(Some(a)) | E::Break(_, Some(a)) => self.scan_expr(m, a)?,
            E::Range(a, b, _) => {
                if let Some(a) = a {
                    self.scan_expr(m, a)?;
                }
                if let Some(b) = b {
                    self.scan_expr(m, b)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn build(std_items: Vec<Item>, items: Vec<Item>) -> Result<Db> {
    let mut b = Builder {
        defs: Vec::new(),
        mods: Vec::new(),
        impls: Vec::new(),
        block_mods: BTreeMap::new(),
        in_std: true,
    };
    let std_root = b.new_mod(Rc::from("std"), None, false, None);
    b.items(std_root, std_items)?;
    b.in_std = false;
    let crate_root = b.new_mod(Rc::from("crate"), None, false, None);
    b.items(crate_root, items)?;
    let prelude = b.mods[std_root as usize].mods.get("prelude").copied();
    let mut db = Db {
        defs: b.defs,
        mods: b.mods,
        impls: b.impls,
        std_root,
        crate_root,
        prelude,
        block_mods: b.block_mods,
        lang: Lang::default(),
        trait_methods: BTreeMap::new(),
        resolving: RefCell::new(Vec::new()),
    };
    for (name, path) in LANG_PATHS {
        let segs: Vec<&str> = path.split("::").collect();
        let mut cur = std_root;
        let mut found = None;
        for (i, s) in segs.iter().enumerate() {
            match db.lookup_here(cur, s, Ns::Type, 0) {
                Some(Res::Mod(m)) => cur = m,
                Some(Res::Def(d)) if i + 1 == segs.len() => found = Some(d),
                Some(Res::Def(d)) if i + 2 == segs.len() => {
                    found = db.child(d, segs[i + 1]);
                    break;
                }
                _ => break,
            }
        }
        match found {
            Some(d) => {
                db.lang.items.insert(name, d);
            }
            None => {
                return Err(Error::new(Span::default(), format!("the standard library has no `{}`", path)));
            }
        }
    }
    for (i, d) in db.defs.iter().enumerate() {
        if let (DefKind::Fn(f), Some(p)) = (&d.kind, d.parent) {
            if let DefKind::Trait(_) = db.defs[p as usize].kind {
                db.trait_methods.entry(f.name.clone()).or_default().push(i as DefId);
            }
        }
    }
    Ok(db)
}
