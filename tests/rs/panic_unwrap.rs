fn find(x: i32) -> Option<i32> {
    if x > 0 { Some(x) } else { None }
}
fn main() {
    println!("{}", find(3).unwrap());
    print!("partial line ");
    let r: Result<i32, String> = Err("boom".to_string());
    if find(1).is_some() {
        r.expect("it failed");
    }
}
