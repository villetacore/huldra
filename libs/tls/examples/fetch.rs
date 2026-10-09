//! Fetches a URL over HTTPS with huldra-tls, on the build machine:
//!
//!     cargo run -p huldra-tls --example fetch -- example.com [/path] [roots.pem]
//!
//! Roots default to the system bundle (Linux) or Git for Windows'.

use huldra_tls::{Client, Config, RootStore};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).map(String::as_str).unwrap_or("example.com");
    let path = args.get(2).map(String::as_str).unwrap_or("/");
    let bundle = args.get(3).cloned().unwrap_or_else(|| {
        ["/etc/ssl/certs/ca-certificates.crt", "C:/Program Files/Git/mingw64/etc/ssl/certs/ca-bundle.crt"]
            .iter()
            .find(|p| std::path::Path::new(p).exists())
            .expect("no CA bundle found")
            .to_string()
    });
    let roots = RootStore::from_pem(&std::fs::read_to_string(&bundle).unwrap());
    eprintln!("{} trusted roots from {}", roots.len(), bundle);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    let mut random = [0u8; 64];
    // Good enough for a debugging tool on the host.
    let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    for (i, b) in random.iter_mut().enumerate() {
        *b = (seed.wrapping_mul(6364136223846793005u128.wrapping_add(i as u128 * 2 + 1)) >> 64) as u8 ^ i as u8;
    }
    let (name, port) = host.split_once(':').map(|(h, p)| (h, p.parse().unwrap())).unwrap_or((host, 443));
    let mut sock = TcpStream::connect((name, port)).unwrap();
    let start = std::time::Instant::now();
    let mut tls = Client::new(name, Config { roots: &roots, now, insecure: false, alpn: &["http/1.1"] }, &random);
    tls.write(format!("GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: huldra-tls-example\r\n\r\n", path, name).as_bytes());
    let mut buf = vec![0u8; 16384];
    let mut response = Vec::new();
    let mut shown = false;
    loop {
        let out = tls.take_output();
        if !out.is_empty() {
            sock.write_all(&out).unwrap();
        }
        if tls.is_connected() && !shown {
            shown = true;
            let leaf = &tls.certificates()[0];
            eprintln!("handshake: {} in {:?}, certificate '{}' issued by '{}'", tls.cipher_suite(), start.elapsed(), leaf.subject_name(), leaf.issuer_name());
        }
        let n = sock.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        if let Err(e) = tls.feed(&buf[..n]) {
            eprintln!("TLS error: {}", e);
            std::process::exit(1);
        }
        response.extend_from_slice(&tls.read());
        if tls.is_closed() {
            break;
        }
    }
    let text = String::from_utf8_lossy(&response);
    println!("{}", text.lines().take(12).collect::<Vec<_>>().join("\n"));
    eprintln!("... {} bytes", response.len());
}
