//! host NAME: looks up the IPv4 address of a host name.

#![no_std]
#![no_main]

use huldra_user::{env, eprintln, net, println, Errno};

huldra_user::main!(main);

fn main() -> i32 {
    let args = env::args();
    let Some(name) = args.get(1) else {
        eprintln!("usage: host NAME");
        return 2;
    };
    match net::resolve(name) {
        Ok(ip) => {
            println!("{} has address {}", name, ip);
            0
        }
        Err(Errno::ENOENT) => {
            eprintln!("host: {}: not found", name);
            1
        }
        Err(e) => {
            eprintln!("host: {}: {}", name, e);
            1
        }
    }
}
