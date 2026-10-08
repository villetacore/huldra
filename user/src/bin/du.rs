//! du [-s] [-h] [path...]: disk usage in KiB.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, format, fs, println, String, Vec};

huldra_user::main!(main);

fn human(kib: u64) -> String {
    if kib >= 1024 * 1024 {
        format!("{:.1}G", kib as f64 / 1048576.0)
    } else if kib >= 1024 {
        format!("{:.1}M", kib as f64 / 1024.0)
    } else {
        format!("{}K", kib)
    }
}

fn walk(path: &str, depth: usize, summary: bool, h: bool) -> u64 {
    let st = match fs::metadata(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("du: {}: {}", path, e);
            return 0;
        }
    };
    let mut total = (st.st_blocks as u64 * 512).div_ceil(1024).max(if st.st_size > 0 { 1 } else { 0 });
    if fs::is_dir(&st) {
        for e in fs::read_dir(path).unwrap_or_default() {
            total += walk(&fs::join(path, &e.name), depth + 1, summary, h);
        }
        if !summary || depth == 0 {
            println!("{}\t{}", if h { human(total) } else { format!("{}", total) }, path);
        }
    } else if depth == 0 {
        println!("{}\t{}", if h { human(total) } else { format!("{}", total) }, path);
    }
    total
}

fn main() -> i32 {
    let args = &env::args()[1..];
    let summary = args.iter().any(|a| a.starts_with('-') && a.contains('s'));
    let h = args.iter().any(|a| a.starts_with('-') && a.contains('h'));
    let mut paths: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if paths.is_empty() {
        paths.push(".");
    }
    for p in paths {
        walk(p, 0, summary, h);
    }
    0
}
