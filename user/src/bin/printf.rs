//! printf FORMAT [args...]: %s %d %i %x %o %c %% with width/flags; escapes.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, print, String};

huldra_user::main!(main);

fn pad(s: String, width: usize, left: bool, zero: bool) -> String {
    let len = s.chars().count();
    if len >= width {
        return s;
    }
    let fill: String = core::iter::repeat_n(if zero && !left { '0' } else { ' ' }, width - len).collect();
    if left {
        s + &fill
    } else if zero && s.starts_with('-') {
        huldra_user::format!("-{}{}", fill, &s[1..])
    } else {
        fill + &s
    }
}

fn main() -> i32 {
    let args = env::args();
    let Some(format) = args.get(1) else {
        eprintln!("usage: printf FORMAT [args...]");
        return 2;
    };
    let mut values = args[2..].iter();
    let chars: huldra_user::Vec<char> = format.chars().collect();
    // The format is reused while arguments remain.
    loop {
        let mut out = String::new();
        let mut used = false;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '\\' && i + 1 < chars.len() {
                out.push(match chars[i + 1] {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    'e' => '\x1b',
                    '\\' => '\\',
                    o => o,
                });
                i += 2;
                continue;
            }
            if c != '%' || i + 1 >= chars.len() {
                out.push(c);
                i += 1;
                continue;
            }
            i += 1;
            if chars[i] == '%' {
                out.push('%');
                i += 1;
                continue;
            }
            let (mut left, mut zero) = (false, false);
            while i < chars.len() && matches!(chars[i], '-' | '0') {
                if chars[i] == '-' {
                    left = true;
                } else {
                    zero = true;
                }
                i += 1;
            }
            let mut width = 0usize;
            while i < chars.len() && chars[i].is_ascii_digit() {
                width = width * 10 + chars[i].to_digit(10).unwrap() as usize;
                i += 1;
            }
            if i >= chars.len() {
                break;
            }
            let arg = values.next().map(String::as_str).unwrap_or("");
            used = true;
            let num = || arg.trim().parse::<i64>().unwrap_or(0);
            let s = match chars[i] {
                's' => String::from(arg),
                'd' | 'i' => huldra_user::format!("{}", num()),
                'x' => huldra_user::format!("{:x}", num()),
                'X' => huldra_user::format!("{:X}", num()),
                'o' => huldra_user::format!("{:o}", num()),
                'c' => arg.chars().next().map(String::from).unwrap_or_default(),
                other => huldra_user::format!("%{}", other),
            };
            out.push_str(&pad(s, width, left, zero));
            i += 1;
        }
        print!("{}", out);
        if !used || values.len() == 0 {
            break;
        }
    }
    0
}
