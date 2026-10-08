//! hexdump [file]: canonical hex+ASCII dump (like `hexdump -C`).

#![no_std]
#![no_main]

use huldra_user::io::{Reader, STDIN};
use huldra_user::{env, eprintln, print, println, sys};

huldra_user::main!(main);

fn main() -> i32 {
    let fd = match env::args().get(1) {
        Some(p) => match sys::open(p, 0, 0) {
            Ok(fd) => fd,
            Err(e) => {
                eprintln!("hexdump: {}: {}", p, e);
                return 1;
            }
        },
        None => STDIN,
    };
    let data = Reader::new(fd).read_to_end().unwrap_or_default();
    for (i, chunk) in data.chunks(16).enumerate() {
        print!("{:08x}  ", i * 16);
        for j in 0..16 {
            match chunk.get(j) {
                Some(b) => print!("{:02x} ", b),
                None => print!("   "),
            }
            if j == 7 {
                print!(" ");
            }
        }
        print!(" |");
        for &b in chunk {
            print!(
                "{}",
                if (0x20..0x7F).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            );
        }
        println!("|");
    }
    if !data.is_empty() {
        println!("{:08x}", data.len());
    }
    0
}
