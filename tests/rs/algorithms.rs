// Classic programs: linked list, binary search tree, dynamic programming,
// graph search, matrices, generic containers.
use std::collections::{HashMap, VecDeque};
use std::fmt::Display;

#[derive(Debug)]
enum List<T> {
    Cons(T, Box<List<T>>),
    Nil,
}

use List::{Cons, Nil};

impl<T: Display> List<T> {
    fn render(&self) -> String {
        match self {
            Cons(v, rest) => format!("{} -> {}", v, rest.render()),
            Nil => "nil".to_string(),
        }
    }
}

#[derive(Debug, Default)]
struct Tree {
    root: Option<Box<TreeNode>>,
}

#[derive(Debug)]
struct TreeNode {
    key: i32,
    left: Option<Box<TreeNode>>,
    right: Option<Box<TreeNode>>,
}

impl Tree {
    fn insert(&mut self, key: i32) {
        let mut cur = &mut self.root;
        while let Some(node) = cur {
            cur = if key < node.key { &mut node.left } else { &mut node.right };
        }
        *cur = Some(Box::new(TreeNode { key, left: None, right: None }));
    }

    fn inorder(&self) -> Vec<i32> {
        fn walk(n: &Option<Box<TreeNode>>, out: &mut Vec<i32>) {
            if let Some(node) = n {
                walk(&node.left, out);
                out.push(node.key);
                walk(&node.right, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }

    fn height(&self) -> usize {
        fn h(n: &Option<Box<TreeNode>>) -> usize {
            match n {
                None => 0,
                Some(node) => 1 + h(&node.left).max(h(&node.right)),
            }
        }
        h(&self.root)
    }

    fn contains(&self, key: i32) -> bool {
        let mut cur = &self.root;
        while let Some(node) = cur {
            if key == node.key {
                return true;
            }
            cur = if key < node.key { &node.left } else { &node.right };
        }
        false
    }
}

fn lcs(a: &str, b: &str) -> String {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            dp[i][j] = if a[i - 1] == b[j - 1] { dp[i - 1][j - 1] + 1 } else { dp[i - 1][j].max(dp[i][j - 1]) };
        }
    }
    let (mut i, mut j) = (a.len(), b.len());
    let mut out = Vec::new();
    while i > 0 && j > 0 {
        if a[i - 1] == b[j - 1] {
            out.push(a[i - 1]);
            i -= 1;
            j -= 1;
        } else if dp[i - 1][j] >= dp[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    out.iter().rev().collect()
}

fn bfs(graph: &HashMap<u32, Vec<u32>>, start: u32, goal: u32) -> Option<Vec<u32>> {
    let mut prev: HashMap<u32, u32> = HashMap::new();
    let mut queue = VecDeque::from(vec![start]);
    let mut seen = vec![start];
    while let Some(n) = queue.pop_front() {
        if n == goal {
            let mut path = vec![goal];
            let mut cur = goal;
            while let Some(&p) = prev.get(&cur) {
                path.push(p);
                cur = p;
            }
            path.reverse();
            return Some(path);
        }
        for &m in graph.get(&n).map(|v| v.as_slice()).unwrap_or(&[]) {
            if !seen.contains(&m) {
                seen.push(m);
                prev.insert(m, n);
                queue.push_back(m);
            }
        }
    }
    None
}

fn matmul(a: &[[i64; 2]; 2], b: &[[i64; 2]; 2]) -> [[i64; 2]; 2] {
    let mut c = [[0; 2]; 2];
    for i in 0..2 {
        for j in 0..2 {
            for k in 0..2 {
                c[i][j] += a[i][k] * b[k][j];
            }
        }
    }
    c
}

fn fib_matrix(n: u32) -> i64 {
    let mut result = [[1, 0], [0, 1]];
    let mut base = [[1, 1], [1, 0]];
    let mut n = n;
    while n > 0 {
        if n & 1 == 1 {
            result = matmul(&result, &base);
        }
        base = matmul(&base, &base);
        n >>= 1;
    }
    result[0][1]
}

fn quicksort<T: PartialOrd + Clone>(v: &[T]) -> Vec<T> {
    if v.len() <= 1 {
        return v.to_vec();
    }
    let pivot = v[v.len() / 2].clone();
    let less: Vec<T> = v.iter().filter(|x| **x < pivot).cloned().collect();
    let equal: Vec<T> = v.iter().filter(|x| **x == pivot).cloned().collect();
    let greater: Vec<T> = v.iter().filter(|x| **x > pivot).cloned().collect();
    let mut out = quicksort(&less);
    out.extend(equal);
    out.extend(quicksort(&greater));
    out
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn is_palindrome(s: &str) -> bool {
    let cleaned: Vec<char> = s.chars().filter(|c| c.is_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
    cleaned.iter().eq(cleaned.iter().rev())
}

fn roman(mut n: u32) -> String {
    let table = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")];
    let mut s = String::new();
    for &(v, r) in table.iter() {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

fn main() {
    let list = Cons(1, Box::new(Cons(2, Box::new(Cons(3, Box::new(Nil))))));
    println!("{}", list.render());

    let mut t = Tree::default();
    for k in [50, 30, 70, 20, 40, 60, 80, 35] {
        t.insert(k);
    }
    println!("{:?} height {}", t.inorder(), t.height());
    println!("{} {}", t.contains(35), t.contains(36));

    println!("{}", lcs("AGGTAB", "GXTXAYB"));

    let mut g: HashMap<u32, Vec<u32>> = HashMap::new();
    for (a, b) in [(1, 2), (1, 3), (2, 4), (3, 4), (4, 5), (5, 6), (3, 7)] {
        g.entry(a).or_default().push(b);
        g.entry(b).or_default().push(a);
    }
    println!("{:?}", bfs(&g, 1, 6));
    println!("{:?}", bfs(&g, 1, 99));

    println!("{}", fib_matrix(50));
    println!("{:?}", quicksort(&[9, 4, 7, 1, 8, 2, 2, 6]));
    println!("{:?}", quicksort(&["pear", "apple", "fig"]));
    println!("{}", gcd(1071, 462));
    for s in ["A man, a plan, a canal: Panama", "hello"] {
        println!("{} {}", s, is_palindrome(s));
    }
    for n in [1994, 2024, 3999] {
        println!("{} = {}", n, roman(n));
    }
    let mut memo: HashMap<u64, u64> = HashMap::new();
    fn collatz(n: u64, memo: &mut HashMap<u64, u64>) -> u64 {
        if n == 1 {
            return 0;
        }
        if let Some(&v) = memo.get(&n) {
            return v;
        }
        let next = if n % 2 == 0 { n / 2 } else { 3 * n + 1 };
        let v = 1 + collatz(next, memo);
        memo.insert(n, v);
        v
    }
    let best = (1..10000u64).max_by_key(|&n| collatz(n, &mut memo)).unwrap();
    println!("longest collatz below 10000: {} ({} steps)", best, collatz(best, &mut memo));
    let mut primes = vec![true; 100];
    primes[0] = false;
    primes[1] = false;
    let mut i = 2;
    while i * i < 100 {
        if primes[i] {
            let mut j = i * i;
            while j < 100 {
                primes[j] = false;
                j += i;
            }
        }
        i += 1;
    }
    let count = primes.iter().filter(|&&p| p).count();
    println!("{} primes below 100", count);
}
