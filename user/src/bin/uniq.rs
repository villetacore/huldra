//! uniq [-c] [-d] [-u] [-i] [file]: filter repeated adjacent lines.

#![no_std]
#![no_main]

use huldra_user::io::input_lines;
use huldra_user::{env, println, String, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let (mut count, mut dups, mut uniques, mut ignore) = (false, false, false, false);
    let mut files: Vec<&str> = Vec::new();
    for a in &env::args()[1..] {
        match a.as_str() {
            "-c" => count = true,
            "-d" => dups = true,
            "-u" => uniques = true,
            "-i" => ignore = true,
            f => files.push(f),
        }
    }
    let (lines, ok) = input_lines(&files);
    let same = |a: &str, b: &str| if ignore { a.to_lowercase() == b.to_lowercase() } else { a == b };
    let mut groups: Vec<(String, usize)> = Vec::new();
    for l in lines {
        match groups.last_mut() {
            Some((prev, n)) if same(prev, &l) => *n += 1,
            _ => groups.push((l, 1)),
        }
    }
    for (l, n) in groups {
        if (dups && n < 2) || (uniques && n > 1) {
            continue;
        }
        if count {
            println!("{:>7} {}", n, l);
        } else {
            println!("{}", l);
        }
    }
    (!ok) as i32
}
