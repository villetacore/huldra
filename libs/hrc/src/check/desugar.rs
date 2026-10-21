//! Syntax sugar rewritten into plain Rust before checking: `for` loops,
//! formatting macros, assertions, `dbg!` and `matches!`.

use crate::ast::*;
use crate::Span;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub fn e(kind: ExprKind, span: Span) -> Expr {
    Expr { kind, span }
}

/// `::std::a::b`
pub fn std_path(segs: &[&str], span: Span) -> Path {
    let mut s: Vec<PathSeg> = vec![PathSeg {
        name: Rc::from("std"),
        args: None,
    }];
    s.extend(segs.iter().map(|n| PathSeg {
        name: Rc::from(*n),
        args: None,
    }));
    Path {
        segs: s,
        global: true,
        qself: None,
        span,
    }
}

pub fn std_expr(segs: &[&str], span: Span) -> Expr {
    e(ExprKind::Path(std_path(segs, span)), span)
}

pub fn var(name: &str, span: Span) -> Expr {
    e(ExprKind::Path(Path::single(Rc::from(name), span)), span)
}

pub fn call(f: Expr, args: Vec<Expr>, span: Span) -> Expr {
    e(ExprKind::Call(Box::new(f), args), span)
}

pub fn method(recv: Expr, name: &str, args: Vec<Expr>, span: Span) -> Expr {
    e(
        ExprKind::MethodCall {
            recv: Box::new(recv),
            name: Rc::from(name),
            args: None,
            params: args,
        },
        span,
    )
}

pub fn str_lit(s: &str, span: Span) -> Expr {
    e(ExprKind::Lit(Lit::Str(Rc::from(s))), span)
}

pub fn int_lit(v: u128, suffix: &str, span: Span) -> Expr {
    e(ExprKind::Lit(Lit::Int(v, Some(Rc::from(suffix)))), span)
}

pub fn reff(x: Expr, m: bool) -> Expr {
    let span = x.span;
    e(ExprKind::Ref(m, Box::new(x)), span)
}

pub fn deref(x: Expr) -> Expr {
    let span = x.span;
    e(ExprKind::Unary(UnOp::Deref, Box::new(x)), span)
}

pub fn block(stmts: Vec<Stmt>, tail: Option<Expr>, span: Span) -> Expr {
    e(
        ExprKind::Block(
            Rc::new(Block {
                stmts,
                expr: tail.map(Box::new),
                span,
            }),
            None,
        ),
        span,
    )
}

pub fn semi(x: Expr) -> Stmt {
    Stmt::Expr(x, true)
}

pub fn let_(name: &str, mutable: bool, init: Expr) -> Stmt {
    let span = init.span;
    Stmt::Let {
        pat: ident_pat(name, mutable, span),
        ty: None,
        init: Some(init),
        else_: None,
        span,
    }
}

pub fn ident_pat(name: &str, mutable: bool, span: Span) -> Pat {
    Pat {
        kind: PatKind::Ident {
            name: Rc::from(name),
            by_ref: false,
            mutable,
            sub: None,
        },
        span,
    }
}

pub fn wild(span: Span) -> Pat {
    Pat { kind: PatKind::Wild, span }
}

pub fn arm(pat: Pat, guard: Option<Expr>, body: Expr) -> Arm {
    let span = pat.span;
    Arm { pat, guard, body, span }
}

pub fn match_(scrut: Expr, arms: Vec<Arm>, span: Span) -> Expr {
    e(ExprKind::Match(Box::new(scrut), arms), span)
}

pub fn some(x: Expr) -> Expr {
    let span = x.span;
    call(std_expr(&["option", "Option", "Some"], span), vec![x], span)
}

pub fn none(span: Span) -> Expr {
    std_expr(&["option", "Option", "None"], span)
}

pub fn location(span: Span) -> Expr {
    e(ExprKind::Location, span)
}

/// `for pat in iter body` as `loop`/`match` on `IntoIterator`.
pub fn for_loop(pat: &Pat, iter: &Expr, body: &Rc<Block>, label: &Option<Name>, span: Span) -> Expr {
    let it = "__hrc_iter";
    let into = call(std_expr(&["iter", "IntoIterator", "into_iter"], span), vec![iter.clone()], span);
    let next = call(std_expr(&["iter", "Iterator", "next"], span), vec![reff(var(it, span), true)], span);
    let some_pat = Pat {
        kind: PatKind::TupleStruct(std_path(&["option", "Option", "Some"], span), vec![pat.clone()]),
        span: pat.span,
    };
    let none_pat = Pat {
        kind: PatKind::Path(std_path(&["option", "Option", "None"], span)),
        span,
    };
    let body_expr = e(ExprKind::Block(body.clone(), None), body.span);
    let m = match_(
        next,
        vec![arm(some_pat, None, body_expr), arm(none_pat, None, e(ExprKind::Break(None, None), span))],
        span,
    );
    let lp = e(
        ExprKind::Loop(
            Rc::new(Block {
                stmts: vec![Stmt::Expr(m, false)],
                expr: None,
                span,
            }),
            label.clone(),
        ),
        span,
    );
    block(vec![let_(it, true, into)], Some(lp), span)
}

fn fmt_trait(t: FmtTrait) -> &'static str {
    match t {
        FmtTrait::Display => "Display",
        FmtTrait::Debug => "Debug",
        FmtTrait::LowerHex => "LowerHex",
        FmtTrait::UpperHex => "UpperHex",
        FmtTrait::Octal => "Octal",
        FmtTrait::Binary => "Binary",
        FmtTrait::LowerExp => "LowerExp",
        FmtTrait::UpperExp => "UpperExp",
    }
}

fn count(c: &Option<FmtCount>, span: Span) -> Expr {
    match c {
        None => none(span),
        Some(FmtCount::Is(n)) => some(int_lit(*n as u128, "usize", span)),
        Some(FmtCount::Arg(k)) => some(deref(var(&format!("__hrc_a{}", k), span))),
    }
}

/// The literal text of a format without arguments.
pub fn literal_only(f: &Format) -> Option<String> {
    let mut s = String::new();
    for p in &f.pieces {
        match p {
            FmtPiece::Lit(l) => s.push_str(l),
            FmtPiece::Arg(_) => return None,
        }
    }
    Some(s)
}

/// An expression producing the formatted `String`.
pub fn format_string(f: &Format) -> Expr {
    let span = f.span;
    if let Some(s) = literal_only(f) {
        return call(std_expr(&["string", "String", "from"], span), vec![str_lit(&s, span)], span);
    }
    let fv = "__hrc_f";
    let mut stmts = vec![let_(fv, true, call(std_expr(&["fmt", "Formatter", "new"], span), Vec::new(), span))];
    let mut dirty = false;
    for p in &f.pieces {
        match p {
            FmtPiece::Lit(l) => stmts.push(semi(method(var(fv, span), "write_str", vec![str_lit(l, span)], span))),
            FmtPiece::Arg(a) => {
                let default = a.fill == ' ' && a.align == 0 && !a.plus && !a.alternate && !a.zero && a.width.is_none() && a.precision.is_none();
                if !default {
                    let flags = (a.plus as u128) | ((a.alternate as u128) << 1) | ((a.zero as u128) << 2);
                    stmts.push(semi(method(
                        var(fv, span),
                        "set_spec",
                        vec![
                            e(ExprKind::Lit(Lit::Char(a.fill)), span),
                            int_lit(a.align as u128, "u8", span),
                            int_lit(flags, "u32", span),
                            count(&a.width, span),
                            count(&a.precision, span),
                        ],
                        span,
                    )));
                    dirty = true;
                } else if dirty {
                    stmts.push(semi(method(var(fv, span), "clear_spec", Vec::new(), span)));
                    dirty = false;
                }
                let f = std_expr(&["fmt", fmt_trait(a.trait_), "fmt"], span);
                stmts.push(semi(call(
                    f,
                    vec![var(&format!("__hrc_a{}", a.arg), span), reff(var(fv, span), true)],
                    span,
                )));
            }
        }
    }
    let body = block(stmts, Some(method(var(fv, span), "into_string", Vec::new(), span)), span);
    if f.args.is_empty() {
        return body;
    }
    let refs: Vec<Expr> = f.args.iter().map(|a| reff(a.clone(), false)).collect();
    let pats: Vec<Pat> = (0..f.args.len()).map(|k| ident_pat(&format!("__hrc_a{}", k), false, span)).collect();
    let pat = Pat {
        kind: PatKind::Tuple(pats),
        span,
    };
    match_(e(ExprKind::Tuple(refs), span), vec![arm(pat, None, body)], span)
}

pub fn macro_expr(kind: MacroKind, f: &Format) -> Expr {
    let span = f.span;
    match kind {
        MacroKind::Format => format_string(f),
        MacroKind::Args => call(std_expr(&["fmt", "Arguments", "new"], span), vec![format_string(f)], span),
        MacroKind::Print | MacroKind::Println | MacroKind::Eprint | MacroKind::Eprintln => {
            let name = if matches!(kind, MacroKind::Print | MacroKind::Println) { "_print" } else { "_eprint" };
            let arg = match literal_only(f) {
                Some(s) => str_lit(&s, span),
                None => reff(format_string(f), false),
            };
            call(std_expr(&["io", name], span), vec![arg], span)
        }
        MacroKind::Panic => panic_expr(f),
    }
}

pub fn panic_expr(f: &Format) -> Expr {
    let span = f.span;
    match literal_only(f) {
        Some(s) => call(std_expr(&["rt", "begin_panic_str"], span), vec![str_lit(&s, span), location(span)], span),
        None => call(std_expr(&["rt", "begin_panic"], span), vec![format_string(f), location(span)], span),
    }
}

pub fn write_expr(dst: &Expr, f: &Format) -> Expr {
    let span = f.span;
    let args = call(std_expr(&["fmt", "Arguments", "new"], span), vec![format_string(f)], span);
    method(dst.clone(), "write_fmt", vec![args], span)
}

pub fn assert_expr(cond: &Expr, msg: &Option<Rc<Format>>, span: Span) -> Expr {
    let not = e(
        ExprKind::Unary(UnOp::Not, Box::new(e(ExprKind::Paren(Box::new(cond.clone())), span))),
        span,
    );
    let p = match msg {
        Some(m) => panic_expr(m),
        None => panic_expr(&Format {
            pieces: vec![FmtPiece::Lit(String::from("assertion failed"))],
            args: Vec::new(),
            span,
        }),
    };
    e(
        ExprKind::If(
            Box::new(not),
            Rc::new(Block {
                stmts: vec![semi(p)],
                expr: None,
                span,
            }),
            None,
        ),
        span,
    )
}

pub fn assert_cmp_expr(eq: bool, a: &Expr, b: &Expr, msg: &Option<Rc<Format>>, span: Span) -> Expr {
    let l = var("__hrc_l", span);
    let r = var("__hrc_r", span);
    let cmp = e(ExprKind::Binary(BinOp::Eq, Box::new(deref(l.clone())), Box::new(deref(r.clone()))), span);
    let cond = if eq {
        e(ExprKind::Unary(UnOp::Not, Box::new(e(ExprKind::Paren(Box::new(cmp)), span))), span)
    } else {
        cmp
    };
    let m = match msg {
        Some(f) => some(format_string(f)),
        None => none(span),
    };
    let fail = call(
        std_expr(&["rt", "assert_failed"], span),
        vec![e(ExprKind::Lit(Lit::Bool(eq)), span), l, r, m, location(span)],
        span,
    );
    let iff = e(
        ExprKind::If(
            Box::new(cond),
            Rc::new(Block {
                stmts: vec![semi(fail)],
                expr: None,
                span,
            }),
            None,
        ),
        span,
    );
    let pat = Pat {
        kind: PatKind::Tuple(vec![ident_pat("__hrc_l", false, span), ident_pat("__hrc_r", false, span)]),
        span,
    };
    match_(
        e(ExprKind::Tuple(vec![reff(a.clone(), false), reff(b.clone(), false)]), span),
        vec![arm(pat, None, block(vec![semi(iff)], None, span))],
        span,
    )
}

pub fn matches_expr(x: &Expr, pat: &Pat, guard: &Option<Box<Expr>>, span: Span) -> Expr {
    let t = e(ExprKind::Lit(Lit::Bool(true)), span);
    let f = e(ExprKind::Lit(Lit::Bool(false)), span);
    match_(
        x.clone(),
        vec![arm(pat.clone(), guard.as_ref().map(|g| (**g).clone()), t), arm(wild(span), None, f)],
        span,
    )
}

pub fn dbg_expr(x: &Expr, text: &str, span: Span) -> Expr {
    let tmp = "__hrc_dbg";
    let print = call(
        std_expr(&["rt", "dbg_print"], span),
        vec![location(span), str_lit(text, span), reff(var(tmp, span), false)],
        span,
    );
    match_(
        x.clone(),
        vec![arm(ident_pat(tmp, false, span), None, block(vec![semi(print)], Some(var(tmp, span)), span))],
        span,
    )
}
