// Traits, generics and trait objects.
use std::fmt;

trait Shape {
    fn area(&self) -> f64;
    fn name(&self) -> &str;
}

struct Circle(f64);
struct Rect { w: f64, h: f64 }

impl Shape for Circle {
    fn area(&self) -> f64 { std::f64::consts::PI * self.0 * self.0 }
    fn name(&self) -> &str { "circle" }
}

impl Shape for Rect {
    fn area(&self) -> f64 { self.w * self.h }
    fn name(&self) -> &str { "rectangle" }
}

impl fmt::Display for dyn Shape {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:>10}: {:8.3}", self.name(), self.area())
    }
}

fn largest<'a>(shapes: &'a [Box<dyn Shape>]) -> Option<&'a Box<dyn Shape>> {
    shapes.iter().max_by(|a, b| a.area().partial_cmp(&b.area()).unwrap())
}

fn main() {
    let shapes: Vec<Box<dyn Shape>> = vec![
        Box::new(Circle(1.0)),
        Box::new(Rect { w: 3.0, h: 1.5 }),
        Box::new(Circle(0.5)),
    ];
    for s in &shapes {
        println!("{}", s);
    }
    if let Some(s) = largest(&shapes) {
        println!("largest: {}", s.name());
    }
    let total: f64 = shapes.iter().map(|s| s.area()).sum();
    println!("total area {:.2}", total);
}
