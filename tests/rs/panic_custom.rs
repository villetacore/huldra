#[derive(Debug)]
struct Point { x: i32 }
fn main() {
    let p = Point { x: 3 };
    assert_eq!(p.x, 3);
    assert!(p.x > 0, "x must be positive");
    let a = 1 + 1;
    assert_eq!(a, 3, "math is broken for {:?}", p);
}
