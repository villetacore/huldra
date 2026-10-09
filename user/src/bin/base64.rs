//! base64 [-d] [file]: encode (or with -d decode) base64.

#![no_std]
#![no_main]

use huldra_user::io::{read_input, write_all, STDOUT};
use huldra_user::{env, eprintln, String, Vec};

huldra_user::main!(main);

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode(data: &[u8]) -> String {
    let mut out = String::new();
    for (i, chunk) in data.chunks(3).enumerate() {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
        if (i + 1) % 19 == 0 {
            out.push('\n');
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn decode(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for &c in text {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn main() -> i32 {
    let args = &env::args()[1..];
    let dec = args.iter().any(|a| a == "-d");
    let file = args.iter().find(|a| *a != "-d").map_or("-", |s| s.as_str());
    let data = match read_input(file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("base64: {}: {}", file, e);
            return 1;
        }
    };
    if dec {
        match decode(&data) {
            Some(d) => {
                let _ = write_all(STDOUT, &d);
                0
            }
            None => {
                eprintln!("base64: invalid input");
                1
            }
        }
    } else {
        let _ = write_all(STDOUT, encode(&data).as_bytes());
        0
    }
}
