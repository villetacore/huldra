// Variables, arithmetic, control flow, functions, tuples and arrays.

fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn factorial(n: u64) -> u64 {
    if n <= 1 {
        1
    } else {
        n * factorial(n - 1)
    }
}

fn fib(n: u32) -> u64 {
    let (mut a, mut b) = (0u64, 1u64);
    for _ in 0..n {
        let t = a + b;
        a = b;
        b = t;
    }
    a
}

fn swap(p: (i32, &str)) -> (&str, i32) {
    (p.1, p.0)
}

fn classify(n: i32) -> &'static str {
    match n {
        i32::MIN..=-1 => "negative",
        0 => "zero",
        1 | 2 | 3 => "small",
        x if x % 2 == 0 => "even",
        _ => "odd",
    }
}

fn main() {
    let x = 5;
    let y = 7;
    println!("{} + {} = {}", x, y, add(x, y));
    let x = x * 2;
    println!("shadowed x = {x}");

    let mut total = 0;
    let mut i = 0;
    while i < 10 {
        total += i;
        i += 1;
    }
    println!("total = {}", total);

    let mut n = 0;
    let found = loop {
        n += 1;
        if n * n > 50 {
            break n;
        }
    };
    println!("first square over 50: {}^2", found);

    'outer: for a in 1..10 {
        for b in 1..10 {
            if a * b == 42 {
                println!("{} * {} = 42", a, b);
                break 'outer;
            }
        }
    }

    for i in (1..=5).rev() {
        print!("{} ", i);
    }
    println!();

    println!("10! = {}", factorial(10));
    println!("fib(50) = {}", fib(50));

    let t = swap((1, "one"));
    println!("{:?}", t);

    let arr = [3, 1, 4, 1, 5, 9, 2, 6];
    let mut sum = 0;
    for v in arr.iter() {
        sum += v;
    }
    println!("sum = {}, len = {}, first = {}, last = {}", sum, arr.len(), arr[0], arr[arr.len() - 1]);
    let zeros = [0u8; 4];
    println!("{:?}", zeros);

    for n in [-5, 0, 2, 10, 13] {
        println!("{} is {}", n, classify(n));
    }

    let c = 'A';
    let b = true && !false;
    let f = 2.5f64 * 4.0;
    println!("{} {} {}", c, b, f);
    println!("{}", 17 / 5);
    println!("{}", 17 % 5);
    println!("{}", -17 / 5);
    println!("{}", -17 % 5);
    println!("{}", 1u32 << 10);
    println!("{}", 0xFFu8 as i8);
    println!("{}", 300i32 as u8);
    println!("{}", -1i64 as u64);
    println!("{}", 3.99f64 as i32);
    println!("{}", (65u8 as char));
    println!("{}", 'z' as u32);
    let big: i64 = 1 << 40;
    println!("{}", big);
    let maybe_negative = if big > 0 { "positive" } else { "not positive" };
    println!("{}", maybe_negative);
}
