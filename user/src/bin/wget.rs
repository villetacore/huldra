//! wget [options] URL...: download files over HTTP and HTTPS.
//!
//!   -O FILE           save as FILE (- for standard output)
//!   -P DIR            save into DIR
//!   -c                continue a partial download (HTTP Range)
//!   -q                quiet: no progress
//!   -S                print the response headers
//!   --header 'K: V'   add a request header (may repeat)
//!   --post-data DATA  send a POST request with DATA as the body
//!   --compressed      ask for gzip and decode it
//!   --insecure        do not check HTTPS certificates
//!
//! Redirects are followed. HTTPS uses TLS 1.3; certificates are checked
//! against /etc/ssl/certs/ca-certificates.crt and the system clock.

#![no_std]
#![no_main]

use huldra_http::{Request, Url};
use huldra_user::abi::fs::{O_APPEND, O_CREAT, O_TRUNC, O_WRONLY};
use huldra_user::{env, eprint, eprintln, format, fs, http, io, time, String, ToString, Vec};

huldra_user::main!(main);

struct Args {
    output: Option<String>,
    dir: Option<String>,
    resume: bool,
    quiet: bool,
    show_headers: bool,
    headers: Vec<(String, String)>,
    post: Option<String>,
    insecure: bool,
    compressed: bool,
    urls: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let argv = env::args();
    let mut a = Args { output: None, dir: None, resume: false, quiet: false, show_headers: false, headers: Vec::new(), post: None, insecure: false, compressed: false, urls: Vec::new() };
    let mut i = 1;
    let value = |i: &mut usize, flag: &str| -> Result<String, String> {
        *i += 1;
        argv.get(*i).cloned().ok_or_else(|| format!("{} needs a value", flag))
    };
    while i < argv.len() {
        match argv[i].as_str() {
            "-O" => a.output = Some(value(&mut i, "-O")?),
            "-P" => a.dir = Some(value(&mut i, "-P")?),
            "-c" => a.resume = true,
            "-q" => a.quiet = true,
            "-S" => a.show_headers = true,
            "--insecure" | "--no-check-certificate" => a.insecure = true,
            "--compressed" => a.compressed = true,
            "--header" => {
                let h = value(&mut i, "--header")?;
                let (k, v) = h.split_once(':').ok_or("--header wants 'Name: value'")?;
                a.headers.push((k.trim().to_string(), v.trim().to_string()));
            }
            "--post-data" => a.post = Some(value(&mut i, "--post-data")?),
            s if s.starts_with('-') && s != "-" => return Err(format!("unknown option {}", s)),
            u => a.urls.push(u.to_string()),
        }
        i += 1;
    }
    if a.urls.is_empty() {
        return Err("no URL given".into());
    }
    if a.output.is_some() && a.urls.len() > 1 && a.output.as_deref() != Some("-") {
        return Err("-O with several URLs".into());
    }
    Ok(a)
}

fn human(n: u64) -> String {
    match n {
        0..=9_999 => format!("{} B", n),
        10_000..=9_999_999 => format!("{}.{} KiB", n / 1024, n % 1024 * 10 / 1024),
        _ => format!("{}.{} MiB", n >> 20, (n & 0xFFFFF) * 10 >> 20),
    }
}

/// Progress line: bytes, percent and a bar, speed, time left.
struct Progress {
    start: u64,
    last: u64,
    offset: u64,
    quiet: bool,
}

impl Progress {
    fn show(&mut self, done: u64, total: Option<u64>, last_call: bool) {
        let now = time::uptime_ms();
        if self.quiet || (!last_call && now - self.last < 200) {
            return;
        }
        self.last = now;
        let elapsed = (now - self.start).max(1);
        let speed = done * 1000 / elapsed;
        let done_all = done + self.offset;
        let line = match total.map(|t| t + self.offset) {
            Some(t) if t > 0 => {
                let pct = (done_all * 100 / t).min(100);
                let filled = (pct / 5) as usize;
                let bar: String = core::iter::repeat_n('=', filled).chain(core::iter::repeat_n(' ', 20 - filled)).collect();
                let eta = if speed > 0 && t > done_all { format!("  {}s left", (t - done_all) / speed) } else { String::new() };
                format!("{:>3}% [{}] {} / {}  {}/s{}", pct, bar, human(done_all), human(t), human(speed), eta)
            }
            _ => format!("{}  {}/s", human(done_all), human(speed)),
        };
        eprint!("\r{}\x1b[K", line);
    }
}

fn file_name(url: &Url, a: &Args) -> String {
    let base = match url.file_name() {
        "" => "index.html".to_string(),
        n => huldra_http::percent_decode(n, false).replace('/', "_"),
    };
    match &a.dir {
        Some(d) => fs::join(d, &base),
        None => base,
    }
}

fn download(url_text: &str, a: &Args) -> Result<(), String> {
    let url = Url::parse(url_text)?;
    let out = a.output.clone().unwrap_or_else(|| file_name(&url, a));
    let to_stdout = out == "-";
    let quiet = a.quiet || to_stdout;
    let offset = if a.resume && !to_stdout { fs::metadata(&out).map(|s| s.st_size as u64).unwrap_or(0) } else { 0 };
    let mut req = match &a.post {
        Some(data) => Request::post(url.clone(), "application/x-www-form-urlencoded", data.as_bytes().to_vec()),
        None => Request::get(url.clone()),
    };
    if offset > 0 {
        req = req.header("Range", &format!("bytes={}-", offset));
    }
    let opts = http::Options { insecure: a.insecure, headers: a.headers.clone(), gzip: a.compressed, ..http::Options::default() };
    if !quiet {
        eprintln!("--> {}", url);
    }
    // The file is opened with the first body byte of a successful answer:
    // appended to for 206 Partial Content, replaced otherwise.
    let mut file: Option<fs::File> = None;
    let mut written = 0u64;
    let mut progress = Progress { start: time::uptime_ms(), last: 0, offset: 0, quiet };
    let partial = core::cell::Cell::new(false);
    let result = http::fetch(req, &opts, &mut |head, b| {
        if !(200..300).contains(&head.status) {
            return Ok(()); // an error page: not saved
        }
        partial.set(head.status == 206);
        if file.is_none() && !to_stdout {
            let flags = O_WRONLY | O_CREAT | if head.status == 206 { O_APPEND } else { O_TRUNC };
            file = Some(fs::File::open_with(&out, flags, 0o644).map_err(|e| format!("{}: {}", out, e))?);
        }
        written += b.len() as u64;
        match &file {
            Some(f) => f.write_all(b).map_err(|e| format!("{}: {}", out, e)),
            None => io::write_all(1, b).map_err(|e| format!("stdout: {}", e)),
        }
    }, &mut |n, total| {
        progress.offset = if partial.get() { offset } else { 0 };
        progress.show(n, total, false);
    });
    let (final_url, head) = result?;
    let status = head.status;
    if !quiet {
        progress.show(written, head.content_length(), true);
        eprintln!();
    }
    if a.show_headers || (!quiet && final_url != url) {
        if final_url != url {
            eprintln!("    redirected to {}", final_url);
        }
    }
    if a.show_headers {
        eprintln!("  HTTP {}", http::status_text(&head));
        for (k, v) in &head.headers {
            eprintln!("  {}: {}", k, v);
        }
    }
    match status {
        200..=299 => {
            if !quiet {
                eprintln!("saved {} ({})", out, human(written + if status == 206 { offset } else { 0 }));
            }
            Ok(())
        }
        416 if offset > 0 => {
            if !quiet {
                eprintln!("{} is already complete", out);
            }
            Ok(())
        }
        _ => Err(format!("server answered {}", http::status_text(&head))),
    }
}

fn main() -> i32 {
    let a = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("wget: {}\nusage: wget [-O FILE] [-P DIR] [-c] [-q] [-S] [--header 'K: V'] [--post-data DATA] [--compressed] [--insecure] URL...", e);
            return 2;
        }
    };
    let mut status = 0;
    for u in &a.urls {
        if let Err(e) = download(u, &a) {
            eprintln!("\nwget: {}: {}", u, e);
            status = 1;
        }
    }
    status
}
