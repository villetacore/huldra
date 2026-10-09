//! HTTP and HTTPS client: [`huldra_http`] messages over TCP, or over TLS 1.3
//! ([`huldra_tls`]) with the server's certificate checked against
//! /etc/ssl/certs/ca-certificates.crt.
//!
//! ```ignore
//! let r = http::get("https://example.com/", &Options::default())?;
//! println!("{} {}", r.head.status, String::from_utf8_lossy(&r.body));
//! ```
//!
//! Redirects are followed, `gzip` bodies decoded, and [`fetch`] streams a
//! body to a callback (wget writes it straight to the file).

use crate::net::{self, Socket};
use crate::{format, fs, sys, time, String, ToString, Vec};
use huldra_http::{Request, ResponseHead, ResponseParser, Url};
use huldra_tls::{Client, Config, RootStore};

pub use huldra_http::{Url as HttpUrl, form_encode, percent_decode};

pub const CA_BUNDLE: &str = "/etc/ssl/certs/ca-certificates.crt";

pub type Result<T> = core::result::Result<T, String>;

pub struct Options {
    /// Do not check certificates (`--insecure`).
    pub insecure: bool,
    pub max_redirects: u32,
    /// Extra request headers.
    pub headers: Vec<(String, String)>,
    /// Ask for gzip and decode it (the whole body is kept in memory).
    pub gzip: bool,
    pub timeout_ms: u64,
}

impl Default for Options {
    fn default() -> Self {
        Options { insecure: false, max_redirects: 10, headers: Vec::new(), gzip: true, timeout_ms: 30_000 }
    }
}

pub struct Response {
    /// Where the body came from, after redirects.
    pub url: Url,
    pub head: ResponseHead,
    pub body: Vec<u8>,
}

static mut ROOTS: Option<&'static RootStore> = None;

/// The trusted roots, read once per process.
pub fn roots() -> &'static RootStore {
    // Programs use this from one thread.
    unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(ROOTS);
        if slot.is_none() {
            // The bundle, plus any other *.pem in the directory.
            let mut pem = fs::read_to_string(CA_BUNDLE).unwrap_or_default();
            for e in fs::read_dir("/etc/ssl/certs").unwrap_or_default() {
                if e.name.ends_with(".pem") {
                    pem.push_str(&fs::read_to_string(&fs::join("/etc/ssl/certs", &e.name)).unwrap_or_default());
                }
            }
            *slot = Some(alloc::boxed::Box::leak(alloc::boxed::Box::new(RootStore::from_pem(&pem))));
        }
        slot.unwrap()
    }
}

/// A connection: plain TCP or TLS on top of it.
pub enum Conn {
    Plain(Socket),
    Tls(Socket, Client<'static>),
}

fn errno(e: crate::Errno, what: &str) -> String {
    match e {
        crate::Errno::ENOENT => format!("{}: unknown host", what),
        crate::Errno::ECONNREFUSED => format!("{}: connection refused", what),
        crate::Errno::ETIMEDOUT | crate::Errno::EAGAIN => format!("{}: timed out", what),
        e => format!("{}: {}", what, e.message()),
    }
}

impl Conn {
    pub fn open(url: &Url, opts: &Options) -> Result<Conn> {
        let what = url.authority();
        let ip = net::resolve(&url.host).map_err(|e| errno(e, &url.host))?;
        let s = Socket::tcp().map_err(|e| errno(e, &what))?;
        let _ = s.set_timeout(opts.timeout_ms);
        s.connect(ip, url.port).map_err(|e| errno(e, &what))?;
        if !url.is_https() {
            return Ok(Conn::Plain(s));
        }
        let roots = roots();
        if roots.is_empty() && !opts.insecure {
            return Err(format!("no trusted certificates in {} (use --insecure to skip the check)", CA_BUNDLE));
        }
        let mut random = [0u8; 64];
        sys::getrandom(&mut random).map_err(|e| errno(e, "getrandom"))?;
        let config = Config { roots, now: time::now(), insecure: opts.insecure, alpn: &["http/1.1"] };
        let mut tls = Client::new(&url.host, config, &random);
        // Run the handshake now so certificate errors come first.
        let mut buf = alloc::vec![0u8; 16384];
        while !tls.is_connected() {
            let out = tls.take_output();
            if !out.is_empty() {
                s.send_all(&out).map_err(|e| errno(e, &what))?;
            }
            let n = s.recv(&mut buf).map_err(|e| errno(e, &what))?;
            if n == 0 {
                return Err(format!("{}: connection closed during the TLS handshake", what));
            }
            tls.feed(&buf[..n]).map_err(|e| format!("{}: TLS: {}", what, e))?;
        }
        let out = tls.take_output();
        s.send_all(&out).map_err(|e| errno(e, &what))?;
        Ok(Conn::Tls(s, tls))
    }

    pub fn send(&mut self, data: &[u8]) -> Result<()> {
        match self {
            Conn::Plain(s) => s.send_all(data).map_err(|e| errno(e, "send")),
            Conn::Tls(s, tls) => {
                tls.write(data);
                s.send_all(&tls.take_output()).map_err(|e| errno(e, "send"))
            }
        }
    }

    /// Received data; empty at the end of the stream.
    pub fn recv(&mut self) -> Result<Vec<u8>> {
        let mut buf = alloc::vec![0u8; 16384];
        loop {
            match self {
                Conn::Plain(s) => {
                    let n = s.recv(&mut buf).map_err(|e| errno(e, "receive"))?;
                    buf.truncate(n);
                    return Ok(buf);
                }
                Conn::Tls(s, tls) => {
                    if tls.is_closed() {
                        return Ok(Vec::new());
                    }
                    let n = s.recv(&mut buf).map_err(|e| errno(e, "receive"))?;
                    if n == 0 {
                        return Ok(Vec::new());
                    }
                    tls.feed(&buf[..n]).map_err(|e| format!("TLS: {}", e))?;
                    let out = tls.take_output();
                    if !out.is_empty() {
                        s.send_all(&out).map_err(|e| errno(e, "send"))?;
                    }
                    let data = tls.read();
                    if !data.is_empty() || tls.is_closed() {
                        return Ok(data);
                    }
                }
            }
        }
    }

    /// TLS details for messages (`None` for plain HTTP).
    pub fn tls_info(&self) -> Option<String> {
        match self {
            Conn::Plain(_) => None,
            Conn::Tls(_, t) => {
                let who = t.certificates().first().map(|c| format!("{} (issued by {})", c.subject_name(), c.issuer_name())).unwrap_or_default();
                Some(format!("{}, {}", t.cipher_suite(), who))
            }
        }
    }
}

/// Sends `request`, following redirects. Body bytes go to `sink` (with the
/// response head) as they arrive; `progress(received, total)` is called
/// along the way.
/// Returns the final URL and response head.
pub fn fetch(
    mut request: Request,
    opts: &Options,
    sink: &mut dyn FnMut(&ResponseHead, &[u8]) -> Result<()>,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(Url, ResponseHead)> {
    for _ in 0..=opts.max_redirects {
        if !matches!(request.url.scheme.as_str(), "http" | "https") {
            return Err(format!("{}: unsupported scheme", request.url));
        }
        for (n, v) in &opts.headers {
            request = request.header(n, v);
        }
        if opts.gzip {
            request = request.header("Accept-Encoding", "gzip");
        }
        let mut conn = Conn::open(&request.url, opts)?;
        conn.send(&request.to_bytes())?;
        let mut parser = ResponseParser::new(request.method == "HEAD");
        let mut gzip_body: Option<Vec<u8>> = None;
        let mut redirect = false;
        loop {
            let data = conn.recv()?;
            if data.is_empty() {
                parser.finish()?;
                break;
            }
            let mut chunks: Vec<Vec<u8>> = Vec::new();
            parser.feed(&data, &mut |b| chunks.push(b.to_vec()))?;
            if let Some(h) = parser.head() {
                redirect = h.is_redirect();
                let gzip = h.header("Content-Encoding").is_some_and(|e| e.eq_ignore_ascii_case("gzip"));
                if !redirect {
                    for c in &chunks {
                        if gzip {
                            gzip_body.get_or_insert_with(Vec::new).extend_from_slice(c);
                        } else {
                            sink(h, c)?;
                        }
                    }
                    progress(parser.received(), h.content_length());
                }
            }
            if parser.is_done() {
                break;
            }
        }
        let head = parser.head().cloned().ok_or("no response")?;
        if redirect {
            let location = head.header("Location").unwrap_or("/");
            let next = request.url.join(location)?;
            // 303 (and 301/302 for POST, as browsers do) turn into GET.
            if head.status == 303 || (request.method == "POST" && head.status != 307 && head.status != 308) {
                request = Request::get(next);
            } else {
                request.url = next;
            }
            continue;
        }
        if let Some(g) = gzip_body {
            let body = huldra_flate::gzip_decompress(&g).map_err(|e| format!("bad gzip body: {:?}", e))?;
            sink(&head, &body)?;
        }
        return Ok((request.url, head));
    }
    Err("too many redirects".into())
}

/// GET into memory.
pub fn get(url: &str, opts: &Options) -> Result<Response> {
    let url = Url::parse(url)?;
    request(Request::get(url), opts)
}

/// Any request, body into memory.
pub fn request(req: Request, opts: &Options) -> Result<Response> {
    let mut body = Vec::new();
    let (url, head) = fetch(req, opts, &mut |_, b| {
        body.extend_from_slice(b);
        Ok(())
    }, &mut |_, _| {})?;
    Ok(Response { url, head, body })
}

pub fn status_text(head: &ResponseHead) -> String {
    format!("{} {}", head.status, head.reason).trim().to_string()
}
