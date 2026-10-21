// Structs, impls, traits, derives, operator overloading, trait objects.
use std::fmt;
use std::ops::{Add, Mul};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }
    fn origin() -> Point {
        Point { x: 0.0, y: 0.0 }
    }
    fn dist(&self, other: &Point) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
    fn translate(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }
}

impl Mul<f64> for Point {
    type Output = Point;
    fn mul(self, k: f64) -> Point {
        Point::new(self.x * k, self.y * k)
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Person {
    name: String,
    age: u32,
}

#[derive(Debug, Default)]
struct Config {
    verbose: bool,
    level: u8,
    name: String,
    tags: Vec<String>,
}

struct Counter(u32);

impl Counter {
    fn tick(&mut self) -> u32 {
        self.0 += 1;
        self.0
    }
}

trait Shape {
    fn area(&self) -> f64;
    fn name(&self) -> String {
        String::from("shape")
    }
    fn describe(&self) -> String {
        format!("{} with area {:.2}", self.name(), self.area())
    }
}

struct Circle {
    r: f64,
}

struct Rect {
    w: f64,
    h: f64,
}

impl Shape for Circle {
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.r * self.r
    }
    fn name(&self) -> String {
        "circle".to_string()
    }
}

impl Shape for Rect {
    fn area(&self) -> f64 {
        self.w * self.h
    }
}

fn total_area(shapes: &[Box<dyn Shape>]) -> f64 {
    shapes.iter().map(|s| s.area()).sum()
}

fn print_it<T: fmt::Display>(x: T) {
    println!("[{}]", x);
}

fn largest<T: PartialOrd + Copy>(v: &[T]) -> T {
    let mut m = v[0];
    for &x in v {
        if x > m {
            m = x;
        }
    }
    m
}

struct Stack<T> {
    items: Vec<T>,
}

impl<T: fmt::Debug> Stack<T> {
    fn new() -> Self {
        Stack { items: Vec::new() }
    }
    fn push(&mut self, x: T) {
        self.items.push(x);
    }
    fn pop(&mut self) -> Option<T> {
        self.items.pop()
    }
    fn peek(&self) -> Option<&T> {
        self.items.last()
    }
    fn len(&self) -> usize {
        self.items.len()
    }
}

fn main() {
    let mut p = Point::new(1.0, 2.0);
    let q = Point::origin();
    println!("{:?} {}", p, p);
    println!("dist = {}", p.dist(&q));
    p.translate(2.0, 2.0);
    println!("{}", p);
    println!("{}", p + Point::new(1.0, 1.0));
    println!("{}", p * 2.0);
    println!("{}", p == Point::new(3.0, 4.0));

    let alice = Person {
        name: String::from("Alice"),
        age: 30,
    };
    let bob = Person {
        name: "Bob".to_string(),
        ..alice.clone()
    };
    println!("{:?}", alice);
    println!("{:#?}", bob);
    println!("{}", alice < bob);
    let mut people = vec![bob.clone(), alice.clone(), Person { name: "Carol".into(), age: 25 }];
    people.sort();
    println!("{:?}", people);
    people.sort_by(|a, b| a.age.cmp(&b.age).then(b.name.cmp(&a.name)));
    for p in &people {
        println!("{} is {}", p.name, p.age);
    }

    let c = Config::default();
    println!("{:?}", c);
    let c2 = Config {
        verbose: true,
        tags: vec!["a".to_string(), "b".to_string()],
        ..Default::default()
    };
    println!("{:#?}", c2);

    let mut counter = Counter(0);
    counter.tick();
    counter.tick();
    println!("counter = {}", counter.tick());

    let shapes: Vec<Box<dyn Shape>> = vec![Box::new(Circle { r: 1.0 }), Box::new(Rect { w: 2.0, h: 3.0 })];
    for s in &shapes {
        println!("{}", s.describe());
    }
    println!("total area = {:.3}", total_area(&shapes));

    let r: &dyn Shape = &Rect { w: 1.5, h: 2.0 };
    println!("{}", r.describe());

    print_it(42);
    print_it("text");
    print_it(p);
    println!("{}", largest(&[3, 7, 2, 9, 4]));
    println!("{}", largest(&[1.5, 0.5]));
    println!("{}", largest(&['x', 'b', 'q']));

    let mut st: Stack<i32> = Stack::new();
    for i in 1..=4 {
        st.push(i * 10);
    }
    println!("{:?} {}", st.peek(), st.len());
    while let Some(x) = st.pop() {
        print!("{} ", x);
    }
    println!();
}
