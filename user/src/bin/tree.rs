//! tree [-a] [-L depth] [dir]: show a directory tree.

#![no_std]
#![no_main]

use huldra_user::{env, fs, println, String};

huldra_user::main!(main);

struct Count {
    dirs: usize,
    files: usize,
}

fn walk(dir: &str, prefix: &str, depth: usize, max: usize, all: bool, c: &mut Count) {
    if depth >= max {
        return;
    }
    let entries: huldra_user::Vec<_> = fs::read_dir(dir).unwrap_or_default().into_iter().filter(|e| all || !e.name.starts_with('.')).collect();
    for (i, e) in entries.iter().enumerate() {
        let last = i + 1 == entries.len();
        let path = fs::join(dir, &e.name);
        let is_dir = fs::metadata(&path).map(|s| fs::is_dir(&s)).unwrap_or(false);
        let name = if is_dir { huldra_user::format!("\x1b[1;34m{}\x1b[0m", e.name) } else { e.name.clone() };
        println!("{}{}{}", prefix, if last { "└── " } else { "├── " }, name);
        if is_dir {
            c.dirs += 1;
            let next: String = huldra_user::format!("{}{}", prefix, if last { "    " } else { "│   " });
            walk(&path, &next, depth + 1, max, all, c);
        } else {
            c.files += 1;
        }
    }
}

fn main() -> i32 {
    let args = env::args();
    let mut all = false;
    let mut max = usize::MAX;
    let mut dir = String::from(".");
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-a" => all = true,
            "-L" => {
                max = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
                i += 1;
            }
            d => dir = String::from(d),
        }
        i += 1;
    }
    println!("\x1b[1;34m{}\x1b[0m", dir);
    let mut c = Count { dirs: 0, files: 0 };
    walk(&dir, "", 0, max, all, &mut c);
    println!("\n{} directories, {} files", c.dirs, c.files);
    0
}
