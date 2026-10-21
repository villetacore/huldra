//! Recursive-descent parser: tokens to the syntax tree in [`ast`].
//!
//! Built-in macros (`println!`, `vec!`, `assert_eq!`, ...) are parsed
//! here into dedicated nodes; `macro_rules!` macros are expanded by
//! [`crate::macros`] and the expansion parsed in place.

use crate::ast::*;
use crate::lex::{self, Tok, Token};
use crate::macros::{self, Macros};
use crate::{err, Error, FileSource, Result, Sources, Span};
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
    "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
    "super", "trait", "true", "type", "unsafe", "use", "where", "while", "async", "await", "dyn",
];

pub struct Parser<'m> {
    toks: Vec<Token>,
    pos: usize,
    /// Struct literals are not allowed (`if`/`while`/`match` heads).
    no_struct: bool,
    pub macros: &'m mut Macros,
    depth: u32,
    /// Visibility of the fields parsed last.
    field_vis: Vec<bool>,
}

/// Parses a crate root and loads its `mod name;` files.
pub fn parse_crate(fs: &dyn FileSource, sources: &mut Sources, file: u16) -> Result<Vec<Item>> {
    let mut macros = Macros::default();
    parse_file(fs, sources, file, &mut macros, true)
}

fn parse_file(fs: &dyn FileSource, sources: &mut Sources, file: u16, macros: &mut Macros, root: bool) -> Result<Vec<Item>> {
    let toks = lex::tokenize(&sources.texts[file as usize], file)?;
    let mut p = Parser::new(toks, macros);
    let mut items = p.parse_items(true)?;
    let name = sources.names[file as usize].clone();
    // Children of `dir/main.rs` or `dir/foo/mod.rs` live in `dir/`;
    // children of `dir/foo.rs` in `dir/foo/`.
    let dir = match name.rfind('/') {
        Some(i) => &name[..i + 1],
        None => "",
    };
    let base = name.rsplit('/').next().unwrap_or("");
    let child_dir = if root || base == "mod.rs" {
        String::from(dir)
    } else {
        format!("{}{}/", dir, base.trim_end_matches(".rs"))
    };
    load_modules(fs, sources, &mut items, &child_dir, macros)?;
    Ok(items)
}

fn load_modules(fs: &dyn FileSource, sources: &mut Sources, items: &mut [Item], dir: &str, macros: &mut Macros) -> Result<()> {
    for item in items.iter_mut() {
        match &mut item.kind {
            ItemKind::ModFile(name) => {
                let name = name.clone();
                let a = format!("{}{}.rs", dir, name);
                let b = format!("{}{}/mod.rs", dir, name);
                let (path, text) = match fs.read(&a) {
                    Some(t) => (a, t),
                    None => match fs.read(&b) {
                        Some(t) => (b, t),
                        None => {
                            return Err(Error::new(item.span, format!("file not found for module `{}`", name))
                                .note(format!("help: to create the module `{}`, create file \"{}\"", name, a)))
                        }
                    },
                };
                let file = sources.add(path.clone(), text);
                let toks = lex::tokenize(&sources.texts[file as usize], file)?;
                let mut p = Parser::new(toks, macros);
                let mut sub = p.parse_items(true)?;
                let child_dir = if path.ends_with("/mod.rs") {
                    format!("{}{}/", dir, name)
                } else {
                    format!("{}{}/", dir, name)
                };
                load_modules(fs, sources, &mut sub, &child_dir, macros)?;
                item.kind = ItemKind::Mod(name, sub);
            }
            ItemKind::Mod(name, sub) => {
                let child_dir = format!("{}{}/", dir, name);
                load_modules(fs, sources, sub, &child_dir, macros)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

impl<'m> Parser<'m> {
    pub fn new(toks: Vec<Token>, macros: &'m mut Macros) -> Parser<'m> {
        Parser {
            toks,
            pos: 0,
            no_struct: false,
            macros,
            depth: 0,
            field_vis: Vec::new(),
        }
    }

    // ---- token helpers ----

    pub fn position(&self) -> usize {
        self.pos
    }

    fn tok(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek(&self, k: usize) -> &Tok {
        let i = (self.pos + k).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn at_eof(&self) -> bool {
        matches!(self.tok(), Tok::Eof)
    }

    fn is(&self, p: &str) -> bool {
        matches!(self.tok(), Tok::Punct(q) if *q == p)
    }

    fn is_at(&self, k: usize, p: &str) -> bool {
        matches!(self.peek(k), Tok::Punct(q) if *q == p)
    }

    /// Accepts `p`, also as the first part of a longer token (`>` of `>>`).
    fn eat(&mut self, p: &str) -> bool {
        if let Tok::Punct(q) = self.tok() {
            let q = *q;
            if q == p {
                self.bump();
                return true;
            }
            if q.len() > p.len() && q.starts_with(p) && matches!(p, ">" | "<" | "&" | "|" | "=" | "." | "-" | "!") {
                if let Some(rest) = lex::punct(&q[p.len()..]) {
                    let t = &mut self.toks[self.pos];
                    t.tok = Tok::Punct(rest);
                    t.span.col += p.len() as u32;
                    return true;
                }
            }
        }
        false
    }

    /// `is` that also matches the first part of a longer token.
    fn is_split(&self, p: &str) -> bool {
        matches!(self.tok(), Tok::Punct(q) if q.starts_with(p))
    }

    fn expect(&mut self, p: &str) -> Result<()> {
        if self.eat(p) {
            Ok(())
        } else {
            err(self.span(), format!("expected `{}`, found {}", p, self.describe()))
        }
    }

    fn describe(&self) -> String {
        describe(self.tok())
    }

    fn is_kw(&self, k: &str) -> bool {
        matches!(self.tok(), Tok::Ident(s) if &**s == k)
    }

    fn is_kw_at(&self, n: usize, k: &str) -> bool {
        matches!(self.peek(n), Tok::Ident(s) if &**s == k)
    }

    fn eat_kw(&mut self, k: &str) -> bool {
        if self.is_kw(k) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_kw(&mut self, k: &str) -> Result<()> {
        if self.eat_kw(k) {
            Ok(())
        } else {
            err(self.span(), format!("expected `{}`, found {}", k, self.describe()))
        }
    }

    fn is_ident(&self) -> bool {
        matches!(self.tok(), Tok::Ident(s) if !is_keyword(s))
    }

    fn ident(&mut self) -> Result<Name> {
        match self.tok().clone() {
            Tok::Ident(s) if !is_keyword(&s) => {
                self.bump();
                Ok(s)
            }
            _ => err(self.span(), format!("expected identifier, found {}", self.describe())),
        }
    }

    // ---- attributes ----

    /// Outer attributes; `None` when a `#[cfg(...)]` disables the item.
    fn attrs(&mut self) -> Result<Option<Attrs>> {
        let mut attrs = Attrs {
            derives: Vec::new(),
            names: Vec::new(),
        };
        let mut enabled = true;
        while self.is("#") && !self.is_at(1, "!") {
            self.bump();
            if !self.attr_body(&mut attrs)? {
                enabled = false;
            }
        }
        Ok(if enabled { Some(attrs) } else { None })
    }

    /// `#![...]` attributes at the start of a file or module.
    fn inner_attrs(&mut self) -> Result<()> {
        while self.is("#") && self.is_at(1, "!") && self.is_at(2, "[") {
            self.bump();
            self.bump();
            let mut a = Attrs {
                derives: Vec::new(),
                names: Vec::new(),
            };
            self.attr_body(&mut a)?;
        }
        Ok(())
    }

    /// `[path ...]`; returns false for a `cfg` that is off.
    fn attr_body(&mut self, attrs: &mut Attrs) -> Result<bool> {
        self.expect("[")?;
        let mut name = String::from(&*self.ident_or_kw()?);
        while self.is("::") {
            self.bump();
            name.push_str("::");
            name.push_str(&self.ident_or_kw()?);
        }
        let mut enabled = true;
        if self.is("(") {
            let inner = self.delimited()?;
            match name.as_str() {
                "derive" => {
                    // The last segment of each path (`std::fmt::Debug` is `Debug`).
                    for (k, t) in inner.iter().enumerate() {
                        if let Tok::Ident(s) = &t.tok {
                            if !matches!(inner.get(k + 1).map(|t| &t.tok), Some(Tok::Punct("::"))) {
                                attrs.derives.push(s.clone());
                            }
                        }
                    }
                }
                "cfg" => {
                    let mut i = 0;
                    enabled = cfg_eval(&inner, &mut i);
                }
                _ => {}
            }
        } else if self.eat("=") {
            self.bump();
        }
        self.expect("]")?;
        attrs.names.push(Rc::from(name.as_str()));
        Ok(enabled)
    }

    fn ident_or_kw(&mut self) -> Result<Name> {
        match self.tok().clone() {
            Tok::Ident(s) => {
                self.bump();
                Ok(s)
            }
            _ => err(self.span(), format!("expected identifier, found {}", self.describe())),
        }
    }

    /// The tokens inside a `(...)`, `[...]` or `{...}` group (with a final Eof).
    pub fn delimited(&mut self) -> Result<Vec<Token>> {
        let open = self.span();
        let close = match self.tok() {
            Tok::Punct("(") => ")",
            Tok::Punct("[") => "]",
            Tok::Punct("{") => "}",
            _ => return err(open, format!("expected one of `(`, `[`, or `{{`, found {}", self.describe())),
        };
        self.bump();
        let mut out = Vec::new();
        let mut stack = vec![close];
        loop {
            let t = self.tok().clone();
            match t {
                Tok::Eof => return err(open, String::from("this file contains an unclosed delimiter")),
                Tok::Punct(p @ ("(" | "[" | "{")) => stack.push(match p {
                    "(" => ")",
                    "[" => "]",
                    _ => "}",
                }),
                Tok::Punct(p @ (")" | "]" | "}")) => {
                    if stack.last() != Some(&p) {
                        return err(self.span(), format!("mismatched closing delimiter: `{}`", p));
                    }
                    stack.pop();
                    if stack.is_empty() {
                        let span = self.span();
                        self.bump();
                        out.push(Token {
                            tok: Tok::Eof,
                            span,
                            joint: false,
                        });
                        return Ok(out);
                    }
                }
                _ => {}
            }
            out.push(self.bump());
        }
    }

    fn skip_vis(&mut self) -> Result<()> {
        if self.eat_kw("pub") {
            if self.is("(") {
                // pub(crate), pub(super), pub(in path)
                if self.is_kw_at(1, "crate") || self.is_kw_at(1, "super") || self.is_kw_at(1, "self") || self.is_kw_at(1, "in") {
                    self.delimited()?;
                }
            }
        } else if self.is_kw("crate") && !self.is_at(1, "::") {
            self.bump();
        }
        Ok(())
    }

    // ---- items ----

    pub fn parse_items(&mut self, top: bool) -> Result<Vec<Item>> {
        if top {
            self.inner_attrs()?;
        }
        let mut items = Vec::new();
        loop {
            if self.at_eof() || (!top && self.is("}")) {
                return Ok(items);
            }
            self.item_into(&mut items)?;
        }
    }

    /// Parses one item (or a macro invocation expanding to items).
    fn item_into(&mut self, items: &mut Vec<Item>) -> Result<()> {
        let attrs = self.attrs()?;
        if self.is_ident() && self.is_at(1, "!") && !self.is_kw("macro_rules") {
            let span = self.span();
            let name = self.ident()?;
            self.bump();
            let toks = self.delimited()?;
            self.eat(";");
            let expanded = self.expand_user(&name, toks, span)?;
            let mut sub = Parser::new(expanded, self.macros);
            sub.depth = self.depth + 1;
            let mut new = sub.parse_items(true)?;
            if attrs.is_some() {
                items.append(&mut new);
            }
            return Ok(());
        }
        let item = self.item(attrs.clone().unwrap_or(Attrs {
            derives: Vec::new(),
            names: Vec::new(),
        }))?;
        if let (Some(item), Some(_)) = (item, attrs) {
            items.push(item);
        }
        Ok(())
    }

    fn expand_user(&mut self, name: &Name, toks: Vec<Token>, span: Span) -> Result<Vec<Token>> {
        if self.depth > 64 {
            return err(span, format!("recursion limit reached while expanding `{}!`", name));
        }
        match self.macros.get(name) {
            Some(def) => macros::expand(&def, toks, span),
            None => err(span, format!("cannot find macro `{}` in this scope", name)),
        }
    }

    /// One item; `None` for items that produce nothing (`extern crate`).
    fn item(&mut self, attrs: Attrs) -> Result<Option<Item>> {
        let span = self.span();
        if self.is_kw("macro_rules") && self.is_at(1, "!") {
            self.bump();
            self.bump();
            let name = self.ident()?;
            let body = self.delimited()?;
            self.eat(";");
            let def = Rc::new(macros::parse_rules(name.clone(), body, span)?);
            self.macros.define(def.clone());
            return Ok(Some(Item {
                kind: ItemKind::MacroRules(def),
                attrs,
                span,
            }));
        }
        self.skip_vis()?;
        let kind = if self.is_kw("use") {
            self.bump();
            let tree = self.use_tree(Vec::new())?;
            self.expect(";")?;
            ItemKind::Use(tree)
        } else if self.is_kw("struct") || (self.is_kw("union") && matches!(self.peek(1), Tok::Ident(_))) {
            if self.is_kw("union") {
                return err(span, String::from("unions are not supported"));
            }
            self.bump();
            ItemKind::Struct(Rc::new(self.struct_def()?))
        } else if self.is_kw("enum") {
            self.bump();
            ItemKind::Enum(Rc::new(self.enum_def()?))
        } else if self.is_kw("impl") || (self.is_kw("unsafe") && self.is_kw_at(1, "impl")) {
            self.eat_kw("unsafe");
            self.bump();
            ItemKind::Impl(Rc::new(self.impl_def(span)?))
        } else if self.is_kw("trait") || (self.is_kw("unsafe") && self.is_kw_at(1, "trait")) || (self.is_kw("auto") && self.is_kw_at(1, "trait")) {
            self.eat_kw("unsafe");
            self.eat_kw("auto");
            self.bump();
            ItemKind::Trait(Rc::new(self.trait_def()?))
        } else if self.is_kw("mod") {
            self.bump();
            let name = self.ident()?;
            if self.eat(";") {
                ItemKind::ModFile(name)
            } else {
                self.expect("{")?;
                self.inner_attrs()?;
                let items = self.parse_items(false)?;
                self.expect("}")?;
                ItemKind::Mod(name, items)
            }
        } else if self.is_kw("type") {
            self.bump();
            let name = self.ident()?;
            let mut generics = self.generics()?;
            self.where_clause(&mut generics)?;
            self.expect("=")?;
            let ty = self.ty()?;
            self.expect(";")?;
            ItemKind::TypeAlias(name, generics, ty)
        } else if self.is_kw("static") {
            self.bump();
            let mutable = self.eat_kw("mut");
            let name = self.ident()?;
            self.expect(":")?;
            let ty = self.ty()?;
            self.expect("=")?;
            let e = self.expr()?;
            self.expect(";")?;
            ItemKind::Static(name, mutable, ty, Rc::new(e))
        } else if self.is_kw("const") && !self.is_kw_at(1, "fn") && !self.is_kw_at(1, "unsafe") && !self.is_kw_at(1, "extern") {
            self.bump();
            let name = if self.eat_kw("_") { Rc::from("_") } else { self.ident()? };
            self.expect(":")?;
            let ty = self.ty()?;
            self.expect("=")?;
            let e = self.expr()?;
            self.expect(";")?;
            ItemKind::Const(name, ty, Rc::new(e))
        } else if self.is_kw("extern") && self.is_kw_at(1, "crate") {
            while !self.eat(";") {
                if self.at_eof() {
                    return err(span, String::from("expected `;`"));
                }
                self.bump();
            }
            return Ok(None);
        } else if self.is_kw("extern") && (self.is_at(1, "{") || (matches!(self.peek(1), Tok::Str(_)) && self.is_at(2, "{"))) {
            self.bump();
            let abi: Name = match self.tok().clone() {
                Tok::Str(s) => {
                    self.bump();
                    s
                }
                _ => Rc::from("C"),
            };
            self.expect("{")?;
            let mut fns = Vec::new();
            while !self.eat("}") {
                self.attrs()?;
                self.skip_vis()?;
                self.eat_kw("unsafe");
                self.eat_kw("safe");
                self.expect_kw("fn")?;
                fns.push(Rc::new(self.fn_decl()?));
            }
            ItemKind::Extern(abi, fns)
        } else if self.fn_start() {
            self.fn_quals()?;
            ItemKind::Fn(Rc::new(self.fn_decl()?))
        } else if self.is_kw("async") {
            return err(span, String::from("async functions are not supported"));
        } else {
            return err(span, format!("expected item, found {}", self.describe()));
        };
        Ok(Some(Item { kind, attrs, span }))
    }

    fn fn_start(&self) -> bool {
        let mut k = 0;
        loop {
            match self.peek(k) {
                Tok::Ident(s) if matches!(&**s, "const" | "unsafe" | "default") => k += 1,
                Tok::Ident(s) if &**s == "extern" => {
                    k += 1;
                    if matches!(self.peek(k), Tok::Str(_)) {
                        k += 1;
                    }
                }
                Tok::Ident(s) if &**s == "fn" => return true,
                _ => return false,
            }
        }
    }

    fn fn_quals(&mut self) -> Result<()> {
        loop {
            if self.eat_kw("const") || self.eat_kw("unsafe") || self.eat_kw("default") {
                continue;
            }
            if self.eat_kw("extern") {
                if let Tok::Str(_) = self.tok() {
                    self.bump();
                }
                continue;
            }
            return self.expect_kw("fn");
        }
    }

    fn use_tree(&mut self, mut prefix: Vec<Name>) -> Result<UseTree> {
        let span = self.span();
        if self.eat("::") && prefix.is_empty() {
            // `use ::std::...`
        }
        loop {
            if self.eat("*") {
                return Ok(UseTree::Glob(prefix, span));
            }
            if self.is("{") {
                self.bump();
                let mut trees = Vec::new();
                while !self.eat("}") {
                    trees.push(self.use_tree(Vec::new())?);
                    if !self.eat(",") {
                        self.expect("}")?;
                        break;
                    }
                }
                return Ok(UseTree::Group(prefix, trees));
            }
            let name = self.ident_or_kw()?;
            prefix.push(name);
            if self.eat("::") {
                continue;
            }
            let alias = if self.eat_kw("as") {
                if matches!(self.tok(), Tok::Ident(s) if &**s == "_") {
                    self.bump();
                    Some(Rc::from("_"))
                } else {
                    Some(self.ident()?)
                }
            } else {
                None
            };
            return Ok(UseTree::Path(prefix, alias, span));
        }
    }

    fn generics(&mut self) -> Result<Generics> {
        let mut g = Generics::default();
        if !self.is("<") {
            return Ok(g);
        }
        self.bump();
        loop {
            if self.eat(">") {
                break;
            }
            let span = self.span();
            if let Tok::Lifetime(_) = self.tok() {
                self.bump();
                if self.eat(":") {
                    while let Tok::Lifetime(_) = self.tok() {
                        self.bump();
                        if !self.eat("+") {
                            break;
                        }
                    }
                }
            } else if self.eat_kw("const") {
                let name = self.ident()?;
                self.expect(":")?;
                let ty = self.ty()?;
                let default = if self.eat("=") { Some(self.ty()?) } else { None };
                g.params.push(GenericParam {
                    name,
                    bounds: Vec::new(),
                    default,
                    const_ty: Some(ty),
                    span,
                });
            } else {
                let name = self.ident()?;
                let bounds = if self.eat(":") { self.bounds()? } else { Vec::new() };
                let default = if self.eat("=") { Some(self.ty()?) } else { None };
                g.params.push(GenericParam {
                    name,
                    bounds,
                    default,
                    const_ty: None,
                    span,
                });
            }
            if !self.eat(",") {
                self.expect(">")?;
                break;
            }
        }
        Ok(g)
    }

    fn where_clause(&mut self, g: &mut Generics) -> Result<()> {
        if !self.eat_kw("where") {
            return Ok(());
        }
        loop {
            if self.is("{") || self.is(";") || self.at_eof() || self.is("=") {
                return Ok(());
            }
            if let Tok::Lifetime(_) = self.tok() {
                self.bump();
                self.expect(":")?;
                while let Tok::Lifetime(_) = self.tok() {
                    self.bump();
                    if !self.eat("+") {
                        break;
                    }
                }
            } else {
                self.skip_for_lifetimes()?;
                let ty = self.ty()?;
                self.expect(":")?;
                let b = self.bounds()?;
                g.where_.push((ty, b));
            }
            if !self.eat(",") {
                return Ok(());
            }
        }
    }

    fn skip_for_lifetimes(&mut self) -> Result<()> {
        if self.is_kw("for") && self.is_at(1, "<") {
            self.bump();
            self.bump();
            while !self.eat(">") {
                if self.at_eof() {
                    return err(self.span(), String::from("expected `>`"));
                }
                self.bump();
            }
        }
        Ok(())
    }

    /// `A + B<T> + 'a + ?Sized`
    fn bounds(&mut self) -> Result<Vec<Bound>> {
        let mut out = Vec::new();
        loop {
            match self.tok() {
                Tok::Lifetime(_) => {
                    self.bump();
                }
                Tok::Punct("?") => {
                    self.bump();
                    self.path(PathMode::Type)?;
                }
                Tok::Punct("(") => {
                    self.bump();
                    self.skip_for_lifetimes()?;
                    out.push(self.path(PathMode::Type)?);
                    self.expect(")")?;
                }
                Tok::Punct("~") => {
                    self.bump();
                    self.eat_kw("const");
                    out.push(self.path(PathMode::Type)?);
                }
                Tok::Ident(_) | Tok::Punct("::") | Tok::Punct("<") => {
                    self.skip_for_lifetimes()?;
                    out.push(self.path(PathMode::Type)?);
                }
                _ => break,
            }
            if !self.eat("+") {
                break;
            }
        }
        Ok(out)
    }

    fn fields_named(&mut self) -> Result<Vec<(Name, Type)>> {
        self.expect("{")?;
        let mut fields = Vec::new();
        self.field_vis.clear();
        while !self.eat("}") {
            let attrs = self.attrs()?;
            let public = self.is_kw("pub");
            self.skip_vis()?;
            let name = self.ident()?;
            self.expect(":")?;
            let ty = self.ty()?;
            if attrs.is_some() {
                fields.push((name, ty));
                self.field_vis.push(public);
            }
            if !self.eat(",") {
                self.expect("}")?;
                break;
            }
        }
        Ok(fields)
    }

    fn fields_tuple(&mut self) -> Result<Vec<Type>> {
        self.expect("(")?;
        let mut fields = Vec::new();
        self.field_vis.clear();
        while !self.eat(")") {
            self.attrs()?;
            self.field_vis.push(self.is_kw("pub"));
            self.skip_vis()?;
            fields.push(self.ty()?);
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        Ok(fields)
    }

    fn struct_def(&mut self) -> Result<StructDef> {
        let name = self.ident()?;
        let mut generics = self.generics()?;
        self.where_clause(&mut generics)?;
        self.field_vis.clear();
        let data = if self.eat(";") {
            VariantData::Unit
        } else if self.is("(") {
            let f = self.fields_tuple()?;
            self.where_clause(&mut generics)?;
            self.expect(";")?;
            VariantData::Tuple(f)
        } else {
            VariantData::Named(self.fields_named()?)
        };
        let public = core::mem::take(&mut self.field_vis);
        Ok(StructDef { name, generics, data, public })
    }

    fn enum_def(&mut self) -> Result<EnumDef> {
        let name = self.ident()?;
        let mut generics = self.generics()?;
        self.where_clause(&mut generics)?;
        self.expect("{")?;
        let mut variants = Vec::new();
        while !self.eat("}") {
            let attrs = self.attrs()?;
            self.skip_vis()?;
            let span = self.span();
            let vname = self.ident()?;
            let data = if self.is("(") {
                VariantData::Tuple(self.fields_tuple()?)
            } else if self.is("{") {
                VariantData::Named(self.fields_named()?)
            } else {
                VariantData::Unit
            };
            let disc = if self.eat("=") { Some(self.expr()?) } else { None };
            if let Some(a) = attrs {
                variants.push(Variant {
                    name: vname,
                    data,
                    disc,
                    default: a.has("default"),
                    span,
                });
            }
            if !self.eat(",") {
                self.expect("}")?;
                break;
            }
        }
        Ok(EnumDef { name, generics, variants })
    }

    fn impl_def(&mut self, span: Span) -> Result<ImplDef> {
        // `impl<T>` vs `impl <T as X>::Y` (the latter is not supported).
        let mut generics = if self.is("<") { self.generics()? } else { Generics::default() };
        self.eat_kw("const");
        let negative = self.eat("!");
        let first = self.ty_no_bounds()?;
        let (trait_, self_ty) = if self.eat_kw("for") {
            let path = match first.kind {
                TypeKind::Path(p) => p,
                _ => return err(first.span, String::from("expected a trait, found type")),
            };
            (Some(path), self.ty()?)
        } else {
            (None, first)
        };
        self.where_clause(&mut generics)?;
        self.expect("{")?;
        self.inner_attrs()?;
        let mut items = Vec::new();
        while !self.eat("}") {
            let attrs = self.attrs()?;
            if self.is_ident() && self.is_at(1, "!") {
                // A macro in an impl block: expand to impl items.
                let mspan = self.span();
                let name = self.ident()?;
                self.bump();
                let toks = self.delimited()?;
                self.eat(";");
                let mut expanded = self.expand_user(&name, toks, mspan)?;
                // Parse the expansion as impl items by wrapping it.
                let end = expanded.pop().unwrap();
                let mut wrapped = vec![
                    Token { tok: Tok::Ident(Rc::from("impl")), span: mspan, joint: false },
                    Token { tok: Tok::Ident(Rc::from("X")), span: mspan, joint: false },
                    Token { tok: Tok::Punct("{"), span: mspan, joint: false },
                ];
                wrapped.extend(expanded);
                wrapped.push(Token { tok: Tok::Punct("}"), span: mspan, joint: false });
                wrapped.push(end);
                let mut sub = Parser::new(wrapped, self.macros);
                sub.depth = self.depth + 1;
                sub.bump();
                let inner = sub.impl_def(mspan)?;
                items.extend(inner.items);
                continue;
            }
            self.skip_vis()?;
            let ispan = self.span();
            if self.is_kw("type") {
                self.bump();
                let name = self.ident()?;
                let mut g = self.generics()?;
                self.where_clause(&mut g)?;
                self.expect("=")?;
                let ty = self.ty()?;
                self.expect(";")?;
                if attrs.is_some() {
                    items.push(ImplItem::Type(name, ty));
                }
            } else if self.is_kw("const") && !self.is_kw_at(1, "fn") && !self.is_kw_at(1, "unsafe") {
                self.bump();
                let name = self.ident()?;
                self.expect(":")?;
                let ty = self.ty()?;
                self.expect("=")?;
                let e = self.expr()?;
                self.expect(";")?;
                if attrs.is_some() {
                    items.push(ImplItem::Const(name, ty, Rc::new(e)));
                }
            } else if self.fn_start() {
                self.fn_quals()?;
                let f = self.fn_decl()?;
                if let Some(a) = attrs {
                    items.push(ImplItem::Fn(Rc::new(f), a));
                }
            } else {
                return err(ispan, format!("expected item in impl, found {}", self.describe()));
            }
        }
        Ok(ImplDef {
            generics,
            trait_,
            negative,
            self_ty,
            items,
            span,
        })
    }

    fn trait_def(&mut self) -> Result<TraitDef> {
        let name = self.ident()?;
        let mut generics = self.generics()?;
        let supertraits = if self.eat(":") { self.bounds()? } else { Vec::new() };
        self.where_clause(&mut generics)?;
        self.expect("{")?;
        let mut items = Vec::new();
        while !self.eat("}") {
            let attrs = self.attrs()?;
            let ispan = self.span();
            if self.is_kw("type") {
                self.bump();
                let n = self.ident()?;
                let mut g = self.generics()?;
                let b = if self.eat(":") { self.bounds()? } else { Vec::new() };
                self.where_clause(&mut g)?;
                let d = if self.eat("=") { Some(self.ty()?) } else { None };
                self.expect(";")?;
                items.push(TraitItem::Type(n, b, d));
            } else if self.is_kw("const") && !self.is_kw_at(1, "fn") {
                self.bump();
                let n = self.ident()?;
                self.expect(":")?;
                let ty = self.ty()?;
                let d = if self.eat("=") { Some(Rc::new(self.expr()?)) } else { None };
                self.expect(";")?;
                items.push(TraitItem::Const(n, ty, d));
            } else if self.fn_start() {
                self.fn_quals()?;
                let f = self.fn_decl()?;
                if let Some(a) = attrs {
                    items.push(TraitItem::Fn(Rc::new(f), a));
                }
            } else {
                return err(ispan, format!("expected item in trait, found {}", self.describe()));
            }
        }
        Ok(TraitDef {
            name,
            generics,
            supertraits,
            items,
        })
    }

    /// After `fn`.
    fn fn_decl(&mut self) -> Result<FnDecl> {
        let span = self.prev_span();
        let name = self.ident()?;
        let mut generics = self.generics()?;
        self.expect("(")?;
        let mut self_param = None;
        let mut self_ty = None;
        let mut params = Vec::new();
        let mut variadic = false;
        // self forms
        let save = self.pos;
        let mut is_self = false;
        if self.is("&") {
            let mut k = 1;
            if let Tok::Lifetime(_) = self.peek(k) {
                k += 1;
            }
            let m = self.is_kw_at(k, "mut");
            if m {
                k += 1;
            }
            if self.is_kw_at(k, "self") && !self.is_at(k + 1, "::") {
                for _ in 0..=k {
                    self.bump();
                }
                self_param = Some((if m { SelfKind::RefMut } else { SelfKind::Ref }, false));
                is_self = true;
            }
        } else if self.is_kw("self") || (self.is_kw("mut") && self.is_kw_at(1, "self")) {
            let m = self.eat_kw("mut");
            self.bump();
            if self.eat(":") {
                let t = self.ty()?;
                let kind = match &t.kind {
                    TypeKind::Ref(true, inner) if is_self_ty(inner) => SelfKind::RefMut,
                    TypeKind::Ref(false, inner) if is_self_ty(inner) => SelfKind::Ref,
                    _ if is_self_ty(&t) => SelfKind::Value,
                    _ => {
                        self_ty = Some(t);
                        SelfKind::Value
                    }
                };
                self_param = Some((kind, m));
            } else {
                self_param = Some((SelfKind::Value, m));
            }
            is_self = true;
        }
        if !is_self {
            self.pos = save;
        } else if !self.eat(",") {
            self.expect(")")?;
            return self.fn_rest(name, generics, self_param, self_ty, params, false, span);
        }
        while !self.eat(")") {
            self.attrs()?;
            if self.eat("...") {
                variadic = true;
                continue;
            }
            let pat = self.pat_single()?;
            self.expect(":")?;
            let ty = self.ty()?;
            params.push(Param { pat, ty });
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        let _ = &mut generics;
        self.fn_rest(name, generics, self_param, self_ty, params, variadic, span)
    }

    #[allow(clippy::too_many_arguments)]
    fn fn_rest(
        &mut self,
        name: Name,
        mut generics: Generics,
        self_param: Option<(SelfKind, bool)>,
        self_ty: Option<Type>,
        params: Vec<Param>,
        variadic: bool,
        span: Span,
    ) -> Result<FnDecl> {
        let ret = if self.eat("->") { Some(self.ty_no_bounds_ret()?) } else { None };
        self.where_clause(&mut generics)?;
        let body = if self.eat(";") { None } else { Some(Rc::new(self.block()?)) };
        Ok(FnDecl {
            name,
            generics,
            self_param,
            self_ty,
            params,
            ret,
            body,
            variadic,
            span,
        })
    }

    // ---- types ----

    pub fn ty(&mut self) -> Result<Type> {
        let span = self.span();
        if self.is_kw("impl") {
            self.bump();
            return Ok(Type {
                kind: TypeKind::ImplTrait(self.bounds()?),
                span,
            });
        }
        if self.is_kw("dyn") {
            self.bump();
            return Ok(Type {
                kind: TypeKind::Dyn(self.bounds()?),
                span,
            });
        }
        let t = self.ty_no_bounds()?;
        // `Box<Trait + Send>` (bare trait objects)
        if self.is("+") {
            if let TypeKind::Path(p) = &t.kind {
                let mut b = vec![p.clone()];
                while self.eat("+") {
                    b.extend(self.bounds()?);
                }
                return Ok(Type {
                    kind: TypeKind::Dyn(b),
                    span,
                });
            }
        }
        Ok(t)
    }

    /// A return type: `-> impl Fn(i32) -> i32` keeps the inner arrow.
    fn ty_no_bounds_ret(&mut self) -> Result<Type> {
        self.ty()
    }

    fn ty_no_bounds(&mut self) -> Result<Type> {
        let span = self.span();
        let kind = match self.tok().clone() {
            Tok::Punct("(") => {
                self.bump();
                let mut elems = Vec::new();
                let mut trailing = false;
                while !self.eat(")") {
                    elems.push(self.ty()?);
                    trailing = self.eat(",");
                    if !trailing {
                        self.expect(")")?;
                        break;
                    }
                }
                if elems.len() == 1 && !trailing {
                    return Ok(elems.pop().unwrap());
                }
                TypeKind::Tuple(elems)
            }
            Tok::Punct("!") => {
                self.bump();
                TypeKind::Never
            }
            Tok::Punct("[") => {
                self.bump();
                let elem = self.ty()?;
                if self.eat(";") {
                    let n = self.expr()?;
                    self.expect("]")?;
                    TypeKind::Array(Box::new(elem), Box::new(n))
                } else {
                    self.expect("]")?;
                    TypeKind::Slice(Box::new(elem))
                }
            }
            Tok::Punct(p) if p.starts_with('&') => {
                self.eat("&");
                if let Tok::Lifetime(_) = self.tok() {
                    self.bump();
                }
                let m = self.eat_kw("mut");
                let inner = self.ty_no_bounds_or_dyn()?;
                TypeKind::Ref(m, Box::new(inner))
            }
            Tok::Punct("*") => {
                self.bump();
                let m = if self.eat_kw("mut") {
                    true
                } else {
                    self.expect_kw("const")?;
                    false
                };
                let inner = self.ty_no_bounds_or_dyn()?;
                TypeKind::Ptr(m, Box::new(inner))
            }
            Tok::Ident(s) if &*s == "_" => {
                self.bump();
                TypeKind::Infer
            }
            Tok::Ident(s) if &*s == "dyn" || &*s == "impl" => {
                // `&dyn A + B` is ambiguous in Rust; accept one bound.
                self.bump();
                let b = vec![self.path(PathMode::Type)?];
                if &*s == "dyn" {
                    TypeKind::Dyn(b)
                } else {
                    TypeKind::ImplTrait(b)
                }
            }
            Tok::Ident(s) if &*s == "fn" || &*s == "unsafe" || &*s == "extern" || &*s == "for" => {
                self.skip_for_lifetimes()?;
                self.eat_kw("unsafe");
                if self.eat_kw("extern") {
                    if let Tok::Str(_) = self.tok() {
                        self.bump();
                    }
                }
                self.expect_kw("fn")?;
                self.expect("(")?;
                let mut params = Vec::new();
                while !self.eat(")") {
                    // optional `name:`
                    if self.is_ident() && self.is_at(1, ":") && !self.is_at(1, "::") {
                        self.bump();
                        self.bump();
                    }
                    params.push(self.ty()?);
                    if !self.eat(",") {
                        self.expect(")")?;
                        break;
                    }
                }
                let ret = if self.eat("->") {
                    self.ty_no_bounds()?
                } else {
                    Type {
                        kind: TypeKind::Tuple(Vec::new()),
                        span,
                    }
                };
                TypeKind::Fn(params, Box::new(ret))
            }
            Tok::Ident(_) | Tok::Punct("::") | Tok::Punct("<") | Tok::Punct("<<") => TypeKind::Path(self.path(PathMode::Type)?),
            _ => return err(span, format!("expected type, found {}", self.describe())),
        };
        Ok(Type { kind, span })
    }

    fn ty_no_bounds_or_dyn(&mut self) -> Result<Type> {
        if self.is_kw("dyn") {
            let span = self.span();
            self.bump();
            let mut b = vec![self.path(PathMode::Type)?];
            // `&(dyn A + B)` uses parens; `&dyn A + B` is an error in Rust.
            while self.is("+") && !matches!(self.peek(1), Tok::Punct(_)) {
                self.bump();
                b.extend(self.bounds()?);
            }
            return Ok(Type {
                kind: TypeKind::Dyn(b),
                span,
            });
        }
        self.ty_no_bounds()
    }

    // ---- paths ----

    pub fn path(&mut self, mode: PathMode) -> Result<Path> {
        let span = self.span();
        let mut qself = None;
        let mut global = false;
        if self.is_split("<") && !self.is("<=") && !self.is("<<=") {
            // `<T>::x` or `<T as Trait>::x`
            self.eat("<");
            let ty = self.ty()?;
            let trait_ = if self.eat_kw("as") { Some(self.path(PathMode::Type)?) } else { None };
            self.expect(">")?;
            self.expect("::")?;
            qself = Some(Box::new(QSelf { ty, trait_ }));
        } else if self.eat("::") {
            global = true;
        }
        let mut segs = Vec::new();
        loop {
            let name = match self.tok().clone() {
                Tok::Ident(s) if !is_keyword(&s) || matches!(&*s, "self" | "Self" | "super" | "crate") => {
                    self.bump();
                    s
                }
                _ => return err(self.span(), format!("expected identifier, found {}", self.describe())),
            };
            let mut args = None;
            match mode {
                PathMode::Type => {
                    if self.is_split("<") && !self.is("<=") && !self.is("<<=") {
                        args = Some(self.generic_args()?);
                    } else if self.is("::") && self.is_split_at(1, "<") {
                        self.bump();
                        args = Some(self.generic_args()?);
                    } else if self.is("(") && matches!(&*name, "Fn" | "FnMut" | "FnOnce") {
                        args = Some(self.paren_args()?);
                    }
                }
                PathMode::Expr => {
                    if self.is("::") && self.is_split_at(1, "<") && !self.is_at(1, "<=") {
                        self.bump();
                        args = Some(self.generic_args()?);
                    }
                }
            }
            segs.push(PathSeg { name, args });
            if self.is("::") && matches!(self.peek(1), Tok::Ident(_)) {
                self.bump();
                continue;
            }
            break;
        }
        Ok(Path {
            segs,
            global,
            qself,
            span,
        })
    }

    fn is_split_at(&self, k: usize, p: &str) -> bool {
        matches!(self.peek(k), Tok::Punct(q) if q.starts_with(p))
    }

    fn generic_args(&mut self) -> Result<GenericArgs> {
        self.expect("<")?;
        let mut a = GenericArgs::default();
        loop {
            if self.eat(">") {
                break;
            }
            match self.tok().clone() {
                Tok::Lifetime(_) => {
                    self.bump();
                }
                Tok::Ident(n) if self.is_at(1, "=") && !is_keyword(&n) => {
                    self.bump();
                    self.bump();
                    a.bindings.push((n, self.ty()?));
                }
                Tok::Ident(n) if self.is_at(1, ":") && !is_keyword(&n) => {
                    // `Item: Bound` (associated type bounds) – ignore the bound.
                    self.bump();
                    self.bump();
                    self.bounds()?;
                }
                Tok::Int(..) | Tok::Punct("{") | Tok::Punct("-") => {
                    let span = self.span();
                    let e = if self.is("{") {
                        let b = self.block()?;
                        Expr {
                            kind: ExprKind::Block(Rc::new(b), None),
                            span,
                        }
                    } else {
                        self.unary()?
                    };
                    a.types.push(Type {
                        kind: TypeKind::Array(
                            Box::new(Type {
                                kind: TypeKind::Never,
                                span,
                            }),
                            Box::new(e),
                        ),
                        span,
                    });
                }
                _ => a.types.push(self.ty()?),
            }
            if !self.eat(",") {
                self.expect(">")?;
                break;
            }
        }
        Ok(a)
    }

    /// `Fn(A, B) -> C`
    fn paren_args(&mut self) -> Result<GenericArgs> {
        let span = self.span();
        self.expect("(")?;
        let mut params = Vec::new();
        while !self.eat(")") {
            params.push(self.ty()?);
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        let out = if self.eat("->") {
            self.ty_no_bounds()?
        } else {
            Type {
                kind: TypeKind::Tuple(Vec::new()),
                span,
            }
        };
        Ok(GenericArgs {
            types: vec![Type {
                kind: TypeKind::Tuple(params),
                span,
            }],
            bindings: vec![(Rc::from("Output"), out)],
            paren: true,
        })
    }

    // ---- blocks and statements ----

    pub fn block(&mut self) -> Result<Block> {
        let span = self.span();
        self.expect("{")?;
        let save = self.no_struct;
        self.no_struct = false;
        self.inner_attrs()?;
        let mut stmts = Vec::new();
        while !self.is("}") {
            if self.at_eof() {
                return err(span, String::from("this file contains an unclosed delimiter"));
            }
            self.stmt_into(&mut stmts)?;
        }
        self.bump();
        self.no_struct = save;
        Ok(finish_block(stmts, span))
    }

    /// Parses statements up to the end of the token stream (macro expansions).
    fn stmts_to_end(&mut self) -> Result<Vec<Stmt>> {
        let mut stmts = Vec::new();
        while !self.at_eof() {
            self.stmt_into(&mut stmts)?;
        }
        Ok(stmts)
    }

    fn stmt_into(&mut self, out: &mut Vec<Stmt>) -> Result<()> {
        let span = self.span();
        if self.eat(";") {
            return Ok(());
        }
        // Attributes on statements: `#[allow(...)] let x = ...;`
        let attrs = if self.is("#") { self.attrs()? } else { Some(Attrs { derives: Vec::new(), names: Vec::new() }) };
        if self.is_kw("let") {
            self.bump();
            let pat = self.pat_top()?;
            let ty = if self.eat(":") { Some(self.ty()?) } else { None };
            let mut init = None;
            let mut else_ = None;
            if self.eat("=") {
                init = Some(self.expr()?);
                if self.eat_kw("else") {
                    else_ = Some(Rc::new(self.block()?));
                }
            }
            self.expect(";")?;
            if attrs.is_some() {
                out.push(Stmt::Let {
                    pat,
                    ty,
                    init,
                    else_,
                    span,
                });
            }
            return Ok(());
        }
        if self.item_start() {
            let mut items = Vec::new();
            // Re-parse attributes as part of the item.
            let item = self.item(attrs.clone().unwrap_or(Attrs { derives: Vec::new(), names: Vec::new() }))?;
            if let (Some(i), Some(_)) = (item, attrs) {
                items.push(i);
            }
            out.extend(items.into_iter().map(Stmt::Item));
            return Ok(());
        }
        // A user macro in statement position may expand to statements.
        if self.is_ident() && self.is_at(1, "!") && !self.is_builtin_macro() {
            if let Tok::Ident(name) = self.tok().clone() {
                if self.macros.get(&name).is_some() {
                    self.bump();
                    self.bump();
                    let brace = self.is("{");
                    let toks = self.delimited()?;
                    let expanded = self.expand_user(&name, toks, span)?;
                    let mut sub = Parser::new(expanded, self.macros);
                    sub.depth = self.depth + 1;
                    let mut stmts = sub.stmts_to_end()?;
                    let semi = self.eat(";") || brace;
                    if let Some(Stmt::Expr(_, s)) = stmts.last_mut() {
                        if semi {
                            *s = true;
                        }
                    }
                    if self.is(".") || self.is("?") {
                        // `m!(..).method()`: treat the expansion as an expression.
                        if let Some(Stmt::Expr(e, _)) = stmts.pop() {
                            let e = self.expr_with_lhs(e)?;
                            let semi = self.eat(";");
                            stmts.push(Stmt::Expr(e, semi));
                        }
                    }
                    if attrs.is_some() {
                        out.extend(stmts);
                    }
                    return Ok(());
                }
            }
        }
        let e = if self.block_like() {
            let e = self.block_like_expr()?;
            if self.is(".") || self.is("?") {
                self.expr_with_lhs(e)?
            } else {
                let semi = self.eat(";");
                if attrs.is_some() {
                    out.push(Stmt::Expr(e, semi || !(self.is("}") || self.at_eof())));
                }
                return Ok(());
            }
        } else {
            self.expr()?
        };
        let semi = self.eat(";");
        if !semi && !self.is("}") && !self.at_eof() {
            return err(self.span(), format!("expected `;`, found {}", self.describe()));
        }
        if attrs.is_some() {
            out.push(Stmt::Expr(e, semi));
        }
        Ok(())
    }

    fn is_builtin_macro(&self) -> bool {
        matches!(self.tok(), Tok::Ident(s) if BUILTIN_MACROS.contains(&&**s))
    }

    fn item_start(&self) -> bool {
        match self.tok() {
            Tok::Ident(s) => match &**s {
                "fn" | "struct" | "enum" | "trait" | "impl" | "mod" | "use" | "static" | "pub" | "extern" => true,
                "type" => self.is_ident_at(1),
                "const" => !self.is_at(1, "{"),
                "unsafe" => self.is_kw_at(1, "fn") || self.is_kw_at(1, "impl") || self.is_kw_at(1, "trait"),
                "macro_rules" => self.is_at(1, "!"),
                "union" => self.is_ident_at(1),
                _ => false,
            },
            _ => false,
        }
    }

    fn is_ident_at(&self, k: usize) -> bool {
        matches!(self.peek(k), Tok::Ident(s) if !is_keyword(s))
    }

    fn block_like(&self) -> bool {
        match self.tok() {
            Tok::Ident(s) => match &**s {
                "if" | "match" | "loop" | "while" | "for" => true,
                "unsafe" => self.is_at(1, "{"),
                _ => false,
            },
            Tok::Punct("{") => true,
            Tok::Lifetime(_) => self.is_at(1, ":"),
            _ => false,
        }
    }

    fn block_like_expr(&mut self) -> Result<Expr> {
        self.primary()
    }

    // ---- expressions ----

    pub fn expr(&mut self) -> Result<Expr> {
        self.assign()
    }

    /// An expression without struct literals at the top level.
    fn expr_ns(&mut self) -> Result<Expr> {
        let save = self.no_struct;
        self.no_struct = true;
        let e = self.expr();
        self.no_struct = save;
        e
    }

    /// Continues parsing an expression whose first operand is `lhs`.
    fn expr_with_lhs(&mut self, lhs: Expr) -> Result<Expr> {
        let e = self.postfix_rest(lhs)?;
        let e = self.cast_rest(e)?;
        let e = self.binary_rest(e, 0)?;
        let e = self.range_rest(Some(e))?;
        self.assign_rest(e)
    }

    fn assign(&mut self) -> Result<Expr> {
        let lhs = self.range()?;
        self.assign_rest(lhs)
    }

    fn assign_rest(&mut self, lhs: Expr) -> Result<Expr> {
        let span = lhs.span;
        let op = match self.tok() {
            Tok::Punct("=") => None,
            Tok::Punct("+=") => Some(BinOp::Add),
            Tok::Punct("-=") => Some(BinOp::Sub),
            Tok::Punct("*=") => Some(BinOp::Mul),
            Tok::Punct("/=") => Some(BinOp::Div),
            Tok::Punct("%=") => Some(BinOp::Rem),
            Tok::Punct("&=") => Some(BinOp::BitAnd),
            Tok::Punct("|=") => Some(BinOp::BitOr),
            Tok::Punct("^=") => Some(BinOp::BitXor),
            Tok::Punct("<<=") => Some(BinOp::Shl),
            Tok::Punct(">>=") => Some(BinOp::Shr),
            _ => return Ok(lhs),
        };
        self.bump();
        let rhs = self.assign()?;
        let kind = match op {
            None => ExprKind::Assign(Box::new(lhs), Box::new(rhs)),
            Some(op) => ExprKind::AssignOp(op, Box::new(lhs), Box::new(rhs)),
        };
        Ok(Expr { kind, span })
    }

    fn range_end_follows(&self) -> bool {
        match self.tok() {
            Tok::Punct(p) => matches!(*p, "(" | "[" | "-" | "!" | "*" | "&" | "&&" | "|" | "||" | "<" | "::") || (*p == "{" && !self.no_struct),
            Tok::Ident(s) => !matches!(&**s, "as" | "else"),
            Tok::Eof => false,
            _ => true,
        }
    }

    fn range(&mut self) -> Result<Expr> {
        if self.is("..") || self.is("..=") {
            return self.range_rest(None);
        }
        let lhs = self.binary(0)?;
        self.range_rest(Some(lhs))
    }

    fn range_rest(&mut self, lhs: Option<Expr>) -> Result<Expr> {
        let span = lhs.as_ref().map(|e| e.span).unwrap_or(self.span());
        let inclusive = if self.is("..=") || self.is("...") {
            true
        } else if self.is("..") {
            false
        } else {
            return Ok(lhs.unwrap());
        };
        self.bump();
        let rhs = if self.range_end_follows() { Some(Box::new(self.binary(0)?)) } else { None };
        Ok(Expr {
            kind: ExprKind::Range(lhs.map(Box::new), rhs, inclusive),
            span,
        })
    }

    fn bin_op(&self) -> Option<(BinOp, u8)> {
        let t = match self.tok() {
            Tok::Punct(p) => *p,
            _ => return None,
        };
        Some(match t {
            "||" => (BinOp::Or, 1),
            "&&" => (BinOp::And, 2),
            "==" => (BinOp::Eq, 3),
            "!=" => (BinOp::Ne, 3),
            "<" => (BinOp::Lt, 3),
            "<=" => (BinOp::Le, 3),
            ">" => (BinOp::Gt, 3),
            ">=" => (BinOp::Ge, 3),
            "|" => (BinOp::BitOr, 4),
            "^" => (BinOp::BitXor, 5),
            "&" => (BinOp::BitAnd, 6),
            "<<" => (BinOp::Shl, 7),
            ">>" => (BinOp::Shr, 7),
            "+" => (BinOp::Add, 8),
            "-" => (BinOp::Sub, 8),
            "*" => (BinOp::Mul, 9),
            "/" => (BinOp::Div, 9),
            "%" => (BinOp::Rem, 9),
            _ => return None,
        })
    }

    fn binary(&mut self, min: u8) -> Result<Expr> {
        let lhs = self.cast()?;
        self.binary_rest(lhs, min)
    }

    fn binary_rest(&mut self, mut lhs: Expr, min: u8) -> Result<Expr> {
        while let Some((op, prec)) = self.bin_op() {
            if prec <= min {
                break;
            }
            self.bump();
            // Comparisons do not chain; everything else is left-associative.
            let rhs = self.binary(prec)?;
            let span = lhs.span;
            lhs = Expr {
                kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
                span,
            };
        }
        Ok(lhs)
    }

    fn cast(&mut self) -> Result<Expr> {
        let e = self.unary()?;
        self.cast_rest(e)
    }

    fn cast_rest(&mut self, mut e: Expr) -> Result<Expr> {
        while self.is_kw("as") {
            self.bump();
            let t = self.ty_no_bounds()?;
            let span = e.span;
            e = Expr {
                kind: ExprKind::Cast(Box::new(e), t),
                span,
            };
        }
        Ok(e)
    }

    fn unary(&mut self) -> Result<Expr> {
        let span = self.span();
        let kind = match self.tok() {
            Tok::Punct("-") => {
                self.bump();
                ExprKind::Unary(UnOp::Neg, Box::new(self.unary()?))
            }
            Tok::Punct("!") => {
                self.bump();
                ExprKind::Unary(UnOp::Not, Box::new(self.unary()?))
            }
            Tok::Punct("*") => {
                self.bump();
                ExprKind::Unary(UnOp::Deref, Box::new(self.unary()?))
            }
            Tok::Punct(p) if p.starts_with('&') && *p != "&=" => {
                self.eat("&");
                // `&raw const x` is not supported; `&mut x`
                let m = self.eat_kw("mut");
                ExprKind::Ref(m, Box::new(self.unary()?))
            }
            _ => return self.postfix(),
        };
        Ok(Expr { kind, span })
    }

    fn postfix(&mut self) -> Result<Expr> {
        let e = self.primary()?;
        self.postfix_rest(e)
    }

    fn postfix_rest(&mut self, mut e: Expr) -> Result<Expr> {
        loop {
            let span = self.span();
            match self.tok().clone() {
                Tok::Punct("?") => {
                    self.bump();
                    e = Expr {
                        kind: ExprKind::Try(Box::new(e)),
                        span,
                    };
                }
                Tok::Punct("(") => {
                    let args = self.call_args()?;
                    let s = e.span;
                    e = Expr {
                        kind: ExprKind::Call(Box::new(e), args),
                        span: s,
                    };
                }
                Tok::Punct("[") => {
                    // The span is the bracket (where rustc reports panics of
                    // overloaded indexing).
                    self.bump();
                    let save = self.no_struct;
                    self.no_struct = false;
                    let idx = self.expr()?;
                    self.no_struct = save;
                    self.expect("]")?;
                    e = Expr {
                        kind: ExprKind::Index(Box::new(e), Box::new(idx)),
                        span,
                    };
                }
                Tok::Punct(".") => {
                    self.bump();
                    let fspan = self.span();
                    match self.tok().clone() {
                        Tok::Int(n, None) => {
                            self.bump();
                            e = Expr {
                                kind: ExprKind::TupleField(Box::new(e), n as usize),
                                span: fspan,
                            };
                        }
                        Tok::Float(text, None) => {
                            // `t.0.1` lexes as `t` `.` `0.1`
                            self.bump();
                            for part in text.split('.') {
                                let n: usize = part.parse().map_err(|_| Error::new(fspan, String::from("invalid tuple index")))?;
                                e = Expr {
                                    kind: ExprKind::TupleField(Box::new(e), n),
                                    span: fspan,
                                };
                            }
                        }
                        Tok::Ident(name) if &*name == "await" => {
                            return err(fspan, String::from("async/await is not supported"));
                        }
                        Tok::Ident(name) => {
                            self.bump();
                            let args = if self.is("::") {
                                self.bump();
                                Some(self.generic_args()?)
                            } else {
                                None
                            };
                            if self.is("(") {
                                let params = self.call_args()?;
                                e = Expr {
                                    kind: ExprKind::MethodCall {
                                        recv: Box::new(e),
                                        name,
                                        args,
                                        params,
                                    },
                                    span: fspan,
                                };
                            } else {
                                e = Expr {
                                    kind: ExprKind::Field(Box::new(e), name),
                                    span: fspan,
                                };
                            }
                        }
                        _ => return err(fspan, format!("unexpected token: {}", self.describe())),
                    }
                }
                _ => return Ok(e),
            }
        }
    }

    fn call_args(&mut self) -> Result<Vec<Expr>> {
        self.expect("(")?;
        let save = self.no_struct;
        self.no_struct = false;
        let mut args = Vec::new();
        while !self.eat(")") {
            args.push(self.expr()?);
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        self.no_struct = save;
        Ok(args)
    }

    fn label(&mut self) -> Option<Name> {
        if let Tok::Lifetime(l) = self.tok().clone() {
            if self.is_at(1, ":") {
                self.bump();
                self.bump();
                return Some(l);
            }
        }
        None
    }

    fn primary(&mut self) -> Result<Expr> {
        let span = self.span();
        let lit = |l: Lit| -> Result<Expr> {
            Ok(Expr {
                kind: ExprKind::Lit(l),
                span,
            })
        };
        match self.tok().clone() {
            Tok::Int(v, s) => {
                self.bump();
                return lit(Lit::Int(v, s));
            }
            Tok::Float(t, s) => {
                self.bump();
                return lit(Lit::Float(t, s));
            }
            Tok::Str(s) => {
                self.bump();
                return lit(Lit::Str(s));
            }
            Tok::ByteStr(s) => {
                self.bump();
                return lit(Lit::ByteStr(s));
            }
            Tok::Char(c) => {
                self.bump();
                return lit(Lit::Char(c));
            }
            Tok::Byte(b) => {
                self.bump();
                return lit(Lit::Byte(b));
            }
            Tok::Lifetime(_) => {
                let label = self.label();
                if label.is_none() {
                    return err(span, String::from("expected expression, found lifetime"));
                }
                return match self.tok().clone() {
                    Tok::Ident(k) if &*k == "loop" || &*k == "while" || &*k == "for" => self.loop_expr(label),
                    Tok::Punct("{") => {
                        let b = self.block()?;
                        Ok(Expr {
                            kind: ExprKind::Block(Rc::new(b), label),
                            span,
                        })
                    }
                    _ => err(span, String::from("expected `loop`, `while`, `for` or a block after a label")),
                };
            }
            Tok::Punct("(") => {
                self.bump();
                let save = self.no_struct;
                self.no_struct = false;
                let mut elems = Vec::new();
                let mut trailing = false;
                while !self.eat(")") {
                    elems.push(self.expr()?);
                    trailing = self.eat(",");
                    if !trailing {
                        self.expect(")")?;
                        break;
                    }
                }
                self.no_struct = save;
                if elems.len() == 1 && !trailing {
                    return Ok(Expr {
                        kind: ExprKind::Paren(Box::new(elems.pop().unwrap())),
                        span,
                    });
                }
                return Ok(Expr {
                    kind: ExprKind::Tuple(elems),
                    span,
                });
            }
            Tok::Punct("[") => {
                self.bump();
                let save = self.no_struct;
                self.no_struct = false;
                if self.eat("]") {
                    self.no_struct = save;
                    return Ok(Expr {
                        kind: ExprKind::Array(Vec::new()),
                        span,
                    });
                }
                let first = self.expr()?;
                if self.eat(";") {
                    let n = self.expr()?;
                    self.expect("]")?;
                    self.no_struct = save;
                    return Ok(Expr {
                        kind: ExprKind::ArrayRepeat(Box::new(first), Box::new(n)),
                        span,
                    });
                }
                let mut elems = vec![first];
                while self.eat(",") {
                    if self.is("]") {
                        break;
                    }
                    elems.push(self.expr()?);
                }
                self.expect("]")?;
                self.no_struct = save;
                return Ok(Expr {
                    kind: ExprKind::Array(elems),
                    span,
                });
            }
            Tok::Punct("{") => {
                let b = self.block()?;
                return Ok(Expr {
                    kind: ExprKind::Block(Rc::new(b), None),
                    span,
                });
            }
            Tok::Punct("|") | Tok::Punct("||") => return self.closure(false),
            Tok::Punct("<") | Tok::Punct("<<") | Tok::Punct("::") => {
                let p = self.path(PathMode::Expr)?;
                return Ok(Expr {
                    kind: ExprKind::Path(p),
                    span,
                });
            }
            Tok::Ident(k) => match &*k {
                "true" | "false" => {
                    self.bump();
                    return lit(Lit::Bool(&*k == "true"));
                }
                "if" => return self.if_expr(),
                "match" => {
                    self.bump();
                    let scrut = self.expr_ns()?;
                    self.expect("{")?;
                    let save = self.no_struct;
                    self.no_struct = false;
                    self.inner_attrs()?;
                    let mut arms = Vec::new();
                    while !self.eat("}") {
                        let attrs = self.attrs()?;
                        let aspan = self.span();
                        let pat = self.pat_top()?;
                        let guard = if self.eat_kw("if") { Some(self.expr()?) } else { None };
                        self.expect("=>")?;
                        let body = if self.block_like() && !self.is_kw("if") && !self.is_kw("match") {
                            let b = self.block_like_expr()?;
                            if self.is(".") || self.is("?") {
                                self.expr_with_lhs(b)?
                            } else {
                                b
                            }
                        } else {
                            self.expr()?
                        };
                        let is_block = matches!(body.kind, ExprKind::Block(..) | ExprKind::If(..) | ExprKind::Match(..) | ExprKind::Loop(..) | ExprKind::While(..) | ExprKind::For(..) | ExprKind::Unsafe(..));
                        if !self.eat(",") && !self.is("}") && !is_block {
                            return err(self.span(), format!("expected one of `,`, `.`, `?`, `}}`, or an operator, found {}", self.describe()));
                        }
                        if attrs.is_some() {
                            arms.push(Arm {
                                pat,
                                guard,
                                body,
                                span: aspan,
                            });
                        }
                    }
                    self.no_struct = save;
                    return Ok(Expr {
                        kind: ExprKind::Match(Box::new(scrut), arms),
                        span,
                    });
                }
                "loop" | "while" | "for" => return self.loop_expr(None),
                "unsafe" => {
                    self.bump();
                    let b = self.block()?;
                    return Ok(Expr {
                        kind: ExprKind::Unsafe(Rc::new(b)),
                        span,
                    });
                }
                "const" if self.is_at(1, "{") => {
                    self.bump();
                    let b = self.block()?;
                    return Ok(Expr {
                        kind: ExprKind::Block(Rc::new(b), None),
                        span,
                    });
                }
                "move" => {
                    self.bump();
                    return self.closure(true);
                }
                "return" => {
                    self.bump();
                    let v = if self.expr_follows() { Some(Box::new(self.expr()?)) } else { None };
                    return Ok(Expr {
                        kind: ExprKind::Return(v),
                        span,
                    });
                }
                "break" => {
                    self.bump();
                    let label = if let Tok::Lifetime(l) = self.tok().clone() {
                        self.bump();
                        Some(l)
                    } else {
                        None
                    };
                    let v = if self.expr_follows() && !(self.no_struct && self.is("{")) { Some(Box::new(self.expr()?)) } else { None };
                    return Ok(Expr {
                        kind: ExprKind::Break(label, v),
                        span,
                    });
                }
                "continue" => {
                    self.bump();
                    let label = if let Tok::Lifetime(l) = self.tok().clone() {
                        self.bump();
                        Some(l)
                    } else {
                        None
                    };
                    return Ok(Expr {
                        kind: ExprKind::Continue(label),
                        span,
                    });
                }
                "let" => {
                    self.bump();
                    let pat = self.pat_top()?;
                    self.expect("=")?;
                    // The scrutinee binds tighter than `&&`/`||`.
                    let save = self.no_struct;
                    self.no_struct = true;
                    let e = self.binary(2)?;
                    self.no_struct = save;
                    return Ok(Expr {
                        kind: ExprKind::Let(Box::new(pat), Box::new(e)),
                        span,
                    });
                }
                "async" => return err(span, String::from("async blocks are not supported")),
                _ => {}
            },
            _ => {}
        }
        if !matches!(self.tok(), Tok::Ident(_)) {
            return err(span, format!("expected expression, found {}", self.describe()));
        }
        // Macro invocation?
        if self.is_ident() && self.is_at(1, "!") && !self.is_at(1, "!=") {
            if let Tok::Ident(name) = self.tok().clone() {
                self.bump();
                self.bump();
                return self.macro_call(name, span);
            }
        }
        let path = self.path(PathMode::Expr)?;
        if self.is("{") && !self.no_struct && self.struct_lit_follows() {
            return self.struct_lit(path, span);
        }
        Ok(Expr {
            kind: ExprKind::Path(path),
            span,
        })
    }

    /// After `Path {`: a struct literal (`{}`, `{ x, ..}`, `{ x: e }`)
    /// rather than a block.
    fn struct_lit_follows(&self) -> bool {
        match self.peek(1) {
            Tok::Punct("}") => true,
            Tok::Punct("..") => true,
            Tok::Ident(_) | Tok::Int(..) => matches!(self.peek(2), Tok::Punct(":") | Tok::Punct(",") | Tok::Punct("}")),
            _ => false,
        }
    }

    fn expr_follows(&self) -> bool {
        match self.tok() {
            Tok::Punct(p) => !matches!(*p, ";" | "}" | ")" | "]" | "," | "=>" | "=" ),
            Tok::Ident(s) => !matches!(&**s, "else" | "as"),
            Tok::Eof => false,
            _ => true,
        }
    }

    fn struct_lit(&mut self, path: Path, span: Span) -> Result<Expr> {
        self.expect("{")?;
        let mut fields = Vec::new();
        let mut base = None;
        while !self.eat("}") {
            if self.eat("..") {
                if !self.is("}") {
                    base = Some(Box::new(self.expr()?));
                }
                self.expect("}")?;
                break;
            }
            let fspan = self.span();
            let name: Name = match self.tok().clone() {
                Tok::Int(n, None) => {
                    self.bump();
                    Rc::from(n.to_string().as_str())
                }
                _ => self.ident()?,
            };
            let value = if self.eat(":") {
                self.expr()?
            } else {
                Expr {
                    kind: ExprKind::Path(Path::single(name.clone(), fspan)),
                    span: fspan,
                }
            };
            fields.push((name, value, fspan));
            if !self.eat(",") {
                self.expect("}")?;
                break;
            }
        }
        Ok(Expr {
            kind: ExprKind::StructLit { path, fields, base },
            span,
        })
    }

    fn if_expr(&mut self) -> Result<Expr> {
        let span = self.span();
        self.expect_kw("if")?;
        let cond = self.expr_ns()?;
        let then = self.block()?;
        let els = if self.eat_kw("else") {
            if self.is_kw("if") {
                Some(Box::new(self.if_expr()?))
            } else {
                let bspan = self.span();
                let b = self.block()?;
                Some(Box::new(Expr {
                    kind: ExprKind::Block(Rc::new(b), None),
                    span: bspan,
                }))
            }
        } else {
            None
        };
        Ok(Expr {
            kind: ExprKind::If(Box::new(cond), Rc::new(then), els),
            span,
        })
    }

    fn loop_expr(&mut self, label: Option<Name>) -> Result<Expr> {
        let span = self.span();
        if self.eat_kw("loop") {
            let b = self.block()?;
            return Ok(Expr {
                kind: ExprKind::Loop(Rc::new(b), label),
                span,
            });
        }
        if self.eat_kw("while") {
            let cond = self.expr_ns()?;
            let b = self.block()?;
            return Ok(Expr {
                kind: ExprKind::While(Box::new(cond), Rc::new(b), label),
                span,
            });
        }
        self.expect_kw("for")?;
        let pat = self.pat_top()?;
        self.expect_kw("in")?;
        let it = self.expr_ns()?;
        let b = self.block()?;
        Ok(Expr {
            kind: ExprKind::For(Box::new(pat), Box::new(it), Rc::new(b), label),
            span,
        })
    }

    fn closure(&mut self, is_move: bool) -> Result<Expr> {
        let span = self.span();
        let mut params = Vec::new();
        if !self.eat("||") {
            self.expect("|")?;
            while !self.eat("|") {
                let pat = self.pat_single()?;
                let ty = if self.eat(":") { Some(self.ty_no_bounds()?) } else { None };
                params.push(ClosureParam { pat, ty });
                if !self.eat(",") {
                    self.expect("|")?;
                    break;
                }
            }
        }
        let (ret, body) = if self.eat("->") {
            let t = self.ty_no_bounds()?;
            let bspan = self.span();
            let b = self.block()?;
            (
                Some(t),
                Expr {
                    kind: ExprKind::Block(Rc::new(b), None),
                    span: bspan,
                },
            )
        } else {
            let save = self.no_struct;
            self.no_struct = false;
            let e = self.expr()?;
            self.no_struct = save;
            (None, e)
        };
        Ok(Expr {
            kind: ExprKind::Closure {
                params,
                ret,
                body: Rc::new(body),
                is_move,
            },
            span,
        })
    }

    // ---- patterns ----

    pub fn pat_top(&mut self) -> Result<Pat> {
        let span = self.span();
        self.eat("|");
        let first = self.pat_single()?;
        if !self.is("|") {
            return Ok(first);
        }
        let mut alts = vec![first];
        while self.is("|") {
            self.bump();
            alts.push(self.pat_single()?);
        }
        Ok(Pat {
            kind: PatKind::Or(alts),
            span,
        })
    }

    fn pat_list(&mut self, close: &str) -> Result<(Vec<Pat>, bool)> {
        let mut pats = Vec::new();
        let mut trailing = false;
        while !self.eat(close) {
            pats.push(self.pat_top()?);
            trailing = self.eat(",");
            if !trailing {
                self.expect(close)?;
                break;
            }
        }
        Ok((pats, trailing))
    }

    fn lit_pat_expr(&mut self) -> Result<Option<Expr>> {
        let span = self.span();
        let neg = self.is("-");
        let k = if neg { 1 } else { 0 };
        let lit = match self.peek(k).clone() {
            Tok::Int(v, s) => Lit::Int(v, s),
            Tok::Float(t, s) => Lit::Float(t, s),
            Tok::Str(s) if !neg => Lit::Str(s),
            Tok::ByteStr(s) if !neg => Lit::ByteStr(s),
            Tok::Char(c) if !neg => Lit::Char(c),
            Tok::Byte(b) if !neg => Lit::Byte(b),
            Tok::Ident(s) if !neg && (&*s == "true" || &*s == "false") => Lit::Bool(&*s == "true"),
            _ => return Ok(None),
        };
        if neg {
            self.bump();
        }
        self.bump();
        let e = Expr {
            kind: ExprKind::Lit(lit),
            span,
        };
        Ok(Some(if neg {
            Expr {
                kind: ExprKind::Unary(UnOp::Neg, Box::new(e)),
                span,
            }
        } else {
            e
        }))
    }

    /// The upper end of a range pattern (a literal or a path).
    fn range_pat_end(&mut self) -> Result<Option<Box<Expr>>> {
        if let Some(e) = self.lit_pat_expr()? {
            return Ok(Some(Box::new(e)));
        }
        if matches!(self.tok(), Tok::Ident(_)) || self.is("::") || self.is("<") {
            let span = self.span();
            let p = self.path(PathMode::Expr)?;
            return Ok(Some(Box::new(Expr {
                kind: ExprKind::Path(p),
                span,
            })));
        }
        Ok(None)
    }

    fn range_pat(&mut self, lo: Option<Box<Expr>>, span: Span) -> Result<Pat> {
        let inclusive = self.is("..=") || self.is("...");
        self.bump();
        let hi = self.range_pat_end()?;
        Ok(Pat {
            kind: PatKind::Range(lo, hi, inclusive),
            span,
        })
    }

    pub fn pat_single(&mut self) -> Result<Pat> {
        let span = self.span();
        let mk = |kind| Ok(Pat { kind, span });
        if let Some(e) = self.lit_pat_expr()? {
            if self.is("..=") || self.is("...") || (self.is("..") && !self.is_at(1, ",") && !self.is_at(1, ")") && !self.is_at(1, "]")) {
                return self.range_pat(Some(Box::new(e)), span);
            }
            if self.is("..") {
                self.bump();
                return mk(PatKind::Range(Some(Box::new(e)), None, false));
            }
            return mk(PatKind::Lit(Box::new(e)));
        }
        match self.tok().clone() {
            Tok::Punct("..=") => {
                self.bump();
                let hi = self.range_pat_end()?;
                return mk(PatKind::Range(None, hi, true));
            }
            Tok::Punct("..") => {
                self.bump();
                return mk(PatKind::Rest);
            }
            Tok::Punct(p) if p.starts_with('&') && p != "&=" => {
                self.eat("&");
                let m = self.eat_kw("mut");
                let inner = self.pat_single()?;
                return mk(PatKind::Ref(Box::new(inner), m));
            }
            Tok::Punct("(") => {
                self.bump();
                let (mut pats, trailing) = self.pat_list(")")?;
                if pats.len() == 1 && !trailing && !matches!(pats[0].kind, PatKind::Rest) {
                    return Ok(pats.pop().unwrap());
                }
                return mk(PatKind::Tuple(pats));
            }
            Tok::Punct("[") => {
                self.bump();
                let (pats, _) = self.pat_list("]")?;
                return mk(PatKind::Slice(pats));
            }
            Tok::Ident(k) if &*k == "_" => {
                self.bump();
                return mk(PatKind::Wild);
            }
            Tok::Ident(k) if &*k == "box" => {
                self.bump();
                let inner = self.pat_single()?;
                return mk(PatKind::Box(Box::new(inner)));
            }
            Tok::Ident(k) if &*k == "ref" || &*k == "mut" => {
                let by_ref = self.eat_kw("ref");
                let mutable = self.eat_kw("mut");
                let name = self.ident_or_self()?;
                let sub = if self.eat("@") { Some(Box::new(self.pat_single()?)) } else { None };
                return mk(PatKind::Ident {
                    name,
                    by_ref,
                    mutable,
                    sub,
                });
            }
            _ => {}
        }
        if !matches!(self.tok(), Tok::Ident(_)) && !self.is("::") && !self.is("<") {
            return err(span, format!("expected pattern, found {}", self.describe()));
        }
        // A plain binding?
        if self.is_ident() && !self.is_at(1, "::") && !self.is_at(1, "(") && !self.is_at(1, "{") && !self.is_at(1, "!") {
            let name = self.ident()?;
            if self.eat("@") {
                let sub = self.pat_single()?;
                return mk(PatKind::Ident {
                    name,
                    by_ref: false,
                    mutable: false,
                    sub: Some(Box::new(sub)),
                });
            }
            if self.is("..=") || self.is("...") {
                let lo = Expr {
                    kind: ExprKind::Path(Path::single(name, span)),
                    span,
                };
                return self.range_pat(Some(Box::new(lo)), span);
            }
            return mk(PatKind::Ident {
                name,
                by_ref: false,
                mutable: false,
                sub: None,
            });
        }
        let path = self.path(PathMode::Expr)?;
        if self.is("(") {
            self.bump();
            let (pats, _) = self.pat_list(")")?;
            return mk(PatKind::TupleStruct(path, pats));
        }
        if self.is("{") {
            self.bump();
            let mut fields = Vec::new();
            let mut rest = false;
            while !self.eat("}") {
                self.attrs()?;
                if self.eat("..") {
                    rest = true;
                    self.expect("}")?;
                    break;
                }
                let fspan = self.span();
                if self.is_kw("ref") || self.is_kw("mut") || self.is_kw("box") {
                    let p = self.pat_single()?;
                    let name = match &p.kind {
                        PatKind::Ident { name, .. } => name.clone(),
                        _ => return err(fspan, String::from("expected field name")),
                    };
                    fields.push((name, p));
                } else {
                    let name: Name = match self.tok().clone() {
                        Tok::Int(n, None) => {
                            self.bump();
                            Rc::from(n.to_string().as_str())
                        }
                        _ => self.ident()?,
                    };
                    let p = if self.eat(":") {
                        self.pat_top()?
                    } else {
                        Pat {
                            kind: PatKind::Ident {
                                name: name.clone(),
                                by_ref: false,
                                mutable: false,
                                sub: None,
                            },
                            span: fspan,
                        }
                    };
                    fields.push((name, p));
                }
                if !self.eat(",") {
                    self.expect("}")?;
                    break;
                }
            }
            return mk(PatKind::Struct(path, fields, rest));
        }
        if self.is("..=") || self.is("...") {
            let lo = Expr {
                kind: ExprKind::Path(path),
                span,
            };
            return self.range_pat(Some(Box::new(lo)), span);
        }
        mk(PatKind::Path(path))
    }

    fn ident_or_self(&mut self) -> Result<Name> {
        if self.is_kw("self") {
            self.bump();
            return Ok(Rc::from("self"));
        }
        self.ident()
    }

    // ---- macros ----

    fn macro_call(&mut self, name: Name, span: Span) -> Result<Expr> {
        if !BUILTIN_MACROS.contains(&&*name) || self.macros.get(&name).is_some() {
            let toks = self.delimited()?;
            let expanded = self.expand_user(&name, toks, span)?;
            let mut sub = Parser::new(expanded, self.macros);
            sub.depth = self.depth + 1;
            let stmts = sub.stmts_to_end()?;
            // A single expression stays an expression; statements become a block.
            let mut stmts = stmts;
            if stmts.len() == 1 {
                if let Stmt::Expr(_, false) = &stmts[0] {
                    if let Some(Stmt::Expr(e, _)) = stmts.pop() {
                        return Ok(e);
                    }
                }
            }
            let b = finish_block(stmts, span);
            return Ok(Expr {
                kind: ExprKind::Block(Rc::new(b), None),
                span,
            });
        }
        let toks = self.delimited()?;
        let mut sub = Parser::new(toks, self.macros);
        sub.depth = self.depth + 1;
        let kind = match &*name {
            "println" | "print" | "eprintln" | "eprint" | "format" | "format_args" => {
                let newline = name.ends_with("ln");
                let kind = match &*name {
                    "println" => MacroKind::Println,
                    "print" => MacroKind::Print,
                    "eprintln" => MacroKind::Eprintln,
                    "eprint" => MacroKind::Eprint,
                    "format" => MacroKind::Format,
                    _ => MacroKind::Args,
                };
                let mut f = sub.format_args(span, true)?;
                if newline {
                    push_lit(&mut f.pieces, "\n");
                }
                let kind = match kind {
                    MacroKind::Println => MacroKind::Print,
                    MacroKind::Eprintln => MacroKind::Eprint,
                    k => k,
                };
                ExprKind::Format(kind, Rc::new(f))
            }
            "write" | "writeln" => {
                let dst = sub.expr()?;
                let mut f = if sub.eat(",") && !sub.at_eof() {
                    sub.format_args(span, false)?
                } else {
                    Format {
                        pieces: Vec::new(),
                        args: Vec::new(),
                        span,
                    }
                };
                if &*name == "writeln" {
                    push_lit(&mut f.pieces, "\n");
                }
                ExprKind::Write(Box::new(dst), Rc::new(f))
            }
            "panic" | "unreachable" | "todo" | "unimplemented" => {
                let prefix = match &*name {
                    "unreachable" => "internal error: entered unreachable code",
                    "todo" => "not yet implemented",
                    "unimplemented" => "not implemented",
                    _ => "explicit panic",
                };
                let f = if sub.at_eof() {
                    Format {
                        pieces: vec![FmtPiece::Lit(String::from(prefix))],
                        args: Vec::new(),
                        span,
                    }
                } else {
                    let mut f = sub.format_args(span, true)?;
                    if &*name != "panic" {
                        f.pieces.insert(0, FmtPiece::Lit(format!("{}: ", prefix)));
                    }
                    f
                };
                ExprKind::Format(MacroKind::Panic, Rc::new(f))
            }
            "vec" => {
                if sub.at_eof() {
                    ExprKind::VecLit(Vec::new())
                } else {
                    let first = sub.expr()?;
                    if sub.eat(";") {
                        let n = sub.expr()?;
                        ExprKind::VecRepeat(Box::new(first), Box::new(n))
                    } else {
                        let mut elems = vec![first];
                        while sub.eat(",") {
                            if sub.at_eof() {
                                break;
                            }
                            elems.push(sub.expr()?);
                        }
                        ExprKind::VecLit(elems)
                    }
                }
            }
            "assert" | "debug_assert" => {
                let start = sub.pos;
                let cond = sub.expr()?;
                let text = stringify(&sub.toks[start..sub.pos]);
                let msg = if sub.eat(",") && !sub.at_eof() {
                    Some(Rc::new(sub.format_args(span, true)?))
                } else {
                    Some(Rc::new(Format {
                        pieces: vec![FmtPiece::Lit(format!("assertion failed: {}", text))],
                        args: Vec::new(),
                        span,
                    }))
                };
                ExprKind::Assert(Box::new(cond), msg)
            }
            "assert_eq" | "assert_ne" | "debug_assert_eq" | "debug_assert_ne" => {
                let a = sub.expr()?;
                sub.expect(",")?;
                let b = sub.expr()?;
                let msg = if sub.eat(",") && !sub.at_eof() { Some(Rc::new(sub.format_args(span, true)?)) } else { None };
                ExprKind::AssertCmp(name.ends_with("eq"), Box::new(a), Box::new(b), msg)
            }
            "matches" => {
                let e = sub.expr()?;
                sub.expect(",")?;
                let pat = sub.pat_top()?;
                let guard = if sub.eat_kw("if") { Some(Box::new(sub.expr()?)) } else { None };
                sub.eat(",");
                ExprKind::Matches(Box::new(e), Box::new(pat), guard)
            }
            "dbg" => {
                let start = sub.pos;
                let e = sub.expr()?;
                let text = stringify(&sub.toks[start..sub.pos]);
                sub.eat(",");
                ExprKind::Dbg(Box::new(e), text)
            }
            "line" => ExprKind::Lit(Lit::Int(span.line as u128, Some(Rc::from("u32")))),
            "column" => ExprKind::Lit(Lit::Int(span.col as u128, Some(Rc::from("u32")))),
            "file" => ExprKind::Path(Path::single(Rc::from("__hrc_file"), span)),
            "stringify" => {
                let n = sub.toks.len() - 1;
                ExprKind::Lit(Lit::Str(Rc::from(stringify(&sub.toks[..n]).as_str())))
            }
            "concat" => {
                let mut s = String::new();
                while !sub.at_eof() {
                    match sub.bump().tok {
                        Tok::Str(t) => s.push_str(&t),
                        Tok::Int(v, _) => s.push_str(&v.to_string()),
                        Tok::Float(t, _) => s.push_str(&t),
                        Tok::Char(c) => s.push(c),
                        Tok::Ident(b) if &*b == "true" || &*b == "false" => s.push_str(&b),
                        Tok::Punct(",") => {}
                        _ => return err(span, String::from("expected a literal in concat!")),
                    }
                }
                ExprKind::Lit(Lit::Str(Rc::from(s.as_str())))
            }
            _ => return err(span, format!("cannot find macro `{}` in this scope", name)),
        };
        if !sub.at_eof() {
            return err(sub.span(), format!("unexpected token in `{}!`: {}", name, sub.describe()));
        }
        Ok(Expr { kind, span })
    }

    /// `"fmt", args...` (the format string must come first).
    fn format_args(&mut self, span: Span, allow_non_literal: bool) -> Result<Format> {
        if self.at_eof() {
            return Ok(Format {
                pieces: Vec::new(),
                args: Vec::new(),
                span,
            });
        }
        let fspan = self.span();
        let fmt = match self.tok().clone() {
            Tok::Str(s) => {
                self.bump();
                s
            }
            _ if allow_non_literal => {
                // `panic!(msg)` in old editions: display the value.
                let e = self.expr()?;
                self.eat(",");
                return Ok(Format {
                    pieces: vec![FmtPiece::Arg(plain_arg(0))],
                    args: vec![e],
                    span,
                });
            }
            _ => return err(fspan, String::from("format argument must be a string literal")),
        };
        let mut args = Vec::new();
        let mut named: Vec<(Name, usize)> = Vec::new();
        while self.eat(",") {
            if self.at_eof() {
                break;
            }
            if self.is_ident() && self.is_at(1, "=") {
                let n = self.ident()?;
                self.bump();
                named.push((n, args.len()));
            }
            args.push(self.expr()?);
        }
        let explicit = args.len();
        let pieces = parse_format(&fmt, fspan, &mut args, &named)?;
        // Every explicit argument must be used.
        let mut used = vec![false; args.len()];
        for p in &pieces {
            if let FmtPiece::Arg(a) = p {
                used[a.arg] = true;
                for c in [&a.width, &a.precision].into_iter().flatten() {
                    if let FmtCount::Arg(i) = c {
                        used[*i] = true;
                    }
                }
            }
        }
        if let Some(i) = (0..explicit).find(|&i| !used[i]) {
            return err(args[i].span, String::from("argument never used"));
        }
        Ok(Format { pieces, args, span })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PathMode {
    Type,
    Expr,
}

const BUILTIN_MACROS: &[&str] = &[
    "println", "print", "eprintln", "eprint", "format", "format_args", "write", "writeln", "panic", "unreachable",
    "todo", "unimplemented", "vec", "assert", "debug_assert", "assert_eq", "assert_ne", "debug_assert_eq",
    "debug_assert_ne", "matches", "dbg", "line", "column", "file", "stringify", "concat",
];

fn is_self_ty(t: &Type) -> bool {
    matches!(&t.kind, TypeKind::Path(p) if p.as_ident().is_some_and(|n| &**n == "Self"))
}

fn finish_block(mut stmts: Vec<Stmt>, span: Span) -> Block {
    let expr = match stmts.last() {
        Some(Stmt::Expr(_, false)) => match stmts.pop() {
            Some(Stmt::Expr(e, _)) => Some(Box::new(e)),
            _ => None,
        },
        _ => None,
    };
    Block { stmts, expr, span }
}

pub fn describe(t: &Tok) -> String {
    match t {
        Tok::Ident(s) if is_keyword(s) => format!("keyword `{}`", s),
        Tok::Ident(s) => format!("`{}`", s),
        Tok::Lifetime(s) => format!("`'{}`", s),
        Tok::Int(v, _) => format!("`{}`", v),
        Tok::Float(t, _) => format!("`{}`", t),
        Tok::Str(_) | Tok::ByteStr(_) => String::from("string literal"),
        Tok::Char(c) => format!("`'{}'`", c),
        Tok::Byte(_) => String::from("byte literal"),
        Tok::Punct(p) => format!("`{}`", p),
        Tok::Eof => String::from("end of input"),
    }
}

fn plain_arg(i: usize) -> FmtArg {
    FmtArg {
        arg: i,
        fill: ' ',
        align: 0,
        plus: false,
        alternate: false,
        zero: false,
        width: None,
        precision: None,
        trait_: FmtTrait::Display,
    }
}

fn push_lit(pieces: &mut Vec<FmtPiece>, s: &str) {
    if let Some(FmtPiece::Lit(l)) = pieces.last_mut() {
        l.push_str(s);
    } else {
        pieces.push(FmtPiece::Lit(String::from(s)));
    }
}

/// Parses a format string; implicit captures (`{x}`) are appended to `args`.
fn parse_format(s: &str, span: Span, args: &mut Vec<Expr>, named: &[(Name, usize)]) -> Result<Vec<FmtPiece>> {
    let chars: Vec<char> = s.chars().collect();
    let mut pieces = Vec::new();
    let mut lit = String::new();
    let mut next = 0usize;
    let mut i = 0;
    let bad = |m: &str| Error::new(span, format!("invalid format string: {}", m));
    // Implicit captures (`{x}`), shared between uses of the same name.
    let mut captured: Vec<(String, usize)> = Vec::new();
    let mut lookup = |name: &str, args: &mut Vec<Expr>| -> usize {
        if let Some((_, i)) = named.iter().find(|(n, _)| &**n == name) {
            return *i;
        }
        if let Some((_, i)) = captured.iter().find(|(n, _)| n == name) {
            return *i;
        }
        args.push(Expr {
            kind: ExprKind::Path(Path::single(Rc::from(name), span)),
            span,
        });
        captured.push((String::from(name), args.len() - 1));
        args.len() - 1
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '{' {
            if chars.get(i + 1) == Some(&'{') {
                lit.push('{');
                i += 2;
                continue;
            }
            i += 1;
            // argument
            let start = i;
            while i < chars.len() && chars[i] != ':' && chars[i] != '}' {
                i += 1;
            }
            let spec_arg: String = chars[start..i].iter().collect::<String>();
            let spec_arg = spec_arg.trim();
            let mut a = plain_arg(0);
            let mut explicit_arg = None;
            if !spec_arg.is_empty() {
                if let Ok(n) = spec_arg.parse::<usize>() {
                    explicit_arg = Some(n);
                } else {
                    explicit_arg = Some(lookup(spec_arg, args));
                }
            }
            let mut precision_star = false;
            if i < chars.len() && chars[i] == ':' {
                i += 1;
                // [[fill]align][sign]['#']['0'][width]['.' precision][type]
                let at = |k: usize| chars.get(k).copied().unwrap_or('}');
                let is_align = |c: char| c == '<' || c == '^' || c == '>';
                if is_align(at(i + 1)) && at(i) != '}' {
                    a.fill = at(i);
                    a.align = align_code(at(i + 1));
                    i += 2;
                } else if is_align(at(i)) {
                    a.align = align_code(at(i));
                    i += 1;
                }
                if at(i) == '+' {
                    a.plus = true;
                    i += 1;
                } else if at(i) == '-' {
                    i += 1;
                }
                if at(i) == '#' {
                    a.alternate = true;
                    i += 1;
                }
                if at(i) == '0' && at(i + 1) != '$' {
                    a.zero = true;
                    i += 1;
                }
                // width
                let ws = i;
                while at(i).is_alphanumeric() || at(i) == '_' {
                    i += 1;
                }
                let wtext: String = chars[ws..i].iter().collect();
                if at(i) == '$' {
                    i += 1;
                    let idx = match wtext.parse::<usize>() {
                        Ok(n) => n,
                        Err(_) => lookup(&wtext, args),
                    };
                    a.width = Some(FmtCount::Arg(idx));
                } else if !wtext.is_empty() {
                    match wtext.parse::<usize>() {
                        Ok(n) => a.width = Some(FmtCount::Is(n)),
                        Err(_) => i = ws, // it was the type
                    }
                }
                if at(i) == '.' {
                    i += 1;
                    if at(i) == '*' {
                        i += 1;
                        precision_star = true;
                    } else {
                        let ps = i;
                        while at(i).is_alphanumeric() || at(i) == '_' {
                            i += 1;
                        }
                        let ptext: String = chars[ps..i].iter().collect();
                        if at(i) == '$' {
                            i += 1;
                            let idx = match ptext.parse::<usize>() {
                                Ok(n) => n,
                                Err(_) => lookup(&ptext, args),
                            };
                            a.precision = Some(FmtCount::Arg(idx));
                        } else {
                            a.precision = Some(FmtCount::Is(ptext.parse().map_err(|_| bad("invalid precision"))?));
                        }
                    }
                }
                let ts = i;
                while at(i) != '}' && i < chars.len() {
                    i += 1;
                }
                let t: String = chars[ts..i].iter().collect();
                a.trait_ = match t.as_str() {
                    "" => FmtTrait::Display,
                    "?" | "x?" | "X?" => FmtTrait::Debug,
                    "x" => FmtTrait::LowerHex,
                    "X" => FmtTrait::UpperHex,
                    "o" => FmtTrait::Octal,
                    "b" => FmtTrait::Binary,
                    "e" => FmtTrait::LowerExp,
                    "E" => FmtTrait::UpperExp,
                    _ => return Err(bad(&format!("unknown format trait `{}`", t))),
                };
            }
            if i >= chars.len() || chars[i] != '}' {
                return Err(bad("expected `}`"));
            }
            i += 1;
            if precision_star {
                a.precision = Some(FmtCount::Arg(next));
                next += 1;
            }
            a.arg = match explicit_arg {
                Some(n) => n,
                None => {
                    next += 1;
                    next - 1
                }
            };
            if a.arg >= args.len() {
                return Err(Error::new(
                    span,
                    format!(
                        "{} positional argument{} in format string, but {}",
                        a.arg + 1,
                        if a.arg == 0 { "" } else { "s" },
                        match args.len() {
                            0 => String::from("no arguments were given"),
                            1 => String::from("there is 1 argument"),
                            n => format!("there are {} arguments", n),
                        }
                    ),
                ));
            }
            if !lit.is_empty() {
                pieces.push(FmtPiece::Lit(core::mem::take(&mut lit)));
            }
            pieces.push(FmtPiece::Arg(a));
        } else if c == '}' {
            if chars.get(i + 1) == Some(&'}') {
                lit.push('}');
                i += 2;
                continue;
            }
            return Err(bad("unmatched `}` found"));
        } else {
            lit.push(c);
            i += 1;
        }
    }
    if !lit.is_empty() {
        pieces.push(FmtPiece::Lit(lit));
    }
    Ok(pieces)
}

fn align_code(c: char) -> u8 {
    match c {
        '<' => 1,
        '^' => 2,
        _ => 3,
    }
}

/// Evaluates a `cfg(...)` predicate.
fn cfg_eval(t: &[Token], i: &mut usize) -> bool {
    let name = match &t.get(*i).map(|x| &x.tok) {
        Some(Tok::Ident(s)) => s.clone(),
        _ => return false,
    };
    *i += 1;
    let is = |t: &[Token], i: usize, p: &str| matches!(t.get(i).map(|x| &x.tok), Some(Tok::Punct(q)) if *q == p);
    if is(t, *i, "(") {
        *i += 1;
        let mut vals = Vec::new();
        while !is(t, *i, ")") && *i < t.len() {
            vals.push(cfg_eval(t, i));
            if is(t, *i, ",") {
                *i += 1;
            } else {
                break;
            }
        }
        *i += 1;
        return match &*name {
            "all" => vals.iter().all(|v| *v),
            "any" => vals.iter().any(|v| *v),
            "not" => !vals.first().copied().unwrap_or(false),
            _ => false,
        };
    }
    if is(t, *i, "=") {
        *i += 1;
        let v = match t.get(*i).map(|x| &x.tok) {
            Some(Tok::Str(s)) => s.clone(),
            _ => Rc::from(""),
        };
        *i += 1;
        return matches!(
            (&*name, &*v),
            ("target_os", "linux") | ("target_arch", "x86_64") | ("target_family", "unix") | ("target_pointer_width", "64") | ("target_endian", "little")
        );
    }
    matches!(&*name, "unix" | "debug_assertions" | "huldra")
}

/// The source text of tokens, spaced like rustc's pretty printer.
pub fn stringify(toks: &[Token]) -> String {
    let mut out = String::new();
    let mut prev: Option<&Tok> = None;
    for t in toks {
        if matches!(t.tok, Tok::Eof) {
            break;
        }
        let text = match &t.tok {
            Tok::Ident(s) => String::from(&**s),
            Tok::Lifetime(s) => format!("'{}", s),
            Tok::Int(v, s) => format!("{}{}", v, s.as_deref().unwrap_or("")),
            Tok::Float(v, s) => format!("{}{}", v, s.as_deref().unwrap_or("")),
            Tok::Str(s) => format!("{:?}", &**s),
            Tok::ByteStr(_) => String::from("b\"..\""),
            Tok::Char(c) => format!("{:?}", c),
            Tok::Byte(b) => format!("b'{}'", *b as char),
            Tok::Punct(p) => String::from(*p),
            Tok::Eof => String::new(),
        };
        let space = match (prev, &t.tok) {
            (None, _) => false,
            (_, Tok::Punct(p)) if matches!(*p, "," | ";" | "." | ")" | "]" | "?" | "::" | ":") => false,
            (_, Tok::Punct("(" | "[")) if matches!(prev, Some(Tok::Ident(_)) | Some(Tok::Punct(")" | "]" | "!" | ">"))) => false,
            (Some(Tok::Punct(p)), _) if matches!(*p, "(" | "[" | "." | "::" | "!" | "&" | "*") => {
                // unary operators stay attached; binary ones are spaced
                match *p {
                    "!" | "&" | "*" => !t.joint,
                    _ => false,
                }
            }
            (Some(Tok::Punct("-")), _) if t.joint => false,
            (_, Tok::Punct("!")) if matches!(prev, Some(Tok::Ident(_))) => false,
            _ => true,
        };
        if space {
            out.push(' ');
        }
        out.push_str(&text);
        prev = Some(&t.tok);
    }
    out
}
