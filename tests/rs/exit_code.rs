use std::process;
fn main() {
    println!("exiting");
    eprintln!("to stderr");
    process::exit(3);
}
