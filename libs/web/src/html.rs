//! A forgiving HTML parser: tokens to a tree, the way browsers cope with
//! real pages (unclosed `<p>` and `<li>`, stray end tags, raw text in
//! `<script>` and `<style>`, character references).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// The first descendant element named `name`.
    pub fn find(&self, name: &str) -> Option<&Element> {
        for c in &self.children {
            if let Node::Element(e) = c {
                if e.name == name {
                    return Some(e);
                }
                if let Some(f) = e.find(name) {
                    return Some(f);
                }
            }
        }
        None
    }

    /// All text inside, with whitespace collapsed.
    pub fn text(&self) -> String {
        let mut s = String::new();
        fn rec(e: &Element, s: &mut String) {
            for c in &e.children {
                match c {
                    Node::Text(t) => s.push_str(t),
                    Node::Element(e) => rec(e, s),
                }
            }
        }
        rec(self, &mut s);
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];
const RAW: &[&str] = &["script", "style", "textarea", "title"];

/// Elements that end an open `<p>` when they start.
const CLOSES_P: &[&str] = &[
    "address", "article", "aside", "blockquote", "div", "dl", "fieldset", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "main", "nav", "ol", "p", "pre", "section", "table", "ul",
];

/// Named character references beyond the basic five.
fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "laquo" => '«',
        "raquo" => '»',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "bull" => '•',
        "middot" => '·',
        "deg" => '°',
        "times" => '×',
        "divide" => '÷',
        "euro" => '€',
        "pound" => '£',
        "sect" => '§',
        "para" => '¶',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "hearts" => '♥',
        "check" => '✓',
        _ => return None,
    })
}

/// Decodes `&amp;`, `&#169;`, `&#xA9;` and friends.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '#')).map(|e| e + 1).unwrap_or(rest.len());
        let name = &rest[1..end];
        let decoded = if let Some(num) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
            u32::from_str_radix(num, 16).ok().and_then(char::from_u32)
        } else if let Some(num) = name.strip_prefix('#') {
            num.parse().ok().and_then(char::from_u32)
        } else {
            named_entity(name)
        };
        match decoded {
            Some(c) if !name.is_empty() => {
                out.push(c);
                rest = &rest[end..];
                if rest.starts_with(';') {
                    rest = &rest[1..];
                }
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        if start == i {
            break;
        }
        let name = s[start..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = decode_entities(&s[vs..i]);
                i += 1;
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = decode_entities(&s[vs..i]);
            }
        }
        out.push((name, value));
    }
    out
}

/// Parses a document; the result is a synthetic root element.
pub fn parse(src: &str) -> Element {
    let mut stack: Vec<Element> = alloc::vec![Element { name: "#root".into(), ..Default::default() }];
    let close = |stack: &mut Vec<Element>, name: &str| {
        // Pop up to and including the nearest `name`, if it is open.
        if let Some(pos) = stack.iter().rposition(|e| e.name == name) {
            if pos == 0 {
                return;
            }
            while stack.len() > pos {
                let e = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(Node::Element(e));
            }
        }
    };
    let text = |stack: &mut Vec<Element>, t: &str| {
        if !t.is_empty() {
            stack.last_mut().unwrap().children.push(Node::Text(decode_entities(t)));
        }
    };
    let mut rest = src;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            text(&mut stack, rest);
            break;
        };
        text(&mut stack, &rest[..lt]);
        rest = &rest[lt..];
        if rest.starts_with("<!--") {
            rest = rest.find("-->").map_or("", |e| &rest[e + 3..]);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            rest = rest.find('>').map_or("", |e| &rest[e + 1..]);
            continue;
        }
        let Some(gt) = rest.find('>') else {
            text(&mut stack, rest);
            break;
        };
        let inner = &rest[1..gt];
        rest = &rest[gt + 1..];
        if let Some(name) = inner.strip_prefix('/') {
            let name = name.trim().to_ascii_lowercase();
            close(&mut stack, &name);
            continue;
        }
        let name_end = inner.find(|c: char| c.is_whitespace() || c == '/').unwrap_or(inner.len());
        let name = inner[..name_end].to_ascii_lowercase();
        if name.is_empty() || !name.chars().next().unwrap().is_ascii_alphabetic() {
            text(&mut stack, &alloc::format!("<{}>", inner));
            continue;
        }
        // Implied end tags.
        if CLOSES_P.contains(&name.as_str()) && stack.iter().any(|e| e.name == "p") {
            close(&mut stack, "p");
        }
        match name.as_str() {
            "li" => {
                if let Some(pos) = stack.iter().rposition(|e| e.name == "li" || e.name == "ul" || e.name == "ol") {
                    if stack[pos].name == "li" {
                        close(&mut stack, "li");
                    }
                }
            }
            "tr" => {
                if stack.iter().rposition(|e| e.name == "tr" || e.name == "table").is_some_and(|p| stack[p].name == "tr") {
                    close(&mut stack, "tr");
                }
            }
            "td" | "th" => {
                if let Some(p) = stack.iter().rposition(|e| e.name == "td" || e.name == "th" || e.name == "tr") {
                    if stack[p].name != "tr" {
                        let n = stack[p].name.clone();
                        close(&mut stack, &n);
                    }
                }
            }
            "option" => {
                if stack.last().is_some_and(|e| e.name == "option") {
                    close(&mut stack, "option");
                }
            }
            "dt" | "dd" => {
                if stack.last().is_some_and(|e| e.name == "dt" || e.name == "dd") {
                    let n = stack.last().unwrap().name.clone();
                    close(&mut stack, &n);
                }
            }
            _ => {}
        }
        let attrs = parse_attrs(&inner[name_end..]);
        let el = Element { name: name.clone(), attrs, children: Vec::new() };
        if VOID.contains(&name.as_str()) || inner.ends_with('/') {
            stack.last_mut().unwrap().children.push(Node::Element(el));
            continue;
        }
        if RAW.contains(&name.as_str()) {
            let lower = rest.to_ascii_lowercase();
            let end = lower.find(&alloc::format!("</{}", name)).unwrap_or(rest.len());
            let mut el = el;
            if !rest[..end].is_empty() {
                let content = if name == "textarea" || name == "title" { decode_entities(&rest[..end]) } else { rest[..end].to_string() };
                el.children.push(Node::Text(content));
            }
            stack.last_mut().unwrap().children.push(Node::Element(el));
            rest = &rest[end..];
            rest = rest.find('>').map_or("", |e| &rest[e + 1..]);
            continue;
        }
        stack.push(el);
    }
    while stack.len() > 1 {
        let e = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(Node::Element(e));
    }
    stack.pop().unwrap()
}
