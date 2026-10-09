//! hostname [NAME]: show or set the host name (/etc/hostname).

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, format, fs, print};

huldra_user::main!(main);

fn main() -> i32 {
    match env::args().get(1) {
        // Written through the link, the name would change inside an
        // immutable generation.
        Some(_) if fs::read_link("/etc/hostname").is_ok_and(|t| t.starts_with("/pkg/")) => {
            eprintln!("hostname: set by /etc/system.conf (hostname = ...); edit it and run 'pkg switch'");
            1
        }
        Some(name) => match fs::write("/etc/hostname", format!("{}\n", name).as_bytes()) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("hostname: {}", e);
                1
            }
        },
        None => {
            print!("{}", fs::read_to_string("/etc/hostname").unwrap_or_else(|_| "localhost\n".into()));
            0
        }
    }
}
