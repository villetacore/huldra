//! Talking to a remote over smart HTTP(S): reference discovery, fetching
//! a pack, pushing one. Credentials come from the URL
//! (`https://user:token@host/...`) or ~/.git-credentials.

use crate::repo::Result;
use huldra_git::object::Id;
use huldra_git::protocol::{self, Advertisement, UploadResponse};
use huldra_http::{Request, Url};
use huldra_user::{env, format, fs, http, String, ToString};

/// A remote URL without credentials, plus the credentials if any.
pub struct Remote {
    pub url: String,
    auth: Option<String>,
}

impl Remote {
    pub fn new(url: &str) -> Result<Remote> {
        let (clean, user) = split_credentials(url);
        let auth = user.or_else(|| stored_credentials(&clean)).map(|(u, p)| format!("Basic {}", huldra_crypto::base64_encode(format!("{}:{}", u, p).as_bytes())));
        Url::parse(&clean).map_err(|e| format!("bad remote URL: {}", e))?;
        Ok(Remote { url: clean.trim_end_matches('/').to_string(), auth })
    }

    fn send(&self, req: Request) -> Result<http::Response> {
        let mut opts = http::Options::default();
        if let Some(a) = &self.auth {
            opts.headers.push(("Authorization".into(), a.clone()));
        }
        opts.headers.push(("Git-Protocol".into(), "version=0".into()));
        opts.insecure = env::var("GIT_SSL_NO_VERIFY").is_some();
        let r = http::request(req, &opts)?;
        match r.head.status {
            200 => Ok(r),
            401 | 403 => Err(format!(
                "{}: authentication failed ({}); use https://USER:TOKEN@host/... or a line like that in ~/.git-credentials",
                self.url,
                http::status_text(&r.head)
            )),
            404 => Err(format!("{}: repository not found", self.url)),
            _ => Err(format!("{}: server answered {}", self.url, http::status_text(&r.head))),
        }
    }

    /// The references the server has, for upload-pack or receive-pack.
    pub fn discover(&self, service: &str) -> Result<Advertisement> {
        let url = Url::parse(&format!("{}/info/refs?service={}", self.url, service))?;
        let r = self.send(Request::get(url))?;
        let ctype = r.head.header("Content-Type").unwrap_or("");
        if !ctype.contains(&format!("application/x-{}-advertisement", service)) {
            return Err(format!("{}: not a smart git server (dumb HTTP is not supported)", self.url));
        }
        Advertisement::parse(&r.body).map_err(|e| e.0)
    }

    pub fn fetch(&self, adv: &Advertisement, wants: &[Id], haves: &[Id], depth: Option<u32>) -> Result<UploadResponse> {
        let url = Url::parse(&format!("{}/git-upload-pack", self.url))?;
        let body = protocol::upload_request(wants, haves, depth, adv);
        let req = Request::post(url, "application/x-git-upload-pack-request", body).header("Accept", "application/x-git-upload-pack-result");
        let r = self.send(req)?;
        protocol::parse_upload_response(&r.body, adv.has("side-band-64k")).map_err(|e| e.0)
    }

    pub fn push(&self, adv: &Advertisement, updates: &[(Id, Id, String)], pack: &[u8]) -> Result<String> {
        let url = Url::parse(&format!("{}/git-receive-pack", self.url))?;
        let body = protocol::push_request(updates, pack, adv);
        let req = Request::post(url, "application/x-git-receive-pack-request", body).header("Accept", "application/x-git-receive-pack-result");
        let r = self.send(req)?;
        protocol::parse_push_response(&r.body, adv.has("side-band-64k")).map_err(|e| e.0)
    }
}

/// `https://user:pass@host/x` -> (`https://host/x`, (user, pass)).
fn split_credentials(url: &str) -> (String, Option<(String, String)>) {
    let Some(i) = url.find("://") else { return (url.to_string(), None) };
    let rest = &url[i + 3..];
    let end = rest.find('/').unwrap_or(rest.len());
    match rest[..end].rfind('@') {
        Some(at) => {
            let userinfo = &rest[..at];
            let (u, p) = userinfo.split_once(':').unwrap_or((userinfo, ""));
            (format!("{}{}", &url[..i + 3], &rest[at + 1..]), Some((huldra_http::percent_decode(u, false), huldra_http::percent_decode(p, false))))
        }
        None => (url.to_string(), None),
    }
}

/// From ~/.git-credentials (one `https://user:token@host` per line).
fn stored_credentials(url: &str) -> Option<(String, String)> {
    let home = env::var("HOME").unwrap_or("/root");
    let text = fs::read_to_string(&fs::join(home, ".git-credentials")).ok()?;
    let host = Url::parse(url).ok()?.host;
    text.lines().find_map(|l| {
        let (clean, creds) = split_credentials(l.trim());
        (Url::parse(&clean).ok()?.host == host).then_some(creds?)
    })
}
