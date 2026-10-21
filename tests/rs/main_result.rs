use std::fmt;
#[derive(Debug)]
struct AppError(String);
impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "app: {}", self.0) }
}
impl std::error::Error for AppError {}
fn step(n: i32) -> Result<i32, Box<dyn std::error::Error>> {
    if n > 2 {
        return Err(Box::new(AppError(format!("too big: {}", n))));
    }
    let v: i32 = "17".parse()?;
    Ok(v + n)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for i in 0..5 {
        let v = step(i)?;
        println!("step {} -> {}", i, v);
    }
    Ok(())
}
