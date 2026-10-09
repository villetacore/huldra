extern crate std;

use super::*;

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

#[test]
fn parsing_urls() {
    let u = url("https://Example.com:8443/a/b?q=1#top");
    assert_eq!((u.scheme.as_str(), u.host.as_str(), u.port, u.path.as_str()), ("https", "example.com", 8443, "/a/b?q=1"));
    assert_eq!(u.fragment.as_deref(), Some("top"));
    assert_eq!(u.to_string(), "https://example.com:8443/a/b?q=1#top");
    let u = url("example.com");
    assert_eq!((u.scheme.as_str(), u.port, u.path.as_str()), ("http", 80, "/"));
    assert_eq!(url("http://h?x=1").path, "/?x=1");
    assert_eq!(url("http://user:pw@h:81/").authority(), "h:81");
    assert_eq!(url("http://[::1]:8080/x").host, "::1");
    assert_eq!(url("https://h/a/./b/../c").path, "/a/c");
    assert_eq!(url("https://h/dir/file.tar.gz?x").file_name(), "file.tar.gz");
    assert!(Url::parse("http://:80/").is_err());
    assert!(Url::parse("http://h:99999/").is_err());
}

#[test]
fn resolving_references() {
    // The examples of RFC 3986, section 5.4.1.
    let base = url("http://a/b/c/d;p?q");
    for (r, want) in [
        ("g", "http://a/b/c/g"),
        ("./g", "http://a/b/c/g"),
        ("g/", "http://a/b/c/g/"),
        ("/g", "http://a/g"),
        ("//g", "http://g/"),
        ("?y", "http://a/b/c/d;p?y"),
        ("g?y", "http://a/b/c/g?y"),
        ("#s", "http://a/b/c/d;p?q#s"),
        ("g#s", "http://a/b/c/g#s"),
        (";x", "http://a/b/c/;x"),
        ("", "http://a/b/c/d;p?q"),
        (".", "http://a/b/c/"),
        ("./", "http://a/b/c/"),
        ("..", "http://a/b/"),
        ("../", "http://a/b/"),
        ("../g", "http://a/b/g"),
        ("../..", "http://a/"),
        ("../../g", "http://a/g"),
        ("../../../g", "http://a/g"),
        ("https://other:444/z", "https://other:444/z"),
    ] {
        assert_eq!(base.join(r).unwrap().to_string(), want, "reference {r:?}");
    }
    assert!(base.join("mailto:x@y").is_err());
    assert!(base.join("javascript:void(0)").is_err());
}

#[test]
fn encoding() {
    assert_eq!(form_encode("a b&c=d/é"), "a+b%26c%3Dd%2F%C3%A9");
    assert_eq!(percent_decode("a+b%26c%3Dd%2F%C3%A9", true), "a b&c=d/é");
    assert_eq!(percent_decode("100%", false), "100%");
    assert_eq!(percent_decode("%zz%4", false), "%zz%4");
}

#[test]
fn requests() {
    let r = Request::post(url("http://h:8080/form?x"), "application/x-www-form-urlencoded", b"a=1".to_vec()).header("Cookie", "k=v");
    let text = String::from_utf8(r.to_bytes()).unwrap();
    assert!(text.starts_with("POST /form?x HTTP/1.1\r\nHost: h:8080\r\nUser-Agent: Huldra/"));
    assert!(text.contains("\r\nCookie: k=v\r\nContent-Length: 3\r\n\r\na=1"));
    assert!(text.contains("Connection: close\r\n"));
}

/// Feeds a response one byte at a time and in one piece; both must agree.
fn parse(raw: &[u8], head_request: bool) -> (ResponseHead, Vec<u8>, Result<()>) {
    let mut whole = Vec::new();
    let mut p = ResponseParser::new(head_request);
    p.feed(raw, &mut |b| whole.extend_from_slice(b)).unwrap();
    let fin = p.finish();
    let mut bytewise = Vec::new();
    let mut q = ResponseParser::new(head_request);
    for b in raw {
        q.feed(core::slice::from_ref(b), &mut |d| bytewise.extend_from_slice(d)).unwrap();
    }
    assert_eq!(q.finish().is_ok(), fin.is_ok());
    assert_eq!(whole, bytewise);
    (p.head().cloned().unwrap_or_default(), whole, fin)
}

#[test]
fn response_framing() {
    let (h, body, fin) = parse(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nX-A: 1\r\n\r\nhelloEXTRA", false);
    assert_eq!((h.status, h.reason.as_str(), h.header("x-a")), (200, "OK", Some("1")));
    assert_eq!((body.as_slice(), fin), (&b"hello"[..], Ok(())));

    let (_, body, fin) = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4;ext=1\r\nWiki\r\n5\r\npedia\r\nE\r\n in\r\n\r\nchunks.\r\n0\r\nTrailer: x\r\n\r\n", false);
    assert_eq!((body.as_slice(), fin), (&b"Wikipedia in\r\n\r\nchunks."[..], Ok(())));

    let (_, body, fin) = parse(b"HTTP/1.0 200 OK\r\n\r\nuntil close", false);
    assert_eq!((body.as_slice(), fin), (&b"until close"[..], Ok(())));

    let (h, body, fin) = parse(b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 304 Not Modified\r\nContent-Length: 10\r\n\r\n", false);
    assert_eq!((h.status, body.len(), fin), (304, 0, Ok(())));

    let (_, body, _) = parse(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n", true);
    assert!(body.is_empty());
}

#[test]
fn response_errors() {
    let (_, body, fin) = parse(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort", false);
    assert_eq!(body, b"short");
    assert!(fin.unwrap_err().contains("truncated"));
    let (_, _, fin) = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nab", false);
    assert!(fin.is_err());
    let mut p = ResponseParser::new(false);
    assert!(p.feed(b"SSH-2.0-OpenSSH\r\n\r\n", &mut |_| {}).is_err());
    let mut p = ResponseParser::new(false);
    assert!(p.feed(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n", &mut |_| {}).is_err());
    assert!(ResponseParser::new(false).finish().is_err());
}

#[test]
fn redirects() {
    let (h, _, _) = parse(b"HTTP/1.1 301 Moved\r\nLocation: /new\r\nContent-Length: 0\r\n\r\n", false);
    assert!(h.is_redirect());
    assert_eq!(url("http://h/old/x").join(h.header("location").unwrap()).unwrap().to_string(), "http://h/new");
}
