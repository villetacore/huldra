//! httpd [-p port] [dir]: a small HTTP/1.0 file server (default port 80,
//! directory /usr/share/huldra). Directories get an index page.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, format, fs, net, println, String};

huldra_user::main!(main);

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" | "htm" => "text/html",
        "txt" | "md" | "c" | "h" | "sh" | "rs" => "text/plain; charset=utf-8",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "tar" | "pkg" => "application/x-tar",
        _ => "application/octet-stream",
    }
}

fn respond(conn: &net::Socket, status: &str, ctype: &str, body: &[u8]) {
    let head = format!("HTTP/1.0 {}\r\nServer: huldra-httpd\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", status, ctype, body.len());
    let _ = conn.send_all(head.as_bytes());
    let _ = conn.send_all(body);
}

fn serve(conn: &net::Socket, root: &str) -> String {
    let mut req = huldra_user::Vec::new();
    let mut buf = [0u8; 2048];
    while !req.windows(4).any(|w| w == b"\r\n\r\n") && req.len() < 16384 {
        match conn.recv(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => req.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&req).into_owned();
    let line = String::from(text.lines().next().unwrap_or(""));
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    if method != "GET" && method != "HEAD" {
        respond(conn, "405 Method Not Allowed", "text/plain", b"method not allowed\n");
        return line;
    }
    let path = target.split('?').next().unwrap_or("/");
    if path.split('/').any(|c| c == "..") {
        respond(conn, "403 Forbidden", "text/plain", b"forbidden\n");
        return line;
    }
    let full = format!("{}{}", root.trim_end_matches('/'), path);
    match fs::metadata(&full) {
        Ok(st) if fs::is_dir(&st) => {
            let mut page = format!("<html><head><title>{0}</title></head><body><h1>Index of {0}</h1><ul>\n", path);
            let mut entries = fs::read_dir(&full).unwrap_or_default();
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            for e in entries {
                let slash = if e.is_dir() { "/" } else { "" };
                page.push_str(&format!("<li><a href=\"{}{}{}\">{}{}</a></li>\n", huldra_user::format!("{}/", path.trim_end_matches('/')), e.name, slash, e.name, slash));
            }
            page.push_str("</ul></body></html>\n");
            respond(conn, "200 OK", "text/html", page.as_bytes());
        }
        Ok(_) => match fs::read(&full) {
            Ok(data) => respond(conn, "200 OK", content_type(&full), if method == "HEAD" { &[] } else { &data }),
            Err(_) => respond(conn, "403 Forbidden", "text/plain", b"cannot read\n"),
        },
        Err(_) => respond(conn, "404 Not Found", "text/plain", b"not found\n"),
    }
    line
}

fn main() -> i32 {
    let args = env::args();
    let mut port = 80u16;
    let mut root = String::from("/usr/share/huldra");
    let mut i = 1;
    while i < args.len() {
        if args[i] == "-p" {
            i += 1;
            port = args.get(i).and_then(|p| p.parse().ok()).unwrap_or(80);
        } else {
            root = args[i].clone();
        }
        i += 1;
    }
    let listener = match net::Socket::tcp().and_then(|s| s.bind(net::Ip::UNSPECIFIED, port).and_then(|_| s.listen(16)).map(|_| s)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("httpd: port {}: {}", port, e);
            return 1;
        }
    };
    println!("httpd: serving {} on port {}", root, port);
    loop {
        match listener.accept() {
            Ok((conn, ip, _)) => {
                let line = serve(&conn, &root);
                println!("{} \"{}\"", ip, line);
            }
            Err(e) => {
                eprintln!("httpd: accept: {}", e);
                return 1;
            }
        }
    }
}
