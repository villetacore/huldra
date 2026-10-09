//! HTTP/1.1 without I/O: [`Url`] parsing and resolution of relative
//! references (RFC 3986), request formatting, and an incremental
//! [`ResponseParser`] that understands `Content-Length`, chunked transfer
//! coding and bodies that end when the connection closes.
//!
//! The program supplies the transport (TCP or TLS) and feeds the parser
//! whatever it receives; the parser hands out body bytes as they arrive,
//! so a download can go straight to a file.

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub type Result<T> = core::result::Result<T, String>;

// ---------------------------------------------------------------------- URLs

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    /// `http` or `https` (lower case); other schemes parse but cannot be fetched.
    pub scheme: String,
    pub host: String,
    pub port: u16,
    /// Path and query, always starting with `/`.
    pub path: String,
    pub fragment: Option<String>,
}

pub fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    }
}

impl Url {
    /// Parses an absolute URL. A missing scheme means `http://`.
    pub fn parse(s: &str) -> Result<Url> {
        let s = s.trim();
        let (scheme, rest) = match s.find("://") {
            Some(i) if s[..i].chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) && i > 0 => (s[..i].to_ascii_lowercase(), &s[i + 3..]),
            _ => ("http".to_string(), s),
        };
        let (rest, fragment) = match rest.split_once('#') {
            Some((r, f)) => (r, Some(f.to_string())),
            None => (rest, None),
        };
        let split = rest.find(['/', '?']).unwrap_or(rest.len());
        let (authority, path) = rest.split_at(split);
        // Drop user:password@.
        let hostport = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let (host, port) = if let Some(v6) = hostport.strip_prefix('[') {
            let (h, after) = v6.split_once(']').ok_or("bad IPv6 address")?;
            (h.to_string(), after.strip_prefix(':'))
        } else {
            match hostport.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), Some(p)),
                None => (hostport.to_string(), None),
            }
        };
        if host.is_empty() {
            return Err(format!("no host in '{}'", s));
        }
        let port = match port {
            Some(p) if !p.is_empty() => p.parse().map_err(|_| format!("bad port in '{}'", s))?,
            _ => default_port(&scheme).unwrap_or(0),
        };
        let path = if path.starts_with('/') { path.to_string() } else { format!("/{}", path) };
        Ok(Url { scheme, host: host.to_ascii_lowercase(), port, path: normalize_path(&path), fragment })
    }

    /// Resolves a reference found in a page or a `Location` header.
    pub fn join(&self, reference: &str) -> Result<Url> {
        let r = reference.trim();
        if r.is_empty() {
            return Ok(Url { fragment: None, ..self.clone() });
        }
        if let Some(f) = r.strip_prefix('#') {
            return Ok(Url { fragment: Some(f.to_string()), ..self.clone() });
        }
        if let Some(rest) = r.strip_prefix("//") {
            return Url::parse(&format!("{}://{}", self.scheme, rest));
        }
        if let Some(i) = r.find(':') {
            let scheme = &r[..i];
            if !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) && !r[..i].contains('/') {
                if r[i + 1..].starts_with("//") {
                    return Url::parse(r);
                }
                // mailto:, javascript: ...: not something we can follow.
                return Err(format!("unsupported link '{}'", r));
            }
        }
        let (r, fragment) = match r.split_once('#') {
            Some((a, f)) => (a, Some(f.to_string())),
            None => (r, None),
        };
        let path = if r.starts_with('/') {
            r.to_string()
        } else if let Some(q) = r.strip_prefix('?') {
            let base = self.path.split('?').next().unwrap_or("/");
            format!("{}?{}", base, q)
        } else {
            let base = self.path.split('?').next().unwrap_or("/");
            let dir = &base[..base.rfind('/').map_or(0, |i| i + 1)];
            format!("{}{}", dir, r)
        };
        Ok(Url { path: normalize_path(&path), fragment, ..self.clone() })
    }

    /// `host` or `host:port` when the port is not the scheme's default.
    pub fn authority(&self) -> String {
        if Some(self.port) == default_port(&self.scheme) {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    pub fn is_https(&self) -> bool {
        self.scheme == "https"
    }

    /// The last path segment (for naming downloads).
    pub fn file_name(&self) -> &str {
        let p = self.path.split('?').next().unwrap_or("");
        p.rsplit('/').next().unwrap_or("")
    }
}

impl core::fmt::Display for Url {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}://{}{}", self.scheme, self.authority(), self.path)?;
        if let Some(frag) = &self.fragment {
            write!(f, "#{}", frag)?;
        }
        Ok(())
    }
}

/// Removes `.` and `..` segments (RFC 3986 section 5.2.4), keeping the query.
fn normalize_path(path: &str) -> String {
    let (p, query) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path, None),
    };
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = p.split('/').skip(1).collect();
    let n = segments.len();
    for (i, seg) in segments.iter().enumerate() {
        match *seg {
            "." => {
                if i == n - 1 {
                    out.push("");
                }
            }
            ".." => {
                out.pop();
                if i == n - 1 {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    let mut s = String::from("/");
    s.push_str(&out.join("/"));
    if let Some(q) = query {
        s.push('?');
        s.push_str(q);
    }
    s
}

/// Percent-encodes form data (`application/x-www-form-urlencoded`).
pub fn form_encode(s: &str) -> String {
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

/// Decodes `%XX` escapes (and `+` as a space when `plus` is set).
pub fn percent_decode(s: &str, plus: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(core::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b'+' if plus => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ------------------------------------------------------------------ requests

pub const USER_AGENT: &str = concat!("Huldra/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn get(url: Url) -> Request {
        Request { method: "GET".into(), url, headers: Vec::new(), body: Vec::new() }
    }

    pub fn post(url: Url, content_type: &str, body: Vec<u8>) -> Request {
        let mut r = Request { method: "POST".into(), url, headers: Vec::new(), body };
        r.headers.push(("Content-Type".into(), content_type.into()));
        r
    }

    pub fn header(mut self, name: &str, value: &str) -> Request {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self.headers.push((name.into(), value.into()));
        self
    }

    fn has(&self, name: &str) -> bool {
        self.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
    }

    /// The request as sent on the wire (one request per connection).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut s = format!("{} {} HTTP/1.1\r\nHost: {}\r\n", self.method, self.url.path, self.url.authority());
        if !self.has("User-Agent") {
            s.push_str(&format!("User-Agent: {}\r\n", USER_AGENT));
        }
        if !self.has("Accept") {
            s.push_str("Accept: */*\r\n");
        }
        if !self.has("Connection") {
            s.push_str("Connection: close\r\n");
        }
        for (n, v) in &self.headers {
            s.push_str(&format!("{}: {}\r\n", n, v));
        }
        if !self.body.is_empty() || self.method == "POST" || self.method == "PUT" {
            s.push_str(&format!("Content-Length: {}\r\n", self.body.len()));
        }
        s.push_str("\r\n");
        let mut out = s.into_bytes();
        out.extend_from_slice(&self.body);
        out
    }
}

// ----------------------------------------------------------------- responses

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResponseHead {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
}

impl ResponseHead {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn is_redirect(&self) -> bool {
        matches!(self.status, 301 | 302 | 303 | 307 | 308) && self.header("Location").is_some()
    }

    /// `Content-Length`, if the server sent one.
    pub fn content_length(&self) -> Option<u64> {
        self.header("Content-Length")?.trim().parse().ok()
    }

    fn parse(text: &str) -> Result<ResponseHead> {
        let mut lines = text.split("\r\n");
        let status_line = lines.next().unwrap_or("");
        let mut parts = status_line.splitn(3, ' ');
        let version = parts.next().unwrap_or("");
        if !version.starts_with("HTTP/") {
            return Err(format!("not an HTTP response: '{}'", status_line.chars().take(40).collect::<String>()));
        }
        let status = parts.next().and_then(|s| s.parse().ok()).ok_or("bad status line")?;
        let reason = parts.next().unwrap_or("").to_string();
        let mut headers: Vec<(String, String)> = Vec::new();
        for l in lines {
            if l.starts_with(' ') || l.starts_with('\t') {
                // Obsolete line folding: continue the previous header.
                if let Some(last) = headers.last_mut() {
                    last.1.push(' ');
                    last.1.push_str(l.trim());
                }
            } else if let Some((n, v)) = l.split_once(':') {
                headers.push((n.trim().to_string(), v.trim().to_string()));
            }
        }
        Ok(ResponseHead { status, reason, headers })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Framing {
    Length(u64),
    Chunked,
    Close,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Head,
    Body,
    /// Reading a chunk-size line.
    ChunkSize,
    /// Inside a chunk: bytes left.
    Chunk(u64),
    /// The CRLF after a chunk.
    ChunkEnd,
    /// Trailer lines after the last chunk.
    Trailer,
    Done,
}

/// Feed it the bytes of one response as they arrive.
pub struct ResponseParser {
    state: State,
    framing: Framing,
    buf: Vec<u8>,
    head: Option<ResponseHead>,
    /// The request was HEAD: no body whatever the headers say.
    head_request: bool,
    received: u64,
}

impl ResponseParser {
    pub fn new(head_request: bool) -> Self {
        ResponseParser { state: State::Head, framing: Framing::Close, buf: Vec::new(), head: None, head_request, received: 0 }
    }

    /// The status line and headers, once they have arrived.
    pub fn head(&self) -> Option<&ResponseHead> {
        self.head.as_ref()
    }

    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    /// Body bytes delivered so far (after removing chunk framing).
    pub fn received(&self) -> u64 {
        self.received
    }

    /// Processes received bytes; body data goes to `body`.
    pub fn feed(&mut self, data: &[u8], body: &mut dyn FnMut(&[u8])) -> Result<()> {
        self.buf.extend_from_slice(data);
        loop {
            match self.state {
                State::Done => {
                    self.buf.clear();
                    return Ok(());
                }
                State::Head => {
                    let Some(end) = find(&self.buf, b"\r\n\r\n") else {
                        if self.buf.len() > 64 * 1024 {
                            return Err("response headers too long".into());
                        }
                        return Ok(());
                    };
                    let head = ResponseHead::parse(&String::from_utf8_lossy(&self.buf[..end]))?;
                    self.buf.drain(..end + 4);
                    if (100..200).contains(&head.status) {
                        continue; // 100 Continue and friends: the real response follows
                    }
                    let te = head.header("Transfer-Encoding").map(|v| v.to_ascii_lowercase());
                    self.framing = if self.head_request || head.status == 204 || head.status == 304 {
                        Framing::None
                    } else if te.is_some_and(|t| t.contains("chunked")) {
                        Framing::Chunked
                    } else if let Some(n) = head.content_length() {
                        Framing::Length(n)
                    } else {
                        Framing::Close
                    };
                    self.head = Some(head);
                    self.state = match self.framing {
                        Framing::None | Framing::Length(0) => State::Done,
                        Framing::Chunked => State::ChunkSize,
                        _ => State::Body,
                    };
                }
                State::Body => {
                    if self.buf.is_empty() {
                        return Ok(());
                    }
                    let take = match self.framing {
                        Framing::Length(n) => ((n - self.received) as usize).min(self.buf.len()),
                        _ => self.buf.len(),
                    };
                    body(&self.buf[..take]);
                    self.received += take as u64;
                    self.buf.drain(..take);
                    if let Framing::Length(n) = self.framing {
                        if self.received == n {
                            self.state = State::Done;
                        }
                    }
                }
                State::ChunkSize => {
                    let Some(end) = find(&self.buf, b"\r\n") else {
                        return Ok(());
                    };
                    let line = String::from_utf8_lossy(&self.buf[..end]).into_owned();
                    let hex = line.split(';').next().unwrap_or("").trim();
                    let size = u64::from_str_radix(hex, 16).map_err(|_| format!("bad chunk size '{}'", hex))?;
                    self.buf.drain(..end + 2);
                    self.state = if size == 0 { State::Trailer } else { State::Chunk(size) };
                }
                State::Chunk(left) => {
                    if self.buf.is_empty() {
                        return Ok(());
                    }
                    let take = (left as usize).min(self.buf.len());
                    body(&self.buf[..take]);
                    self.received += take as u64;
                    self.buf.drain(..take);
                    self.state = if take as u64 == left { State::ChunkEnd } else { State::Chunk(left - take as u64) };
                }
                State::ChunkEnd => {
                    if self.buf.len() < 2 {
                        return Ok(());
                    }
                    if &self.buf[..2] != b"\r\n" {
                        return Err("missing CRLF after chunk".into());
                    }
                    self.buf.drain(..2);
                    self.state = State::ChunkSize;
                }
                State::Trailer => {
                    let Some(end) = find(&self.buf, b"\r\n") else {
                        return Ok(());
                    };
                    self.buf.drain(..end + 2);
                    if end == 0 {
                        self.state = State::Done;
                    }
                }
            }
        }
    }

    /// The connection closed: fine if the body was complete or ends at close.
    pub fn finish(&mut self) -> Result<()> {
        match self.state {
            State::Done => Ok(()),
            State::Body if self.framing == Framing::Close => {
                self.state = State::Done;
                Ok(())
            }
            State::Head => Err("connection closed before a response arrived".into()),
            _ => Err(format!("connection closed after {} bytes: response truncated", self.received)),
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests;
