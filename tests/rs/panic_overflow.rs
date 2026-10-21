fn main() {
    let mut x: u8 = 250;
    for i in 0..10 {
        println!("{}", x);
        x += i;
    }
}
