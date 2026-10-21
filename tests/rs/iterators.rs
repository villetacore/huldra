// Closures and iterator adapters.

fn apply<F: Fn(i32) -> i32>(f: F, x: i32) -> i32 {
    f(x)
}

fn apply_mut<F: FnMut()>(mut f: F, n: usize) {
    for _ in 0..n {
        f();
    }
}

fn make_adder(n: i32) -> impl Fn(i32) -> i32 {
    move |x| x + n
}

fn make_counter() -> Box<dyn FnMut() -> u32> {
    let mut count = 0;
    Box::new(move || {
        count += 1;
        count
    })
}

fn compose<A, B, C>(f: impl Fn(A) -> B, g: impl Fn(B) -> C) -> impl Fn(A) -> C {
    move |x| g(f(x))
}

fn evens() -> impl Iterator<Item = u32> {
    (0..).step_by(2)
}

struct Fib {
    a: u64,
    b: u64,
}

impl Iterator for Fib {
    type Item = u64;
    fn next(&mut self) -> Option<u64> {
        let r = self.a;
        self.a = self.b;
        self.b += r;
        Some(r)
    }
}

fn main() {
    let double = |x| x * 2;
    println!("{}", apply(double, 21));
    let offset = 10;
    println!("{}", apply(|x| x + offset, 5));
    let mut total = 0;
    apply_mut(|| total += 3, 4);
    println!("total = {}", total);
    let add5 = make_adder(5);
    println!("{}", add5(1));
    let mut c = make_counter();
    c();
    c();
    println!("counter = {}", c());
    let f = compose(|x: i32| x + 1, |y: i32| y * 10);
    println!("{}", f(4));
    let fp: fn(i32) -> i32 = |x| x - 1;
    println!("{}", fp(1));

    let v: Vec<i32> = (1..=10).collect();
    let squares: Vec<i32> = v.iter().map(|x| x * x).collect();
    println!("{:?}", squares);
    let evens_sum: i32 = v.iter().filter(|&&x| x % 2 == 0).sum();
    println!("{}", evens_sum);
    println!("{}", v.iter().product::<i32>());
    println!("{:?}", v.iter().max());
    println!("{:?}", v.iter().min_by_key(|x| (*x - 5).abs()));
    println!("{}", v.iter().any(|&x| x > 9));
    println!("{}", v.iter().all(|&x| x > 0));
    println!("{:?}", v.iter().position(|&x| x == 7));
    println!("{}", v.iter().rev().take(3).map(|x| x.to_string()).collect::<Vec<_>>().join(","));
    let (small, big): (Vec<i32>, Vec<i32>) = v.iter().partition(|&&x| x < 4);
    println!("{:?} {:?}", small, big);
    for (i, x) in v.iter().enumerate().skip(7) {
        println!("{} -> {}", i, x);
    }
    let pairs: Vec<(i32, char)> = v.iter().cloned().zip("abc".chars()).collect();
    println!("{:?}", pairs);
    let folded = v.iter().fold(String::new(), |acc, x| acc + &x.to_string());
    println!("{}", folded);
    let words = vec!["apple", "banana", "cherry"];
    let lens: Vec<usize> = words.iter().map(|w| w.len()).collect();
    println!("{:?}", lens);
    let flat: Vec<char> = words.iter().flat_map(|w| w.chars().take(2)).collect();
    println!("{:?}", flat);
    let nested = vec![vec![1, 2], vec![3], vec![]];
    let all: Vec<i32> = nested.into_iter().flatten().collect();
    println!("{:?}", all);
    println!("{:?}", evens().skip(3).take(4).collect::<Vec<_>>());
    println!("{:?}", Fib { a: 0, b: 1 }.take_while(|&x| x < 100).collect::<Vec<_>>());
    let mut it = v.iter().peekable();
    while let Some(x) = it.next() {
        if let Some(&&next) = it.peek() {
            if next % 5 == 0 {
                print!("{}| ", x);
                continue;
            }
        }
        print!("{} ", x);
    }
    println!();
    let chunks: Vec<Vec<i32>> = v.chunks(3).map(|c| c.to_vec()).collect();
    println!("{:?}", chunks);
    let w: Vec<i32> = v.windows(2).map(|w| w[1] - w[0]).collect();
    println!("{:?}", w);
    let mut sorted = vec![5, 3, 8, 1, 9, 2];
    sorted.sort_by(|a, b| b.cmp(a));
    println!("{:?}", sorted);
    sorted.sort_by_key(|x| x % 3);
    println!("{:?}", sorted);
    sorted.dedup();
    sorted.retain(|&x| x != 8);
    println!("{:?} {}", sorted, sorted.contains(&9));
    let count = (1..100).filter(|n| n % 7 == 0).count();
    println!("{}", count);
    let s: u64 = (1..=20u64).map(|x| x * x).filter(|x| x % 2 == 1).sum();
    println!("{}", s);
    let last = v.iter().copied().reduce(|a, b| a.max(b));
    println!("{:?}", last);
    let mut fns: Vec<Box<dyn Fn(i32) -> i32>> = Vec::new();
    for k in 1..=3 {
        fns.push(Box::new(move |x| x * k));
    }
    println!("{:?}", fns.iter().map(|f| f(10)).collect::<Vec<_>>());
    let strs: Vec<String> = vec![3, 1, 2].iter().map(|n| format!("<{}>", n)).collect();
    println!("{}", strs.concat());
    let chained: Vec<i32> = (1..3).chain(7..9).collect();
    println!("{:?}", chained);
    let mx = ["aa", "b", "cccc", "dd"].iter().max_by_key(|s| s.len()).unwrap();
    println!("{}", mx);
    let stepped: Vec<i32> = (0..20).step_by(5).collect();
    println!("{:?}", stepped);
    let mut acc = vec![];
    (1..4).for_each(|x| acc.push(x * 100));
    println!("{:?}", acc);
    let found = v.iter().find_map(|&x| if x > 4 { Some(x * 3) } else { None });
    println!("{:?}", found);
    println!("{:?}", v.iter().skip_while(|&&x| x < 8).collect::<Vec<_>>());
    println!("{:?}", v.iter().scan(0, |s, &x| { *s += x; Some(*s) }).collect::<Vec<_>>());
    println!("{:?}", v.iter().last());
}
