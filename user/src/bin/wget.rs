//! wget [-q] [-O file] URL: downloads over HTTP (to stdout with -O -).

#![no_std]
#![no_main]

use huldra_user::abi::fs::{O_CREAT, O_TRUNC, O_WRONLY};
use huldra_user::{env, eprint, eprintln, fs, io, net, Errno, String};

huldra_user::main!(main);

fn main() -> i32 {
    let args = env::args();
    let mut out: Option<String> = None;
    let mut quiet = false;
    let mut url = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-O" => {
                i += 1;
                out = args.get(i).cloned();
            }
            "-q" => quiet = true,
            u => url = Some(u),
        }
        i += 1;
    }
    let Some(url) = url else {
        eprintln!("usage: wget [-q] [-O file] URL");
        return 2;
    };
    let out = out.unwrap_or_else(|| {
        let name = url.rsplit('/').next().unwrap_or("");
        String::from(if name.is_empty() || !url.trim_start_matches("http://").contains('/') { "index.html" } else { name })
    });
    let to_stdout = out == "-";
    let quiet = quiet || to_stdout;
    if !quiet {
        eprintln!("Connecting to {}", url);
    }
    let mut last = 0;
    let resp = net::http_get(url, |n, total| {
        if !quiet && (n - last >= 65536 || Some(n) == total) {
            last = n;
            match total {
                Some(t) => eprint!("\r{} / {} bytes ({}%)", n, t, n * 100 / t.max(1)),
                None => eprint!("\r{} bytes", n),
            }
        }
    });
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            eprintln!("\nwget: {}: {}", url, if e == Errno::ENOENT { "unknown host" } else { e.message() });
            return 1;
        }
    };
    if !quiet {
        eprintln!("\nHTTP {} , {} bytes", resp.status, resp.body.len());
    }
    if resp.status != 200 {
        eprintln!("wget: server returned HTTP {}", resp.status);
        return 1;
    }
    let written = if to_stdout {
        io::write_all(1, &resp.body)
    } else {
        fs::File::open_with(&out, O_WRONLY | O_CREAT | O_TRUNC, 0o644).and_then(|f| f.write_all(&resp.body))
    };
    if let Err(e) = written {
        eprintln!("wget: {}: {}", out, e);
        return 1;
    }
    if !quiet {
        eprintln!("saved to {}", out);
    }
    0
}
