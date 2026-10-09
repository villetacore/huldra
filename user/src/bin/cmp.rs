//! cmp [-s] FILE1 FILE2: compare two files byte by byte.

#![no_std]
#![no_main]

use huldra_user::io::read_input;
use huldra_user::{env, eprintln, println, Vec};

huldra_user::main!(main);

fn main() -> i32 {
    let args = &env::args()[1..];
    let silent = args.iter().any(|a| a == "-s");
    let files: Vec<&str> = args.iter().filter(|a| *a != "-s").map(|s| s.as_str()).collect();
    if files.len() != 2 {
        eprintln!("usage: cmp [-s] FILE1 FILE2");
        return 2;
    }
    let (a, b) = match (read_input(files[0]), read_input(files[1])) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("cmp: {}", e);
            return 2;
        }
    };
    let mut line = 1;
    for i in 0..a.len().min(b.len()) {
        if a[i] != b[i] {
            if !silent {
                println!("{} {} differ: byte {}, line {}", files[0], files[1], i + 1, line);
            }
            return 1;
        }
        if a[i] == b'\n' {
            line += 1;
        }
    }
    if a.len() != b.len() {
        if !silent {
            let shorter = if a.len() < b.len() { files[0] } else { files[1] };
            eprintln!("cmp: EOF on {}", shorter);
        }
        return 1;
    }
    0
}
