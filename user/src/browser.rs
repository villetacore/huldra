//! The part of a web browser that does not depend on how pages are shown:
//! loading (http, https, local files and directories), history, the
//! selected link or field, filling and submitting forms, searching.
//! `browse` (terminal) and `web` (window) draw [`Browser::page`] their
//! own way and call these methods on key presses and clicks.

use crate::{format, fs, http, String, ToString, Vec};
use huldra_http::{Request, Url};
use huldra_web::layout::{FieldKind, Page};

/// Where a page came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    Web(Url),
    File(String),
    /// The built-in start page.
    Home,
}

impl core::fmt::Display for Location {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Location::Web(u) => write!(f, "{}", u),
            Location::File(p) => write!(f, "file://{}", p),
            Location::Home => f.write_str("about:home"),
        }
    }
}

/// Something on the page that can be selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Link(usize),
    Field(usize),
}

pub struct Document {
    pub location: Location,
    pub page: Page,
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    /// HTML source, kept to lay out again at another width.
    html: Option<String>,
    /// Plain text (for text types).
    text: Option<String>,
}

pub const SEARCH: &str = "https://lite.duckduckgo.com/lite/?q=";

const HOME: &str = r#"<html><head><title>Huldra browser</title></head><body>
<h1>Huldra web browser</h1>
<p>Pages are shown as structured text: no JavaScript, no styles. HTTPS uses
TLS 1.3 with the certificates in /etc/ssl/certs.</p>
<form action="https://lite.duckduckgo.com/lite/" method="get">Search the web: <input name="q" value=""> <input type="submit" value="Search"></form>
<h2>Places</h2>
<ul>
<li><a href="https://lite.duckduckgo.com/lite/">DuckDuckGo Lite</a> - web search that works without JavaScript</li>
<li><a href="https://en.m.wikipedia.org/wiki/Special:Random">A random Wikipedia article</a></li>
<li><a href="https://news.ycombinator.com/">Hacker News</a></li>
<li><a href="https://text.npr.org/">NPR text-only news</a></li>
<li><a href="https://example.com/">example.com</a></li>
<li><a href="https://github.com/villetacore/huldra">Huldra on GitHub</a></li>
<li><a href="file:///usr/share/huldra/docs/">Huldra documentation</a> (local files)</li>
</ul>
<h2>Keys</h2>
<p>Tab and Shift+Tab move between links and fields, Enter follows a link or
edits a field. <b>g</b> goes to an address (or searches), <b>b</b>/Left goes
back, <b>f</b> forward, <b>r</b> reloads, <b>/</b> finds text, <b>s</b> saves
the page or the selected link, <b>q</b> quits.</p>
</body></html>"#;

pub struct Browser {
    pub doc: Document,
    pub back: Vec<Location>,
    pub forward: Vec<Location>,
    pub selected: Option<Target>,
    /// First line shown.
    pub top: usize,
    pub width: usize,
    pub insecure: bool,
    /// Last message for the status line.
    pub message: String,
}

fn guess_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html",
        "md" => "text/markdown",
        "png" | "jpg" | "jpeg" | "gif" | "pkg" | "gz" | "tar" | "img" | "bin" | "iso" => "application/octet-stream",
        _ => "text/plain",
    }
}

fn dir_listing(path: &str) -> String {
    let mut s = format!("<html><head><title>{}</title></head><body><h1>Index of {}</h1><ul>", path, path);
    if path != "/" {
        s.push_str("<li><a href=\"..\">..</a></li>");
    }
    for e in fs::read_dir(path).unwrap_or_default() {
        let slash = if e.is_dir() { "/" } else { "" };
        s.push_str(&format!("<li><a href=\"{}{}\">{}{}</a></li>", e.name, slash, e.name, slash));
    }
    s.push_str("</ul></body></html>");
    s
}

impl Document {
    fn lay_out(&mut self, width: usize) {
        self.page = if let Some(h) = &self.html {
            huldra_web::render(h, width)
        } else if let Some(t) = &self.text {
            if self.content_type == "text/markdown" {
                huldra_web::plain(&huldra_md::render(t, width, false), width)
            } else {
                huldra_web::plain(t, width)
            }
        } else {
            huldra_web::plain(&format!("{} ({} bytes): not something that can be shown.\nPress s to save it.", self.content_type, self.body.len()), width)
        };
        if self.page.title.is_empty() {
            self.page.title = self.location.to_string();
        }
    }

    fn from_body(location: Location, status: u16, content_type: &str, body: Vec<u8>, width: usize) -> Document {
        let ct = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        let latin1 = content_type.to_ascii_lowercase().contains("iso-8859-1") || content_type.to_ascii_lowercase().contains("windows-1252");
        let decode = |b: &[u8]| -> String { if latin1 { b.iter().map(|&c| c as char).collect() } else { String::from_utf8_lossy(b).into_owned() } };
        let is_html = ct == "text/html" || ct == "application/xhtml+xml" || (ct.is_empty() || ct == "application/octet-stream") && huldra_web::looks_like_html(&body);
        let is_text = !is_html && (ct.starts_with("text/") || ct.ends_with("json") || ct.ends_with("xml") || ct.ends_with("javascript"));
        let mut d = Document {
            location,
            page: Page::default(),
            status,
            content_type: if is_html { "text/html".into() } else { ct },
            html: is_html.then(|| decode(&body)),
            text: is_text.then(|| decode(&body)),
            body,
        };
        d.lay_out(width);
        d
    }

    pub fn is_binary(&self) -> bool {
        self.html.is_none() && self.text.is_none()
    }
}

impl Browser {
    pub fn new(width: usize) -> Browser {
        let mut doc = Document::from_body(Location::Home, 200, "text/html", HOME.as_bytes().to_vec(), width);
        doc.page.title = "Huldra browser".into();
        Browser { doc, back: Vec::new(), forward: Vec::new(), selected: None, top: 0, width, insecure: false, message: String::new() }
    }

    /// Turns what the user typed into a location: a URL, a path, or a search.
    pub fn parse_input(&self, input: &str) -> Result<Location, String> {
        let s = input.trim();
        if s.is_empty() || s == "about:home" {
            return Ok(Location::Home);
        }
        if let Some(p) = s.strip_prefix("file://") {
            return Ok(Location::File(p.to_string()));
        }
        if s.starts_with('/') || s.starts_with("./") || s.starts_with("../") {
            let abs = if s.starts_with('/') { s.to_string() } else { fs::join(&fs::current_dir().unwrap_or_default(), s) };
            return Ok(Location::File(abs));
        }
        if s.contains("://") || (!s.contains(' ') && (s.contains('.') || s.contains(':')) ) {
            return Url::parse(s).map(Location::Web);
        }
        Url::parse(&format!("{}{}", SEARCH, huldra_http::form_encode(s))).map(Location::Web)
    }

    /// Resolves a link of the current page.
    pub fn resolve(&self, href: &str) -> Result<Location, String> {
        match &self.doc.location {
            Location::Web(base) => {
                if let Some(p) = href.strip_prefix("file://") {
                    return Ok(Location::File(p.into()));
                }
                base.join(href).map(Location::Web)
            }
            Location::File(path) => {
                if href.contains("://") {
                    return self.parse_input(href);
                }
                let href = href.split('#').next().unwrap_or("");
                if href.is_empty() {
                    return Ok(self.doc.location.clone());
                }
                let dir = if path.ends_with('/') { path.clone() } else { path[..path.rfind('/').map_or(0, |i| i + 1)].to_string() };
                let joined = if href.starts_with('/') { href.to_string() } else { format!("{}{}", dir, href) };
                // Normalize . and ..
                let mut parts: Vec<&str> = Vec::new();
                for c in joined.split('/') {
                    match c {
                        "" | "." => {}
                        ".." => {
                            parts.pop();
                        }
                        c => parts.push(c),
                    }
                }
                let mut p = format!("/{}", parts.join("/"));
                if joined.ends_with('/') && p != "/" {
                    p.push('/');
                }
                Ok(Location::File(p))
            }
            Location::Home => self.parse_input(href),
        }
    }

    fn load(&self, loc: &Location, post: Option<String>) -> Result<Document, String> {
        match loc {
            Location::Home => Ok(Browser::new(self.width).doc),
            Location::File(path) => {
                let st = fs::metadata(path).map_err(|e| format!("{}: {}", path, e))?;
                if fs::is_dir(&st) {
                    let p = if path.ends_with('/') { path.clone() } else { format!("{}/", path) };
                    return Ok(Document::from_body(Location::File(p.clone()), 200, "text/html", dir_listing(&p).into_bytes(), self.width));
                }
                let body = fs::read(path).map_err(|e| format!("{}: {}", path, e))?;
                Ok(Document::from_body(loc.clone(), 200, guess_type(path), body, self.width))
            }
            Location::Web(url) => {
                let req = match post {
                    Some(data) => Request::post(url.clone(), "application/x-www-form-urlencoded", data.into_bytes()),
                    None => Request::get(url.clone()),
                };
                let req = req.header("Accept", "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.5").header("Accept-Language", "en");
                let opts = http::Options { insecure: self.insecure, ..http::Options::default() };
                let r = http::request(req, &opts)?;
                let ct = r.head.header("Content-Type").unwrap_or("").to_string();
                let mut d = Document::from_body(Location::Web(r.url.clone()), r.head.status, &ct, r.body, self.width);
                if r.head.status >= 400 && d.page.title == d.location.to_string() {
                    d.page.title = http::status_text(&r.head);
                }
                Ok(d)
            }
        }
    }

    fn show(&mut self, doc: Document, record: bool) {
        if record {
            self.back.push(core::mem::replace(&mut self.doc.location, Location::Home));
            self.forward.clear();
        }
        self.top = 0;
        if let Location::Web(u) = &doc.location {
            if let Some(f) = &u.fragment {
                if let Some((_, line)) = doc.page.anchors.iter().find(|(id, _)| id == f) {
                    self.top = *line;
                }
            }
        }
        self.message = if doc.status >= 400 { format!("HTTP {}", doc.status) } else { String::new() };
        self.doc = doc;
        self.selected = None;
    }

    /// Opens a location, recording the current one in the history.
    pub fn open(&mut self, loc: Location) {
        self.open_with(loc, None);
    }

    fn open_with(&mut self, loc: Location, post: Option<String>) {
        // Same page, different fragment: just scroll.
        if let (Location::Web(a), Location::Web(b)) = (&self.doc.location, &loc) {
            if b.fragment.is_some() && a.scheme == b.scheme && a.host == b.host && a.port == b.port && a.path == b.path && post.is_none() {
                if let Some((_, line)) = self.doc.page.anchors.iter().find(|(id, _)| Some(id) == b.fragment.as_ref()) {
                    self.top = *line;
                    return;
                }
            }
        }
        match self.load(&loc, post) {
            Ok(doc) => self.show(doc, true),
            Err(e) => self.message = e,
        }
    }

    pub fn go_back(&mut self) {
        let Some(loc) = self.back.pop() else {
            self.message = "no previous page".into();
            return;
        };
        match self.load(&loc, None) {
            Ok(doc) => {
                self.forward.push(self.doc.location.clone());
                self.show(doc, false);
            }
            Err(e) => self.message = e,
        }
    }

    pub fn go_forward(&mut self) {
        let Some(loc) = self.forward.pop() else {
            self.message = "no next page".into();
            return;
        };
        match self.load(&loc, None) {
            Ok(doc) => {
                self.back.push(self.doc.location.clone());
                self.show(doc, false);
            }
            Err(e) => self.message = e,
        }
    }

    pub fn reload(&mut self) {
        let loc = self.doc.location.clone();
        let top = self.top;
        match self.load(&loc, None) {
            Ok(doc) => {
                self.show(doc, false);
                self.top = top.min(self.doc.page.lines.len().saturating_sub(1));
            }
            Err(e) => self.message = e,
        }
    }

    /// Lays the page out again for a new width.
    pub fn set_width(&mut self, width: usize) {
        if width != self.width {
            self.width = width;
            self.doc.lay_out(width);
        }
    }

    /// Links and fields in reading order.
    pub fn targets(&self) -> Vec<(Target, usize)> {
        let mut out: Vec<(Target, usize)> = Vec::new();
        for (i, line) in self.doc.page.lines.iter().enumerate() {
            for s in line {
                let t = match (s.link, s.field) {
                    (_, Some(f)) => Target::Field(f),
                    (Some(l), None) => Target::Link(l),
                    _ => continue,
                };
                if out.last().map(|(x, _)| *x) != Some(t) && !out.iter().any(|(x, _)| *x == t) {
                    out.push((t, i));
                }
            }
        }
        out
    }

    /// Moves the selection to the next (or previous) target, preferring
    /// ones on screen; returns its line.
    pub fn select_next(&mut self, forward: bool, rows: usize) -> Option<usize> {
        let targets = self.targets();
        if targets.is_empty() {
            return None;
        }
        let cur = self.selected.and_then(|s| targets.iter().position(|(t, _)| *t == s));
        let next = match (cur, forward) {
            (Some(i), true) => (i + 1) % targets.len(),
            (Some(i), false) => (i + targets.len() - 1) % targets.len(),
            // Nothing selected: the first one visible.
            (None, true) => targets.iter().position(|(_, l)| *l >= self.top).unwrap_or(0),
            (None, false) => targets.iter().rposition(|(_, l)| *l < self.top + rows).unwrap_or(targets.len() - 1),
        };
        let (t, line) = targets[next];
        self.selected = Some(t);
        if line < self.top || line >= self.top + rows {
            self.top = line.saturating_sub(rows / 3);
        }
        self.message = match t {
            Target::Link(n) => self.resolve(&self.doc.page.links[n]).map(|l| l.to_string()).unwrap_or_else(|e| e),
            Target::Field(_) => "Enter edits this field".into(),
        };
        Some(line)
    }

    /// What Enter on the selection needs: `Some(field)` when a text field
    /// wants a new value from the user.
    pub fn activate(&mut self) -> Option<usize> {
        match self.selected? {
            Target::Link(n) => {
                let href = self.doc.page.links[n].clone();
                match self.resolve(&href) {
                    Ok(loc) => self.open(loc),
                    Err(e) => self.message = e,
                }
                None
            }
            Target::Field(f) => {
                let field = &mut self.doc.page.fields[f];
                match &field.kind {
                    FieldKind::Checkbox => {
                        field.checked = !field.checked;
                        self.refresh_fields();
                        None
                    }
                    FieldKind::Select(opts) => {
                        let i = opts.iter().position(|(v, _)| *v == field.value).map_or(0, |i| (i + 1) % opts.len().max(1));
                        if let Some((v, _)) = opts.get(i) {
                            field.value = v.clone();
                        }
                        self.refresh_fields();
                        None
                    }
                    FieldKind::Submit => {
                        self.submit(f);
                        None
                    }
                    _ => Some(f),
                }
            }
        }
    }

    pub fn set_field(&mut self, f: usize, value: String) {
        self.doc.page.fields[f].value = value;
        self.refresh_fields();
    }

    /// Redraws field labels after their values changed.
    fn refresh_fields(&mut self) {
        let labels: Vec<String> = self.doc.page.fields.iter().map(Page::field_label).collect();
        for line in self.doc.page.lines.iter_mut() {
            for s in line.iter_mut() {
                if let Some(f) = s.field {
                    s.text = labels[f].clone();
                }
            }
        }
    }

    /// Submits the form field `via` belongs to (a submit button, or Enter
    /// in a text field).
    pub fn submit(&mut self, via: usize) {
        let Some(form) = self.doc.page.fields[via].form else {
            self.message = "this field is not in a form".into();
            return;
        };
        let button = (self.doc.page.fields[via].kind == FieldKind::Submit).then_some(via);
        let (method, action, data) = self.doc.page.submission(form, button);
        let target = match self.resolve(if action.is_empty() { "" } else { &action }) {
            Ok(Location::Web(mut u)) => {
                u.fragment = None;
                if method == "post" {
                    self.open_with(Location::Web(u), Some(data));
                    return;
                }
                let base = u.path.split('?').next().unwrap_or("/").to_string();
                u.path = format!("{}?{}", base, data);
                Location::Web(u)
            }
            Ok(other) => other,
            Err(e) => {
                self.message = e;
                return;
            }
        };
        self.open(target);
    }

    /// Finds `text` after the top line; returns the line.
    pub fn find(&mut self, text: &str) -> Option<usize> {
        let t = text.to_lowercase();
        let n = self.doc.page.lines.len();
        for k in 1..=n {
            let i = (self.top + k) % n;
            if self.doc.page.line_text(i).to_lowercase().contains(&t) {
                self.top = i;
                return Some(i);
            }
        }
        self.message = format!("'{}' not found", text);
        None
    }

    /// Saves the selected link's target (or the page) into the current
    /// directory; returns the file name.
    pub fn save(&mut self) -> Result<String, String> {
        let (loc, body) = match self.selected {
            Some(Target::Link(n)) => {
                let loc = self.resolve(&self.doc.page.links[n].clone())?;
                let doc = self.load(&loc, None)?;
                (loc, doc.body)
            }
            _ => (self.doc.location.clone(), self.doc.body.clone()),
        };
        let name = match &loc {
            Location::Web(u) => {
                let n = u.file_name();
                if n.is_empty() { "index.html".to_string() } else { huldra_http::percent_decode(n, false) }
            }
            Location::File(p) => p.rsplit('/').next().unwrap_or("file").to_string(),
            Location::Home => "home.html".into(),
        };
        fs::write(&name, &body).map_err(|e| format!("{}: {}", name, e))?;
        Ok(format!("saved {} ({} bytes)", name, body.len()))
    }
}
