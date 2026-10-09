//! Lays a parsed document out as lines of styled text for a given width:
//! what both browsers draw. Links and form fields keep their index so a
//! browser can highlight, follow and fill them.

use crate::html::{Element, Node};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const BOLD: u8 = 1;
pub const ITALIC: u8 = 2;
pub const CODE: u8 = 4;
pub const LINK: u8 = 8;
pub const HEADING: u8 = 16;
pub const FIELD: u8 = 32;
pub const DIM: u8 = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: u8,
    /// Index into `Page::links`.
    pub link: Option<usize>,
    /// Index into `Page::fields`.
    pub field: Option<usize>,
}

pub type Line = Vec<Span>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Password,
    Hidden,
    Checkbox,
    Submit,
    Select(Vec<(String, String)>),
    TextArea,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub kind: FieldKind,
    pub name: String,
    pub value: String,
    pub checked: bool,
    pub form: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pub action: String,
    pub method: String,
}

#[derive(Clone, Debug, Default)]
pub struct Page {
    pub title: String,
    pub lines: Vec<Line>,
    /// Link targets as written (resolve against the page URL).
    pub links: Vec<String>,
    pub fields: Vec<Field>,
    pub forms: Vec<Form>,
    /// Lines where elements with an `id` start (for #fragments).
    pub anchors: Vec<(String, usize)>,
}

impl Page {
    /// The plain text of a line.
    pub fn line_text(&self, i: usize) -> String {
        self.lines[i].iter().map(|s| s.text.as_str()).collect()
    }

    /// Lines where link `n` appears.
    pub fn link_line(&self, n: usize) -> Option<usize> {
        self.lines.iter().position(|l| l.iter().any(|s| s.link == Some(n)))
    }

    pub fn field_line(&self, n: usize) -> Option<usize> {
        self.lines.iter().position(|l| l.iter().any(|s| s.field == Some(n)))
    }

    /// The text shown for a field.
    pub fn field_label(f: &Field) -> String {
        match &f.kind {
            FieldKind::Text | FieldKind::TextArea => {
                let mut v: String = f.value.chars().take(30).collect();
                let pad = 16usize.saturating_sub(v.chars().count());
                v.extend(core::iter::repeat_n('_', pad));
                format!("[{}]", v)
            }
            FieldKind::Password => format!("[{:_<16}]", "*".repeat(f.value.chars().count().min(16))),
            FieldKind::Checkbox => if f.checked { "[x]".into() } else { "[ ]".into() },
            FieldKind::Submit => format!("[ {} ]", if f.value.is_empty() { "Submit" } else { &f.value }),
            FieldKind::Select(opts) => {
                let shown = opts.iter().find(|(v, _)| *v == f.value).map(|(_, l)| l.as_str()).unwrap_or("");
                format!("[{} ▾]", shown)
            }
            FieldKind::Hidden => String::new(),
        }
    }

    /// The submission for form `form` (submitted with field `via`):
    /// (method, action, url-encoded data).
    pub fn submission(&self, form: usize, via: Option<usize>) -> (String, String, String) {
        let mut pairs = Vec::new();
        for (i, f) in self.fields.iter().enumerate() {
            if f.form != Some(form) || f.name.is_empty() {
                continue;
            }
            match f.kind {
                FieldKind::Submit if via != Some(i) => continue,
                FieldKind::Checkbox if !f.checked => continue,
                _ => {}
            }
            let value = if f.kind == FieldKind::Checkbox && f.value.is_empty() { "on".to_string() } else { f.value.clone() };
            pairs.push(format!("{}={}", encode(&f.name), encode(&value)));
        }
        let fm = &self.forms[form];
        (fm.method.clone(), fm.action.clone(), pairs.join("&"))
    }
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// One word or fixed piece of a paragraph.
struct Atom {
    text: String,
    style: u8,
    link: Option<usize>,
    field: Option<usize>,
    /// A space separates it from the previous atom.
    space: bool,
}

struct Ctx {
    width: usize,
    page: Page,
    atoms: Vec<Atom>,
    /// Pending space between inline elements.
    space: bool,
    indent: usize,
    /// Prefix for the first line of the next paragraph (list bullets).
    bullet: Option<String>,
    style: u8,
    link: Option<usize>,
    form: Option<usize>,
    pre: bool,
    /// The last thing written was a blank line.
    blank: bool,
}

impl Ctx {
    fn push_text(&mut self, t: &str) {
        if self.pre {
            // Preformatted: every line as it is.
            let mut first = true;
            for l in t.split('\n') {
                if !first {
                    self.flush_line();
                }
                first = false;
                if !l.is_empty() {
                    self.atoms.push(Atom { text: l.replace('\t', "    "), style: self.style | CODE, link: self.link, field: None, space: false });
                }
            }
            return;
        }
        for (i, w) in t.split(|c: char| c.is_whitespace() && c != '\u{a0}').enumerate() {
            if i > 0 {
                self.space = true;
            }
            if w.is_empty() {
                continue;
            }
            let space = self.space && !self.atoms.is_empty();
            self.atoms.push(Atom { text: w.replace('\u{a0}', " "), style: self.style, link: self.link, field: None, space });
            self.space = false;
        }
    }

    fn push_atom(&mut self, text: String, style: u8, field: Option<usize>) {
        // Adjacent form controls get a gap, or they run together as text.
        let after_field = field.is_some() && self.atoms.last().is_some_and(|a| a.field.is_some());
        let space = (self.space || after_field) && !self.atoms.is_empty();
        self.atoms.push(Atom { text, style, link: self.link, field, space });
        self.space = false;
    }

    /// Ends a preformatted line.
    fn flush_line(&mut self) {
        let lead: String = core::iter::repeat_n(' ', self.indent).collect();
        let mut line: Line = alloc::vec![Span { text: lead, style: 0, link: None, field: None }];
        for a in self.atoms.drain(..) {
            line.push(Span { text: a.text, style: a.style, link: a.link, field: a.field });
        }
        self.page.lines.push(line);
        self.blank = false;
    }

    /// Wraps the pending atoms into lines.
    fn flush(&mut self) {
        if self.pre {
            if !self.atoms.is_empty() {
                self.flush_line();
            }
            return;
        }
        self.space = false;
        if self.atoms.is_empty() {
            return;
        }
        let first_prefix = self.bullet.take().unwrap_or_default();
        let lead: String = core::iter::repeat_n(' ', self.indent).collect();
        let hang: String = core::iter::repeat_n(' ', self.indent + first_prefix.chars().count()).collect();
        let avail = self.width.saturating_sub(hang.chars().count()).max(10);
        let mut line: Line = alloc::vec![Span { text: format!("{}{}", lead, first_prefix), style: if first_prefix.is_empty() { 0 } else { BOLD }, link: None, field: None }];
        let mut col = 0;
        for a in core::mem::take(&mut self.atoms) {
            let len = a.text.chars().count();
            if col > 0 && col + a.space as usize + len > avail {
                self.page.lines.push(core::mem::take(&mut line));
                line.push(Span { text: hang.clone(), style: 0, link: None, field: None });
                col = 0;
            } else if a.space && col > 0 {
                // The space belongs to the link if both sides are the same link.
                let link = line.last().and_then(|s| s.link).filter(|l| Some(*l) == a.link);
                line.push(Span { text: " ".into(), style: if link.is_some() { a.style } else { 0 }, link, field: None });
                col += 1;
            }
            // Very long words (URLs) are cut.
            let mut text = a.text;
            while text.chars().count() > avail {
                let head: String = text.chars().take(avail - col.min(avail)).collect();
                let rest: String = text.chars().skip(head.chars().count()).collect();
                if head.is_empty() {
                    self.page.lines.push(core::mem::take(&mut line));
                    line.push(Span { text: hang.clone(), style: 0, link: None, field: None });
                    col = 0;
                    continue;
                }
                line.push(Span { text: head, style: a.style, link: a.link, field: a.field });
                self.page.lines.push(core::mem::take(&mut line));
                line.push(Span { text: hang.clone(), style: 0, link: None, field: None });
                col = 0;
                text = rest;
            }
            col += text.chars().count();
            line.push(Span { text, style: a.style, link: a.link, field: a.field });
        }
        self.page.lines.push(line);
        self.blank = false;
    }

    fn blank_line(&mut self) {
        self.flush();
        if !self.blank && !self.page.lines.is_empty() {
            self.page.lines.push(Vec::new());
            self.blank = true;
        }
    }
}

const BLOCKS: &[&str] = &[
    "address", "article", "aside", "blockquote", "center", "dd", "details", "div", "dl", "dt", "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "li", "main", "nav",
    "ol", "p", "pre", "section", "summary", "table", "tr", "ul",
];

const SKIP: &[&str] = &["head", "script", "style", "template", "svg", "math", "canvas", "video", "audio", "object", "map", "datalist"];

fn walk(ctx: &mut Ctx, e: &Element) {
    let name = e.name.as_str();
    if SKIP.contains(&name) || e.attr("hidden").is_some() || e.attr("aria-hidden") == Some("true") && name != "body" {
        return;
    }
    if let Some(id) = e.attr("id").or_else(|| if name == "a" { e.attr("name") } else { None }) {
        let line = ctx.page.lines.len() + !ctx.atoms.is_empty() as usize;
        ctx.page.anchors.push((id.to_string(), line));
    }
    let is_block = BLOCKS.contains(&name);
    let saved = (ctx.style, ctx.link, ctx.indent, ctx.pre);
    match name {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            ctx.blank_line();
            ctx.style |= HEADING | BOLD;
        }
        "p" | "table" | "pre" | "blockquote" | "dl" | "figure" | "details" => ctx.blank_line(),
        "ul" | "ol" => {
            ctx.flush();
            if ctx.indent == 0 {
                ctx.blank_line();
            }
        }
        "br" => {
            ctx.flush();
            return;
        }
        "hr" => {
            ctx.blank_line();
            let rule: String = core::iter::repeat_n('─', ctx.width.min(72)).collect();
            ctx.page.lines.push(alloc::vec![Span { text: rule, style: DIM, link: None, field: None }]);
            ctx.blank = false;
            ctx.blank_line();
            return;
        }
        "img" => {
            if let Some(alt) = e.attr("alt").filter(|a| !a.trim().is_empty()) {
                ctx.push_atom(format!("[{}]", alt.trim()), ctx.style | ITALIC | DIM, None);
            }
            return;
        }
        "iframe" => {
            if let Some(src) = e.attr("src") {
                ctx.page.links.push(src.to_string());
                ctx.link = Some(ctx.page.links.len() - 1);
                ctx.push_atom("[frame]".into(), LINK | DIM, None);
                ctx.link = saved.1;
            }
            return;
        }
        _ => {}
    }
    if is_block {
        ctx.flush();
    }
    match name {
        "b" | "strong" | "th" | "dt" | "summary" => ctx.style |= BOLD,
        "i" | "em" | "cite" | "var" | "dfn" => ctx.style |= ITALIC,
        "code" | "kbd" | "samp" | "tt" => ctx.style |= CODE,
        "pre" => ctx.pre = true,
        "blockquote" | "dd" => ctx.indent += 4,
        "a" => {
            if let Some(href) = e.attr("href").filter(|h| !h.starts_with("javascript:")) {
                ctx.page.links.push(href.to_string());
                ctx.link = Some(ctx.page.links.len() - 1);
                ctx.style |= LINK;
            }
        }
        "form" => {
            ctx.page.forms.push(Form {
                action: e.attr("action").unwrap_or("").to_string(),
                method: e.attr("method").unwrap_or("get").to_ascii_lowercase(),
            });
            ctx.form = Some(ctx.page.forms.len() - 1);
        }
        _ => {}
    }
    match name {
        "ul" | "ol" => {
            let mut n = e.attr("start").and_then(|s| s.parse().ok()).unwrap_or(1u32);
            ctx.indent += 2;
            for c in &e.children {
                match c {
                    Node::Element(li) if li.name == "li" => {
                        ctx.flush();
                        ctx.bullet = Some(if name == "ol" { format!("{}. ", n) } else { String::from(if ctx.indent > 2 { "◦ " } else { "• " }) });
                        n += 1;
                        let inner_indent = ctx.indent;
                        walk_children(ctx, li);
                        ctx.flush();
                        ctx.indent = inner_indent;
                        ctx.bullet = None;
                    }
                    other => walk_node(ctx, other),
                }
            }
        }
        "table" => table(ctx, e),
        "input" => input(ctx, e),
        "button" => {
            let label = e.text();
            ctx.page.fields.push(Field { kind: FieldKind::Submit, name: e.attr("name").unwrap_or("").into(), value: if label.is_empty() { "Submit".into() } else { label }, checked: false, form: ctx.form });
            let i = ctx.page.fields.len() - 1;
            let text = Page::field_label(&ctx.page.fields[i]);
            ctx.push_atom(text, FIELD, Some(i));
        }
        "select" => {
            let mut opts = Vec::new();
            let mut selected = None;
            fn options(e: &Element, opts: &mut Vec<(String, String)>, selected: &mut Option<String>) {
                for c in &e.children {
                    if let Node::Element(o) = c {
                        if o.name == "option" {
                            let label = o.text();
                            let value = o.attr("value").map(String::from).unwrap_or_else(|| label.clone());
                            if o.attr("selected").is_some() {
                                *selected = Some(value.clone());
                            }
                            opts.push((value, label));
                        } else {
                            options(o, opts, selected);
                        }
                    }
                }
            }
            options(e, &mut opts, &mut selected);
            let value = selected.or_else(|| opts.first().map(|o| o.0.clone())).unwrap_or_default();
            ctx.page.fields.push(Field { kind: FieldKind::Select(opts), name: e.attr("name").unwrap_or("").into(), value, checked: false, form: ctx.form });
            let i = ctx.page.fields.len() - 1;
            let text = Page::field_label(&ctx.page.fields[i]);
            ctx.push_atom(text, FIELD, Some(i));
        }
        "textarea" => {
            ctx.page.fields.push(Field { kind: FieldKind::TextArea, name: e.attr("name").unwrap_or("").into(), value: e.text(), checked: false, form: ctx.form });
            let i = ctx.page.fields.len() - 1;
            let text = Page::field_label(&ctx.page.fields[i]);
            ctx.push_atom(text, FIELD, Some(i));
        }
        "title" => ctx.page.title = e.text(),
        _ => walk_children(ctx, e),
    }
    if name == "form" {
        ctx.form = None;
    }
    if is_block {
        ctx.flush();
    }
    match name {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "table" | "pre" | "blockquote" | "dl" | "figure" | "details" => ctx.blank_line(),
        "ul" | "ol" if saved.2 == 0 => ctx.blank_line(),
        _ => {}
    }
    (ctx.style, ctx.link, ctx.indent, ctx.pre) = saved;
}

fn walk_node(ctx: &mut Ctx, n: &Node) {
    match n {
        Node::Text(t) => ctx.push_text(t),
        Node::Element(e) => walk(ctx, e),
    }
}

fn walk_children(ctx: &mut Ctx, e: &Element) {
    for c in &e.children {
        walk_node(ctx, c);
    }
}

fn input(ctx: &mut Ctx, e: &Element) {
    let ty = e.attr("type").unwrap_or("text").to_ascii_lowercase();
    let kind = match ty.as_str() {
        "hidden" => FieldKind::Hidden,
        "password" => FieldKind::Password,
        "checkbox" | "radio" => FieldKind::Checkbox,
        "submit" | "button" | "image" => FieldKind::Submit,
        "reset" => return,
        _ => FieldKind::Text,
    };
    let value = e.attr("value").unwrap_or(if kind == FieldKind::Submit { "Submit" } else { "" }).to_string();
    ctx.page.fields.push(Field { kind: kind.clone(), name: e.attr("name").unwrap_or("").into(), value, checked: e.attr("checked").is_some(), form: ctx.form });
    if kind == FieldKind::Hidden {
        return;
    }
    let i = ctx.page.fields.len() - 1;
    let text = Page::field_label(&ctx.page.fields[i]);
    ctx.push_atom(text, FIELD, Some(i));
}

/// Lays a table cell out at `width` as its own small page (links, fields
/// and forms go into the page's lists as usual).
fn cell_lines(ctx: &mut Ctx, cell: &Element, width: usize) -> Vec<Line> {
    let mut sub = Ctx {
        width,
        page: Page { links: core::mem::take(&mut ctx.page.links), fields: core::mem::take(&mut ctx.page.fields), forms: core::mem::take(&mut ctx.page.forms), ..Page::default() },
        atoms: Vec::new(),
        space: false,
        indent: 0,
        bullet: None,
        style: ctx.style | if cell.name == "th" { BOLD } else { 0 },
        link: ctx.link,
        form: ctx.form,
        pre: false,
        blank: true,
    };
    walk_children(&mut sub, cell);
    sub.flush();
    ctx.page.links = core::mem::take(&mut sub.page.links);
    ctx.page.fields = core::mem::take(&mut sub.page.fields);
    ctx.page.forms = core::mem::take(&mut sub.page.forms);
    let mut lines: Vec<Line> = sub.page.lines;
    // Drop blank lines at the edges, and the empty lead span of each line.
    while lines.first().is_some_and(|l| line_width(l) == 0) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| line_width(l) == 0) {
        lines.pop();
    }
    lines
}

fn line_width(l: &Line) -> usize {
    l.iter().map(|s| s.text.chars().count()).sum()
}

fn pad(n: usize) -> Span {
    Span { text: core::iter::repeat_n(' ', n).collect(), style: 0, link: None, field: None }
}

/// Tables. Columns side by side when they fit; when too wide, the last
/// column wraps within what is left; when even that is too narrow, one
/// cell after another. Empty columns (spacers, icons) are dropped.
fn table(ctx: &mut Ctx, e: &Element) {
    let mut rows: Vec<&Element> = Vec::new();
    fn collect<'a>(e: &'a Element, rows: &mut Vec<&'a Element>) {
        for c in &e.children {
            if let Node::Element(x) = c {
                match x.name.as_str() {
                    "tr" => rows.push(x),
                    "thead" | "tbody" | "tfoot" => collect(x, rows),
                    _ => {}
                }
            }
        }
    }
    collect(e, &mut rows);
    if let Some(Node::Element(cap)) = e.children.iter().find(|c| matches!(c, Node::Element(x) if x.name == "caption")) {
        ctx.style |= BOLD;
        walk_children(ctx, cap);
        ctx.style &= !BOLD;
        ctx.flush();
    }
    // Each row: (first column, columns spanned, cell).
    let grid: Vec<Vec<(usize, usize, &Element)>> = rows
        .iter()
        .map(|r| {
            let mut col = 0;
            let mut out = Vec::new();
            for c in &r.children {
                if let Node::Element(x) = c {
                    if x.name == "td" || x.name == "th" {
                        let span = x.attr("colspan").and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, 50);
                        out.push((col, span, x));
                        col += span;
                    }
                }
            }
            out
        })
        .collect();
    let ncols = grid.iter().map(|r| r.last().map_or(0, |(c, s, _)| c + s)).max().unwrap_or(0);
    // Measure natural widths (no wrapping), then forget what was recorded.
    let saved = (ctx.page.links.len(), ctx.page.fields.len(), ctx.page.forms.len());
    let mut widths = alloc::vec![0usize; ncols];
    let mut spanning: Vec<(usize, usize, usize)> = Vec::new();
    for r in &grid {
        for &(c, span, cell) in r {
            let w = cell_lines(ctx, cell, 10_000).iter().map(line_width).max().unwrap_or(0);
            if span == 1 {
                widths[c] = widths[c].max(w);
            } else if w > 0 {
                spanning.push((c, span, w));
            }
        }
    }
    ctx.page.links.truncate(saved.0);
    ctx.page.fields.truncate(saved.1);
    ctx.page.forms.truncate(saved.2);
    // A spanning cell wider than its columns widens the last non-empty one.
    for (c, span, w) in spanning {
        let range = c..(c + span).min(ncols);
        let have: usize = range.clone().map(|i| widths[i]).sum::<usize>() + 3 * range.clone().filter(|&i| widths[i] > 0).count().saturating_sub(1);
        if w > have {
            let target = range.clone().rev().find(|&i| widths[i] > 0).unwrap_or(range.end - 1);
            widths[target] += w - have;
        }
    }
    let cols: Vec<usize> = (0..ncols).filter(|&i| widths[i] > 0).collect();
    let sep = 3;
    let avail = ctx.width.saturating_sub(ctx.indent);
    let total: usize = cols.iter().map(|&i| widths[i]).sum::<usize>() + sep * cols.len().saturating_sub(1);
    let last = cols.last().copied();
    let fixed: usize = total - last.map_or(0, |l| widths[l]);
    let mut col_width = widths.clone();
    let side_by_side = if total <= avail {
        true
    } else if let Some(l) = last.filter(|_| fixed + 20 <= avail) {
        col_width[l] = avail - fixed;
        true
    } else {
        false
    };
    for r in &grid {
        if side_by_side {
            // Segments of visible columns, each covered by one cell (or none).
            let mut segments: Vec<(Vec<usize>, Option<&Element>)> = Vec::new();
            for &i in &cols {
                let cell = r.iter().find(|(c, s, _)| *c <= i && i < c + s).map(|(_, _, e)| *e);
                match segments.last_mut() {
                    Some((group, Some(prev))) if cell.is_some_and(|c| core::ptr::eq(c, *prev)) => group.push(i),
                    _ => segments.push((alloc::vec![i], cell)),
                }
            }
            let mut cells: Vec<(usize, Vec<Line>)> = Vec::new();
            for (group, cell) in &segments {
                let w = group.iter().map(|&i| col_width[i]).sum::<usize>() + sep * (group.len() - 1);
                let lines = match cell {
                    Some(c) => cell_lines(ctx, c, w),
                    None => Vec::new(),
                };
                cells.push((w, lines));
            }
            let height = cells.iter().map(|(_, l)| l.len()).max().unwrap_or(0).max(1);
            for row_line in 0..height {
                let mut line: Line = alloc::vec![pad(ctx.indent)];
                for (k, (w, lines)) in cells.iter().enumerate() {
                    if k > 0 {
                        line.push(Span { text: " │ ".into(), style: DIM, link: None, field: None });
                    }
                    let content = lines.get(row_line).cloned().unwrap_or_default();
                    let cw = line_width(&content);
                    line.extend(content);
                    if k + 1 < cells.len() {
                        line.push(pad(w.saturating_sub(cw)));
                    }
                }
                // No trailing padding or separators on short lines.
                while line.len() > 1 && line.last().is_some_and(|s| s.text.trim().is_empty() || s.text == " │ ") {
                    line.pop();
                }
                ctx.page.lines.push(line);
            }
        } else {
            for &(_, _, cell) in r {
                for l in cell_lines(ctx, cell, avail) {
                    let mut line: Line = alloc::vec![pad(ctx.indent)];
                    line.extend(l);
                    ctx.page.lines.push(line);
                }
            }
            ctx.page.lines.push(Vec::new());
        }
    }
    ctx.blank = false;
}

/// Lays out a parsed document.
pub fn layout(doc: &Element, width: usize) -> Page {
    let mut ctx = Ctx { width: width.max(20), page: Page::default(), atoms: Vec::new(), space: false, indent: 0, bullet: None, style: 0, link: None, form: None, pre: false, blank: true };
    if let Some(t) = doc.find("title") {
        ctx.page.title = t.text();
    }
    let body = doc.find("body").unwrap_or(doc);
    walk(&mut ctx, body);
    ctx.flush();
    while ctx.page.lines.last().is_some_and(|l| l.is_empty()) {
        ctx.page.lines.pop();
    }
    ctx.page
}

/// Plain text as a page (for text/plain and friends).
pub fn plain(text: &str, width: usize) -> Page {
    let mut page = Page::default();
    for l in text.lines() {
        let l = l.replace('\t', "    ");
        let chars: Vec<char> = l.chars().collect();
        if chars.is_empty() {
            page.lines.push(Vec::new());
        }
        for chunk in chars.chunks(width.max(20)) {
            page.lines.push(alloc::vec![Span { text: chunk.iter().collect(), style: 0, link: None, field: None }]);
        }
    }
    page
}
