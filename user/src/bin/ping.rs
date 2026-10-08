//! ping [-c count] [-i interval_ms] HOST: ICMP echo.

#![no_std]
#![no_main]

use huldra_net::wire::{icmp_message, Icmp, ICMP_ECHO_REPLY, ICMP_ECHO_REQUEST};
use huldra_user::{env, eprintln, net, println, time, Errno};

huldra_user::main!(main);

fn main() -> i32 {
    let args = env::args();
    let mut count = u32::MAX;
    let mut interval = 1000u64;
    let mut host = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-c" => {
                i += 1;
                count = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(1);
            }
            "-i" => {
                i += 1;
                interval = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(1000);
            }
            h => host = Some(h),
        }
        i += 1;
    }
    let Some(host) = host else {
        eprintln!("usage: ping [-c count] [-i interval_ms] HOST");
        return 2;
    };
    let ip = match net::resolve(host) {
        Ok(ip) => ip,
        Err(e) => {
            eprintln!("ping: {}: {}", host, if e == Errno::ENOENT { "Name or service not known" } else { e.message() });
            return 2;
        }
    };
    let sock = match net::Socket::icmp() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ping: socket: {}", e);
            return 2;
        }
    };
    let _ = sock.set_timeout(1000);
    println!("PING {} ({}) 56 data bytes", host, ip);
    let payload = [0x5Au8; 56];
    let (mut sent, mut received) = (0u32, 0u32);
    let (mut min, mut max, mut sum) = (u64::MAX, 0u64, 0u64);
    let mut buf = [0u8; 1500];
    for seq in 1..=count {
        let start = time::uptime_ms();
        let msg = icmp_message(ICMP_ECHO_REQUEST, 0, 0, seq as u16, &payload);
        if let Err(e) = sock.send_to(&msg, ip, 0) {
            eprintln!("ping: sendto: {}", e);
            return 2;
        }
        sent += 1;
        loop {
            match sock.recv_from(&mut buf) {
                Ok((n, from, _)) => {
                    let Some(reply) = Icmp::parse(&buf[..n]) else { continue };
                    if reply.kind != ICMP_ECHO_REPLY || reply.seq != seq as u16 {
                        continue;
                    }
                    let rtt = time::uptime_ms() - start;
                    received += 1;
                    min = min.min(rtt);
                    max = max.max(rtt);
                    sum += rtt;
                    println!("{} bytes from {}: icmp_seq={} time={} ms", n, from, seq, rtt);
                    break;
                }
                Err(_) => {
                    println!("Request timeout for icmp_seq {}", seq);
                    break;
                }
            }
        }
        if seq != count {
            let spent = time::uptime_ms() - start;
            time::sleep_ms(interval.saturating_sub(spent));
        }
    }
    println!("--- {} ping statistics ---", host);
    println!("{} packets transmitted, {} received, {}% packet loss", sent, received, (sent - received) * 100 / sent.max(1));
    if received > 0 {
        println!("rtt min/avg/max = {}/{}/{} ms", min, sum / received as u64, max);
    }
    if received == 0 {
        1
    } else {
        0
    }
}
