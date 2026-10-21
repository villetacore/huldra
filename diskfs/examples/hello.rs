// rustc -run /usr/share/huldra/examples/hello.rs [name]
use std::env;

fn main() {
    let name = env::args().nth(1).unwrap_or_else(|| String::from("world"));
    println!("Hello, {}!", name);
}
