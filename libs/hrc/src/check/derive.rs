//! `#[derive(...)]`: generates the impls as Rust source and parses them.

use crate::ast::*;
use crate::lex;
use crate::macros::Macros;
use crate::parse::Parser;
use crate::{err, Result};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

struct Shape<'a> {
    name: &'a str,
    generics: &'a Generics,
    body: Body<'a>,
}

enum Body<'a> {
    Struct(&'a VariantData),
    Enum(&'a [Variant]),
}

pub fn derive(item: &Item) -> Result<Vec<Item>> {
    let shape = match &item.kind {
        ItemKind::Struct(s) => Shape {
            name: &s.name,
            generics: &s.generics,
            body: Body::Struct(&s.data),
        },
        ItemKind::Enum(e) => Shape {
            name: &e.name,
            generics: &e.generics,
            body: Body::Enum(&e.variants),
        },
        _ => return err(item.span, String::from("`derive` may only be applied to `struct`s and `enum`s")),
    };
    let mut src = String::new();
    for d in &item.attrs.derives {
        let text = match &**d {
            "Debug" => debug(&shape),
            "Clone" => clone(&shape),
            "Copy" => header(&shape, "::std::marker::Copy", "") + "}\n",
            "PartialEq" => partial_eq(&shape),
            "Eq" => header(&shape, "::std::cmp::Eq", "") + "}\n",
            "PartialOrd" => ord(&shape, true),
            "Ord" => ord(&shape, false),
            "Hash" => hash(&shape),
            "Default" => default(&shape, item)?,
            other => return err(item.span, format!("cannot find derive macro `{}` in this scope", other)),
        };
        src.push_str(&text);
    }
    let mut toks = lex::tokenize(&src, item.span.file)?;
    for t in toks.iter_mut() {
        t.span = item.span;
    }
    let mut macros = Macros::default();
    let mut p = Parser::new(toks, &mut macros);
    p.parse_items(true)
}

/// `impl<T: Bound> Trait for Name<T> {`
fn header(s: &Shape, trait_: &str, extra_bound: &str) -> String {
    let params: Vec<&GenericParam> = s.generics.params.iter().collect();
    let mut h = String::from("impl");
    if !params.is_empty() {
        h.push('<');
        let parts: Vec<String> = params
            .iter()
            .map(|p| {
                if p.const_ty.is_some() {
                    format!("const {}: usize", p.name)
                } else if extra_bound.is_empty() {
                    format!("{}: {}", p.name, trait_)
                } else {
                    format!("{}: {} + {}", p.name, trait_, extra_bound)
                }
            })
            .collect();
        h.push_str(&parts.join(", "));
        h.push('>');
    }
    h.push_str(&format!(" {} for {}", trait_, s.name));
    if !params.is_empty() {
        let names: Vec<&str> = params.iter().map(|p| &*p.name).collect();
        h.push_str(&format!("<{}>", names.join(", ")));
    }
    h.push_str(" {\n");
    h
}

fn field_names(d: &VariantData) -> Vec<String> {
    match d {
        VariantData::Named(f) => f.iter().map(|(n, _)| String::from(&**n)).collect(),
        VariantData::Tuple(f) => (0..f.len()).map(|i| format!("{}", i)).collect(),
        VariantData::Unit => Vec::new(),
    }
}

/// `Name::V(x0, x1)` / `Name::V { a: x0 }` / `Name::V` binding with a prefix.
fn variant_pat(enum_name: &str, v: &Variant, prefix: &str) -> String {
    match &v.data {
        VariantData::Named(f) => {
            let parts: Vec<String> = f.iter().enumerate().map(|(i, (n, _))| format!("{}: {}{}", n, prefix, i)).collect();
            format!("{}::{} {{ {} }}", enum_name, v.name, parts.join(", "))
        }
        VariantData::Tuple(f) => {
            let parts: Vec<String> = (0..f.len()).map(|i| format!("{}{}", prefix, i)).collect();
            format!("{}::{}({})", enum_name, v.name, parts.join(", "))
        }
        VariantData::Unit => format!("{}::{}", enum_name, v.name),
    }
}

fn debug_fields(name: &str, d: &VariantData, access: &dyn Fn(usize, &str) -> String) -> String {
    match d {
        VariantData::Named(f) => {
            let mut s = format!("f.debug_struct(\"{}\")", name);
            for (i, (n, _)) in f.iter().enumerate() {
                s.push_str(&format!(".field(\"{}\", {})", n, access(i, n)));
            }
            s + ".finish()"
        }
        VariantData::Tuple(f) => {
            let mut s = format!("f.debug_tuple(\"{}\")", name);
            for i in 0..f.len() {
                s.push_str(&format!(".field({})", access(i, &format!("{}", i))));
            }
            s + ".finish()"
        }
        VariantData::Unit => format!("f.write_str(\"{}\")", name),
    }
}

fn debug(s: &Shape) -> String {
    let mut out = header(s, "::std::fmt::Debug", "");
    out.push_str("fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {\n");
    match &s.body {
        Body::Struct(d) => out.push_str(&debug_fields(s.name, d, &|_, n| format!("&self.{}", n))),
        Body::Enum(vs) => {
            out.push_str("match self {\n");
            for v in vs.iter() {
                out.push_str(&format!(
                    "{} => {},\n",
                    variant_pat(s.name, v, "__x"),
                    debug_fields(&v.name, &v.data, &|i, _| format!("__x{}", i))
                ));
            }
            out.push_str("}\n");
        }
    }
    out.push_str("}\n}\n");
    out
}

fn construct(name: &str, d: &VariantData, value: &dyn Fn(usize, &str) -> String) -> String {
    match d {
        VariantData::Named(f) => {
            let parts: Vec<String> = f.iter().enumerate().map(|(i, (n, _))| format!("{}: {}", n, value(i, n))).collect();
            format!("{} {{ {} }}", name, parts.join(", "))
        }
        VariantData::Tuple(f) => {
            let parts: Vec<String> = (0..f.len()).map(|i| value(i, &format!("{}", i))).collect();
            format!("{}({})", name, parts.join(", "))
        }
        VariantData::Unit => String::from(name),
    }
}

fn clone(s: &Shape) -> String {
    let mut out = header(s, "::std::clone::Clone", "");
    out.push_str("fn clone(&self) -> Self {\n");
    match &s.body {
        Body::Struct(d) => out.push_str(&construct(s.name, d, &|_, n| format!("::std::clone::Clone::clone(&self.{})", n))),
        Body::Enum(vs) => {
            out.push_str("match self {\n");
            for v in vs.iter() {
                let path = format!("{}::{}", s.name, v.name);
                out.push_str(&format!(
                    "{} => {},\n",
                    variant_pat(s.name, v, "__x"),
                    construct(&path, &v.data, &|i, _| format!("::std::clone::Clone::clone(__x{})", i))
                ));
            }
            if vs.is_empty() {
                out.push_str("_ => loop {}\n");
            }
            out.push_str("}\n");
        }
    }
    out.push_str("}\n}\n");
    out
}

fn partial_eq(s: &Shape) -> String {
    let mut out = header(s, "::std::cmp::PartialEq", "");
    out.push_str("fn eq(&self, other: &Self) -> bool {\n");
    match &s.body {
        Body::Struct(d) => {
            let parts: Vec<String> = field_names(d).iter().map(|n| format!("self.{} == other.{}", n, n)).collect();
            out.push_str(&if parts.is_empty() { String::from("true") } else { parts.join(" && ") });
        }
        Body::Enum(vs) => {
            out.push_str("match (self, other) {\n");
            for v in vs.iter() {
                let n = field_names(&v.data).len();
                let cmp: Vec<String> = (0..n).map(|i| format!("__a{} == __b{}", i, i)).collect();
                out.push_str(&format!(
                    "({}, {}) => {},\n",
                    variant_pat(s.name, v, "__a"),
                    variant_pat(s.name, v, "__b"),
                    if cmp.is_empty() { String::from("true") } else { cmp.join(" && ") }
                ));
            }
            if vs.len() != 1 {
                out.push_str("_ => false,\n");
            }
            out.push_str("}\n");
        }
    }
    out.push_str("}\n}\n");
    out
}

/// `PartialOrd` (`partial`) or `Ord`: lexicographic by variant, then fields.
fn ord(s: &Shape, partial: bool) -> String {
    let (trait_, method, ret, eq, wrap) = if partial {
        (
            "::std::cmp::PartialOrd",
            "partial_cmp",
            "::std::option::Option<::std::cmp::Ordering>",
            "::std::option::Option::Some(::std::cmp::Ordering::Equal)",
            true,
        )
    } else {
        ("::std::cmp::Ord", "cmp", "::std::cmp::Ordering", "::std::cmp::Ordering::Equal", false)
    };
    let step = |a: &str, b: &str| -> String {
        format!(
            "match {}::{}({}, {}) {{ {} => {{}}, __c => return __c }}\n",
            trait_, method, a, b, eq
        )
    };
    let mut out = header(s, trait_, "");
    out.push_str(&format!("fn {}(&self, other: &Self) -> {} {{\n", method, ret));
    match &s.body {
        Body::Struct(d) => {
            for n in field_names(d) {
                out.push_str(&step(&format!("&self.{}", n), &format!("&other.{}", n)));
            }
        }
        Body::Enum(vs) => {
            let index = |who: &str| -> String {
                let mut m = format!("match {} {{\n", who);
                for (i, v) in vs.iter().enumerate() {
                    let pat = match &v.data {
                        VariantData::Named(_) => format!("{}::{} {{ .. }}", s.name, v.name),
                        VariantData::Tuple(_) => format!("{}::{}(..)", s.name, v.name),
                        VariantData::Unit => format!("{}::{}", s.name, v.name),
                    };
                    m.push_str(&format!("{} => {}isize,\n", pat, i));
                }
                if vs.is_empty() {
                    m.push_str("_ => 0isize,\n");
                }
                m + "}"
            };
            out.push_str(&format!("let __ia = {};\nlet __ib = {};\n", index("self"), index("other")));
            out.push_str(&step("&__ia", "&__ib"));
            out.push_str("match (self, other) {\n");
            for v in vs.iter() {
                let n = field_names(&v.data).len();
                let mut body = String::new();
                for i in 0..n {
                    body.push_str(&step(&format!("__a{}", i), &format!("__b{}", i)));
                }
                out.push_str(&format!(
                    "({}, {}) => {{ {} }}\n",
                    variant_pat(s.name, v, "__a"),
                    variant_pat(s.name, v, "__b"),
                    body
                ));
            }
            out.push_str("_ => {}\n}\n");
        }
    }
    out.push_str(eq);
    out.push_str("\n}\n");
    let _ = wrap;
    out.push_str("}\n");
    out
}

fn hash(s: &Shape) -> String {
    let mut out = header(s, "::std::hash::Hash", "");
    out.push_str("fn hash<__H: ::std::hash::Hasher>(&self, state: &mut __H) {\n");
    match &s.body {
        Body::Struct(d) => {
            for n in field_names(d) {
                out.push_str(&format!("::std::hash::Hash::hash(&self.{}, state);\n", n));
            }
        }
        Body::Enum(vs) => {
            out.push_str("match self {\n");
            for (i, v) in vs.iter().enumerate() {
                let n = field_names(&v.data).len();
                let mut body = format!("::std::hash::Hash::hash(&{}isize, state);", i);
                for k in 0..n {
                    body.push_str(&format!(" ::std::hash::Hash::hash(__x{}, state);", k));
                }
                out.push_str(&format!("{} => {{ {} }}\n", variant_pat(s.name, v, "__x"), body));
            }
            if vs.is_empty() {
                out.push_str("_ => {}\n");
            }
            out.push_str("}\n");
        }
    }
    out.push_str("}\n}\n");
    out
}

fn default(s: &Shape, item: &Item) -> Result<String> {
    let mut out = header(s, "::std::default::Default", "");
    out.push_str("fn default() -> Self {\n");
    let dflt = |_: usize, _: &str| String::from("::std::default::Default::default()");
    match &s.body {
        Body::Struct(d) => out.push_str(&construct(s.name, d, &dflt)),
        Body::Enum(vs) => {
            let v = match vs.iter().find(|v| v.default) {
                Some(v) => v,
                None => return err(item.span, String::from("no default declared: make a unit variant default by placing `#[default]` above it")),
            };
            out.push_str(&construct(&format!("{}::{}", s.name, v.name), &v.data, &dflt));
        }
    }
    out.push_str("\n}\n}\n");
    Ok(out)
}
