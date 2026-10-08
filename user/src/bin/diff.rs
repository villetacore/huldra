//! diff [-u] FILE1 FILE2: line differences (LCS), normal or unified format.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use huldra_user::io::read_input;
use huldra_user::{env, eprintln, println};

huldra_user::main!(main);

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Same(usize, usize),
    Del(usize),
    Add(usize),
}

fn lines(data: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(data);
    let mut v: Vec<String> = text.split('\n').map(String::from).collect();
    if text.ends_with('\n') {
        v.pop();
    }
    v
}

fn diff(a: &[String], b: &[String]) -> Vec<Op> {
    // Trim common prefix/suffix, then classic LCS table on the middle.
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && a[pre] == b[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < a.len() - pre && suf < b.len() - pre && a[a.len() - 1 - suf] == b[b.len() - 1 - suf] {
        suf += 1;
    }
    let (am, bm) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let (n, m) = (am.len(), bm.len());
    let mut t = vec![0u32; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i * (m + 1) + j] = if am[i] == bm[j] {
                t[(i + 1) * (m + 1) + j + 1] + 1
            } else {
                t[(i + 1) * (m + 1) + j].max(t[i * (m + 1) + j + 1])
            };
        }
    }
    let mut ops: Vec<Op> = (0..pre).map(|i| Op::Same(i, i)).collect();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && am[i] == bm[j] {
            ops.push(Op::Same(pre + i, pre + j));
            i += 1;
            j += 1;
        } else if j < m && (i == n || t[i * (m + 1) + j + 1] >= t[(i + 1) * (m + 1) + j]) {
            ops.push(Op::Add(pre + j));
            j += 1;
        } else {
            ops.push(Op::Del(pre + i));
            i += 1;
        }
    }
    for k in 0..suf {
        ops.push(Op::Same(a.len() - suf + k, b.len() - suf + k));
    }
    ops
}

fn range(start: usize, count: usize) -> String {
    match count {
        0 => alloc::format!("{}", start),
        1 => alloc::format!("{}", start + 1),
        _ => alloc::format!("{},{}", start + 1, start + count),
    }
}

fn main() -> i32 {
    let args = &env::args()[1..];
    let unified = args.iter().any(|a| a == "-u");
    let files: Vec<&str> = args.iter().filter(|a| *a != "-u").map(|s| s.as_str()).collect();
    if files.len() != 2 {
        eprintln!("usage: diff [-u] FILE1 FILE2");
        return 2;
    }
    let (a, b) = match (read_input(files[0]), read_input(files[1])) {
        (Ok(a), Ok(b)) => (lines(&a), lines(&b)),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("diff: {}", e);
            return 2;
        }
    };
    let ops = diff(&a, &b);
    if ops.iter().all(|o| matches!(o, Op::Same(..))) {
        return 0;
    }
    if unified {
        println!("--- {}\n+++ {}", files[0], files[1]);
        let context = 3;
        let changed: Vec<usize> = ops.iter().enumerate().filter(|(_, o)| !matches!(o, Op::Same(..))).map(|(i, _)| i).collect();
        let mut k = 0;
        while k < changed.len() {
            let start = changed[k].saturating_sub(context);
            let mut end = changed[k];
            while k + 1 < changed.len() && changed[k + 1] <= end + 2 * context + 1 {
                k += 1;
                end = changed[k];
            }
            let end = (end + context + 1).min(ops.len());
            let hunk = &ops[start..end];
            let a_start = hunk.iter().find_map(|o| match o { Op::Same(i, _) | Op::Del(i) => Some(*i), _ => None }).unwrap_or(0);
            let b_start = hunk.iter().find_map(|o| match o { Op::Same(_, j) | Op::Add(j) => Some(*j), _ => None }).unwrap_or(0);
            let a_len = hunk.iter().filter(|o| !matches!(o, Op::Add(_))).count();
            let b_len = hunk.iter().filter(|o| !matches!(o, Op::Del(_))).count();
            println!("\x1b[36m@@ -{},{} +{},{} @@\x1b[0m", a_start + 1, a_len, b_start + 1, b_len);
            for o in hunk {
                match o {
                    Op::Same(i, _) => println!(" {}", a[*i]),
                    Op::Del(i) => println!("\x1b[31m-{}\x1b[0m", a[*i]),
                    Op::Add(j) => println!("\x1b[32m+{}\x1b[0m", b[*j]),
                }
            }
            k += 1;
        }
        return 1;
    }
    // Normal format: group consecutive changes.
    let mut i = 0;
    while i < ops.len() {
        if matches!(ops[i], Op::Same(..)) {
            i += 1;
            continue;
        }
        let (mut dels, mut adds) = (Vec::new(), Vec::new());
        while i < ops.len() && !matches!(ops[i], Op::Same(..)) {
            match ops[i] {
                Op::Del(x) => dels.push(x),
                Op::Add(y) => adds.push(y),
                _ => {}
            }
            i += 1;
        }
        let a_pos = dels.first().copied().unwrap_or_else(|| ops[..i].iter().rev().find_map(|o| if let Op::Same(x, _) = o { Some(x + 1) } else { None }).unwrap_or(0));
        let b_pos = adds.first().copied().unwrap_or_else(|| ops[..i].iter().rev().find_map(|o| if let Op::Same(_, y) = o { Some(y + 1) } else { None }).unwrap_or(0));
        let kind = if dels.is_empty() { 'a' } else if adds.is_empty() { 'd' } else { 'c' };
        let left = if kind == 'a' { range(a_pos, 0) } else { range(a_pos, dels.len()) };
        let right = if kind == 'd' { range(b_pos, 0) } else { range(b_pos, adds.len()) };
        println!("{}{}{}", left, kind, right);
        for x in &dels {
            println!("< {}", a[*x]);
        }
        if kind == 'c' {
            println!("---");
        }
        for y in &adds {
            println!("> {}", b[*y]);
        }
    }
    1
}
