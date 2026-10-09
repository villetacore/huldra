//! Line diffs (Myers' O(ND) algorithm) and unified diff output.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Same,
    Delete,
    Insert,
}

/// The edit script turning `a` into `b`, one op per line.
pub fn diff_lines<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(Op, &'a str)> {
    // Trim common prefix and suffix first: diffs of edited files are small.
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (ma, mb) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let mut out: Vec<(Op, &str)> = a[..prefix].iter().map(|l| (Op::Same, *l)).collect();
    out.extend(myers(ma, mb));
    out.extend(a[a.len() - suffix..].iter().map(|l| (Op::Same, *l)));
    out
}

fn myers<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(Op, &'a str)> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    if max == 0 {
        return Vec::new();
    }
    let off = max as isize;
    let mut v = vec![0isize; 2 * max + 2];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    'outer: for d in 0..=max as isize {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let idx = (k + off) as usize;
            let mut x = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) { v[idx + 1] } else { v[idx - 1] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[idx] = x;
            if x >= n && y >= m {
                break 'outer;
            }
            k += 2;
        }
    }
    // Walk back through the saved states.
    let mut ops = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (0..trace.len() as isize).rev() {
        let v = &trace[d as usize];
        let k = x - y;
        let idx = (k + off) as usize;
        let prev_k = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) { k + 1 } else { k - 1 };
        let prev_x = v[(prev_k + off) as usize];
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            ops.push((Op::Same, a[(x - 1) as usize]));
            x -= 1;
            y -= 1;
        }
        if d > 0 {
            if x == prev_x {
                ops.push((Op::Insert, b[(y - 1) as usize]));
            } else {
                ops.push((Op::Delete, a[(x - 1) as usize]));
            }
        }
        x = prev_x;
        y = prev_y;
    }
    ops.reverse();
    ops
}

/// A unified diff of two texts (empty if they are equal).
pub fn unified(old_name: &str, new_name: &str, old: &str, new: &str, context: usize) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let ops = diff_lines(&a, &b);
    if ops.iter().all(|(o, _)| *o == Op::Same) {
        return String::new();
    }
    let mut out = format!("--- {}\n+++ {}\n", old_name, new_name);
    // Line numbers before each op.
    let mut pos = Vec::with_capacity(ops.len() + 1);
    let (mut ai, mut bi) = (0usize, 0usize);
    for (op, _) in &ops {
        pos.push((ai, bi));
        match op {
            Op::Same => {
                ai += 1;
                bi += 1;
            }
            Op::Delete => ai += 1,
            Op::Insert => bi += 1,
        }
    }
    pos.push((ai, bi));
    let changed: Vec<usize> = (0..ops.len()).filter(|&i| ops[i].0 != Op::Same).collect();
    let mut i = 0;
    while i < changed.len() {
        let start = changed[i].saturating_sub(context);
        let mut end = changed[i];
        while i < changed.len() && changed[i] <= end + 2 * context + 1 {
            end = changed[i];
            i += 1;
        }
        let end = (end + context + 1).min(ops.len());
        let (a0, b0) = pos[start];
        let (a1, b1) = pos[end];
        let range = |s: usize, n: usize| if n == 1 { format!("{}", s + 1) } else { format!("{},{}", if n == 0 { s } else { s + 1 }, n) };
        out.push_str(&format!("@@ -{} +{} @@\n", range(a0, a1 - a0), range(b0, b1 - b0)));
        for (op, line) in &ops[start..end] {
            out.push(match op {
                Op::Same => ' ',
                Op::Delete => '-',
                Op::Insert => '+',
            });
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}
