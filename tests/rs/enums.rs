// Enums, pattern matching, Option and Result, the ? operator.
use std::collections::HashMap;
use std::fmt;
use std::num::ParseIntError;

#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Circle(f64),
    Rect { w: f64, h: f64 },
    Unit,
}

impl Shape {
    fn area(&self) -> f64 {
        match self {
            Shape::Circle(r) => 3.0 * r * r,
            Shape::Rect { w, h } => w * h,
            Shape::Unit => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Color {
    Red,
    Green,
    Blue,
}

#[derive(Debug)]
enum Expr {
    Num(i64),
    Add(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
}

fn eval(e: &Expr) -> i64 {
    match e {
        Expr::Num(n) => *n,
        Expr::Add(a, b) => eval(a) + eval(b),
        Expr::Mul(a, b) => eval(a) * eval(b),
        Expr::Neg(a) => -eval(a),
    }
}

#[derive(Debug)]
enum MyError {
    Parse(ParseIntError),
    Negative(i64),
    Empty,
}

impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MyError::Parse(e) => write!(f, "parse error: {}", e),
            MyError::Negative(n) => write!(f, "{} is negative", n),
            MyError::Empty => write!(f, "empty input"),
        }
    }
}

impl From<ParseIntError> for MyError {
    fn from(e: ParseIntError) -> Self {
        MyError::Parse(e)
    }
}

fn parse_positive(s: &str) -> Result<i64, MyError> {
    if s.is_empty() {
        return Err(MyError::Empty);
    }
    let n: i64 = s.trim().parse()?;
    if n < 0 {
        Err(MyError::Negative(n))
    } else {
        Ok(n)
    }
}

fn sum_all(items: &[&str]) -> Result<i64, MyError> {
    let mut total = 0;
    for s in items {
        total += parse_positive(s)?;
    }
    Ok(total)
}

fn first_even(v: &[i32]) -> Option<i32> {
    let x = v.iter().find(|x| **x % 2 == 0)?;
    Some(*x * 10)
}

fn describe(p: (i32, i32)) -> String {
    match p {
        (0, 0) => "origin".to_string(),
        (x, 0) | (0, x) => format!("on an axis at {}", x),
        (x, y) if x == y => format!("diagonal {}", x),
        (x, _) if x < 0 => "left".to_string(),
        _ => "elsewhere".to_string(),
    }
}

fn slices(v: &[i32]) -> String {
    match v {
        [] => "empty".to_string(),
        [x] => format!("one: {}", x),
        [first, .., last] if first == last => format!("same ends {}", first),
        [first, rest @ ..] => format!("first {} then {} more", first, rest.len()),
    }
}

fn main() {
    let shapes = vec![Shape::Circle(1.0), Shape::Rect { w: 2.0, h: 3.5 }, Shape::Unit];
    for s in &shapes {
        println!("{:?} area {}", s, s.area());
    }
    println!("{}", shapes[0] == Shape::Circle(1.0));

    let mut counts: HashMap<Color, u32> = HashMap::new();
    for c in [Color::Red, Color::Blue, Color::Red, Color::Green, Color::Red] {
        *counts.entry(c).or_insert(0) += 1;
    }
    let mut keys: Vec<_> = counts.keys().copied().collect();
    keys.sort();
    for k in keys {
        println!("{:?}: {}", k, counts[&k]);
    }
    println!("{}", Color::Green as i32);

    let e = Expr::Add(Box::new(Expr::Num(2)), Box::new(Expr::Mul(Box::new(Expr::Num(3)), Box::new(Expr::Neg(Box::new(Expr::Num(4)))))));
    println!("{:?} = {}", e, eval(&e));

    for input in ["42", " 7 ", "-3", "abc", ""] {
        match parse_positive(input) {
            Ok(n) => println!("ok {}", n),
            Err(e) => println!("error: {} / {:?}", e, e),
        }
    }
    println!("{:?}", sum_all(&["1", "2", "3"]).unwrap());
    println!("{}", sum_all(&["1", "x"]).is_err());

    println!("{:?} {:?}", first_even(&[1, 3, 4, 6]), first_even(&[1, 3]));

    let maybe: Option<&str> = Some("hello");
    if let Some(s) = maybe {
        println!("got {}", s);
    }
    let nothing: Option<i32> = None;
    println!("{}", nothing.map(|x| x * 2).unwrap_or(-1));
    println!("{:?}", maybe.map(|s| s.len()));
    println!("{:?}", "12".parse::<u8>().ok().and_then(|x| x.checked_mul(30)));
    let r: Result<u32, String> = Err("bad".into());
    println!("{:?} {}", r, r.clone().unwrap_or_default());
    let Some(v) = Some(5) else { panic!() };
    println!("{}", v);

    for p in [(0, 0), (5, 0), (0, -2), (3, 3), (-1, 7), (4, 9)] {
        println!("{:?} -> {}", p, describe(p));
    }
    for v in [&[][..], &[1], &[2, 5, 2], &[1, 2, 3, 4]] {
        println!("{}", slices(v));
    }

    let n = 42;
    let size = match n {
        0 => "zero",
        1..=9 => "digit",
        10..=99 => "two digits",
        _ => "big",
    };
    println!("{}", size);
    let c = 'k';
    println!("{}", matches!(c, 'a'..='z'));
    let pair = (1, "x");
    let (a, b) = pair;
    println!("{} {}", a, b);
    let msg = match &shapes[1] {
        Shape::Rect { w, .. } if *w > 1.0 => "wide",
        _ => "narrow",
    };
    println!("{}", msg);
}
