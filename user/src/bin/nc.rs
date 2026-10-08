//! nc HOST PORT | nc -l PORT [-u]: connects stdin/stdout to a TCP (or UDP)
//! socket.

#![no_std]
#![no_main]

use huldra_user::abi::fs::{PollFd, POLLHUP, POLLIN};
use huldra_user::{env, eprintln, io, net, sys};

huldra_user::main!(main);

fn pump(sock: &net::Socket, udp_peer: Option<(net::Ip, u16)>) -> i32 {
    let mut buf = [0u8; 4096];
    let mut stdin_open = true;
    loop {
        let mut fds = [PollFd { fd: sock.fd(), events: POLLIN, revents: 0 }, PollFd { fd: if stdin_open { 0 } else { -1 }, events: POLLIN, revents: 0 }];
        if sys::poll(&mut fds, -1).is_err() {
            return 1;
        }
        if fds[0].revents & (POLLIN | POLLHUP) != 0 {
            match sock.recv(&mut buf) {
                Ok(0) | Err(_) => return 0,
                Ok(n) => {
                    let _ = io::write_all(1, &buf[..n]);
                }
            }
        }
        if fds[1].revents & (POLLIN | POLLHUP) != 0 {
            match sys::read(0, &mut buf) {
                Ok(0) | Err(_) => {
                    stdin_open = false;
                    let _ = sock.shutdown_write();
                }
                Ok(n) => {
                    let r = match udp_peer {
                        Some((ip, port)) => sock.send_to(&buf[..n], ip, port).map(drop),
                        None => sock.send_all(&buf[..n]),
                    };
                    if r.is_err() {
                        return 1;
                    }
                }
            }
        }
    }
}

fn main() -> i32 {
    let args = env::args();
    let listen = args.iter().any(|a| a == "-l");
    let udp = args.iter().any(|a| a == "-u");
    let rest: huldra_user::Vec<&str> = args[1..].iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    let fail = |what: &str, e: huldra_user::Errno| {
        eprintln!("nc: {}: {}", what, e);
        1
    };
    if listen {
        let Some(port) = rest.first().and_then(|p| p.parse().ok()) else {
            eprintln!("usage: nc -l [-u] PORT");
            return 2;
        };
        let s = match if udp { net::Socket::udp() } else { net::Socket::tcp() } {
            Ok(s) => s,
            Err(e) => return fail("socket", e),
        };
        if let Err(e) = s.bind(net::Ip::UNSPECIFIED, port) {
            return fail("bind", e);
        }
        if udp {
            return pump(&s, None);
        }
        if let Err(e) = s.listen(1) {
            return fail("listen", e);
        }
        return match s.accept() {
            Ok((c, _, _)) => pump(&c, None),
            Err(e) => fail("accept", e),
        };
    }
    let (Some(host), Some(port)) = (rest.first(), rest.get(1).and_then(|p| p.parse::<u16>().ok())) else {
        eprintln!("usage: nc [-u] HOST PORT | nc -l [-u] PORT");
        return 2;
    };
    let ip = match net::resolve(host) {
        Ok(ip) => ip,
        Err(e) => return fail(host, e),
    };
    if udp {
        return match net::Socket::udp() {
            Ok(s) => pump(&s, Some((ip, port))),
            Err(e) => fail("socket", e),
        };
    }
    let s = match net::Socket::tcp().and_then(|s| s.connect(ip, port).map(|_| s)) {
        Ok(s) => s,
        Err(e) => return fail(host, e),
    };
    pump(&s, None)
}
