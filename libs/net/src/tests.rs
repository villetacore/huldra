//! Two stacks on a simulated wire.

use crate::dhcp;
use crate::stack::*;
use crate::wire::*;
use alloc::vec;
use alloc::vec::Vec;

struct Wire {
    a: Stack,
    b: Stack,
    now: u64,
    /// Drop every n-th frame (both directions).
    drop_every: Option<usize>,
    frames: usize,
}

const A: Ip = Ip([10, 0, 0, 1]);
const B: Ip = Ip([10, 0, 0, 2]);
const MASK: Ip = Ip([255, 255, 255, 0]);

impl Wire {
    fn new() -> Wire {
        let mut a = Stack::new(Some(Mac([2, 0, 0, 0, 0, 1])), 1);
        let mut b = Stack::new(Some(Mac([2, 0, 0, 0, 0, 2])), 2);
        a.configure(IfConfig { ip: A, netmask: MASK, ..Default::default() });
        b.configure(IfConfig { ip: B, netmask: MASK, ..Default::default() });
        Wire { a, b, now: 0, drop_every: None, frames: 0 }
    }

    fn step(&mut self) {
        self.a.poll(self.now);
        self.b.poll(self.now);
        for _ in 0..1000 {
            let mut moved = false;
            while let Some(f) = self.a.transmit() {
                moved = true;
                self.frames += 1;
                if self.drop_every.is_none_or(|n| self.frames % n != 0) {
                    self.b.receive(&f, self.now);
                }
            }
            while let Some(f) = self.b.transmit() {
                moved = true;
                self.frames += 1;
                if self.drop_every.is_none_or(|n| self.frames % n != 0) {
                    self.a.receive(&f, self.now);
                }
            }
            if !moved {
                break;
            }
        }
    }

    fn run(&mut self, ms: u64) {
        for _ in 0..ms / 10 {
            self.step();
            self.now += 10;
        }
    }
}

fn read_all(s: &mut Stack, id: SocketId, out: &mut Vec<u8>) -> bool {
    let mut buf = [0u8; 4096];
    loop {
        match s.recv(id, &mut buf) {
            Ok(0) => return true,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(NetError::WouldBlock) => return false,
            Err(e) => panic!("recv: {e:?}"),
        }
    }
}

fn transfer(drop_every: Option<usize>, size: usize) {
    let mut w = Wire::new();
    w.drop_every = drop_every;
    let server = w.b.socket(Proto::Tcp);
    w.b.bind(server, Ip::UNSPECIFIED, 80).unwrap();
    w.b.listen(server, 4).unwrap();
    let client = w.a.socket(Proto::Tcp);
    w.a.connect(client, B, 80, w.now).unwrap();
    w.run(5000);
    assert_eq!(w.a.connect_result(client), Some(Ok(())));
    let conn = w.b.accept(server).unwrap();
    assert_eq!(w.b.peer_addr(conn).map(|p| p.0), Some(A));

    let data: Vec<u8> = (0..size).map(|i| (i * 7 + i / 251) as u8).collect();
    let mut sent = 0;
    let mut got = Vec::new();
    let mut eof = false;
    for _ in 0..200_000 {
        if sent < data.len() {
            match w.a.send(client, &data[sent..], w.now) {
                Ok(n) => sent += n,
                Err(NetError::WouldBlock) => {}
                Err(e) => panic!("send: {e:?}"),
            }
            if sent == data.len() {
                w.a.shutdown_write(client, w.now).unwrap();
            }
        }
        if read_all(&mut w.b, conn, &mut got) {
            eof = true;
            break;
        }
        w.run(10);
    }
    assert!(eof, "no end of stream; got {} of {} bytes", got.len(), data.len());
    assert!(got == data, "data corrupted: {} of {} bytes", got.len(), data.len());

    // Close both sides; the connections must disappear.
    w.b.close(conn, w.now);
    w.a.close(client, w.now);
    w.run(10_000);
    assert_eq!(w.a.socket_table().lines().count(), 1, "{}", w.a.socket_table());
    assert_eq!(w.b.socket_table().lines().filter(|l| l.contains("ESTABLISHED")).count(), 0);
}

#[test]
fn tcp_transfer() {
    transfer(None, 300_000);
}

#[test]
fn tcp_transfer_with_loss() {
    transfer(Some(7), 60_000);
    transfer(Some(3), 20_000);
}

#[test]
fn tcp_refused_and_timeout() {
    let mut w = Wire::new();
    let c = w.a.socket(Proto::Tcp);
    w.a.connect(c, B, 81, w.now).unwrap();
    w.run(100);
    assert_eq!(w.a.connect_result(c), Some(Err(NetError::Refused)));

    let c = w.a.socket(Proto::Tcp);
    w.a.connect(c, Ip([10, 0, 0, 99]), 80, w.now).unwrap();
    assert_eq!(w.a.connect_result(c), None);
    w.run(600_000);
    assert_eq!(w.a.connect_result(c), Some(Err(NetError::TimedOut)));
}

#[test]
fn tcp_echo_both_directions() {
    let mut w = Wire::new();
    let l = w.b.socket(Proto::Tcp);
    w.b.bind(l, Ip::UNSPECIFIED, 7).unwrap();
    w.b.listen(l, 1).unwrap();
    let dup = w.b.socket(Proto::Tcp);
    assert_eq!(w.b.bind(dup, Ip::UNSPECIFIED, 7), Err(NetError::AddrInUse));
    let c = w.a.socket(Proto::Tcp);
    w.a.connect(c, B, 7, w.now).unwrap();
    w.run(100);
    let s = w.b.accept(l).unwrap();
    assert_eq!(w.b.accept(l), Err(NetError::WouldBlock));
    w.a.send(c, b"hello", w.now).unwrap();
    w.run(50);
    let mut buf = [0u8; 16];
    let n = w.b.recv(s, &mut buf).unwrap();
    w.b.send(s, &buf[..n], w.now).unwrap();
    w.b.close(s, w.now);
    w.run(50);
    let mut got = Vec::new();
    assert!(read_all(&mut w.a, c, &mut got));
    assert_eq!(got, b"hello");
    assert!(w.a.readiness(c).readable);
}

#[test]
fn udp_ping_and_arp() {
    let mut w = Wire::new();
    let s = w.b.socket(Proto::Udp);
    w.b.bind(s, Ip::UNSPECIFIED, 5353).unwrap();
    let c = w.a.socket(Proto::Udp);
    w.a.send_to(c, b"datagram", B, 5353, w.now).unwrap();
    w.run(50);
    let mut buf = [0u8; 64];
    let (n, from, port) = w.b.recv_from(s, &mut buf).unwrap();
    assert_eq!((&buf[..n], from), (&b"datagram"[..], A));
    w.b.send_to(s, b"reply", from, port, w.now).unwrap();
    w.run(50);
    assert_eq!(w.a.recv(c, &mut buf), Ok(5));

    let p = w.a.socket(Proto::Icmp);
    let echo = icmp_message(ICMP_ECHO_REQUEST, 0, 0, 1, b"abcdefgh");
    w.a.send_to(p, &echo, B, 0, w.now).unwrap();
    w.run(50);
    let n = w.a.recv(p, &mut buf).unwrap();
    let reply = Icmp::parse(&buf[..n]).unwrap();
    assert_eq!((reply.kind, reply.seq, reply.data), (ICMP_ECHO_REPLY, 1, &b"abcdefgh"[..]));
}

#[test]
fn loopback() {
    let mut s = Stack::new(None, 5);
    let l = s.socket(Proto::Tcp);
    s.bind(l, Ip::UNSPECIFIED, 8080).unwrap();
    s.listen(l, 2).unwrap();
    let c = s.socket(Proto::Tcp);
    s.connect(c, Ip::LOCALHOST, 8080, 0).unwrap();
    for t in 0..10 {
        s.poll(t);
    }
    let a = s.accept(l).unwrap();
    s.send(c, b"over loopback", 10).unwrap();
    s.poll(11);
    let mut buf = [0u8; 32];
    let n = s.recv(a, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"over loopback");
    // No interface: other destinations are unreachable.
    let u = s.socket(Proto::Udp);
    assert_eq!(s.send_to(u, b"x", Ip([8, 8, 8, 8]), 53, 0), Err(NetError::NetUnreachable));
}

#[test]
fn dhcp_client() {
    let mac = Mac([2, 0, 0, 0, 0, 9]);
    let mut s = Stack::new(Some(mac), 3);
    s.start_dhcp(0);
    s.poll(0);
    let frame = s.transmit().expect("DISCOVER");
    let eth = Eth::parse(&frame).unwrap();
    let ip = Ipv4::parse(eth.payload).unwrap();
    let udp = Udp::parse(ip.src, ip.dst, ip.payload).unwrap();
    assert_eq!((eth.dst, ip.dst, udp.dst_port), (Mac::BROADCAST, Ip::BROADCAST, 67));
    let xid = u32::from_be_bytes(udp.data[4..8].try_into().unwrap());

    let lease = dhcp::Lease { ip: Ip([10, 0, 2, 15]), netmask: MASK, router: Ip([10, 0, 2, 2]), dns: Ip([10, 0, 2, 3]), server: Ip([10, 0, 2, 2]), lease_secs: 3600 };
    let server_mac = Mac([0x52, 0x55, 10, 0, 2, 2]);
    let deliver = |s: &mut Stack, kind: u8| {
        let msg = dhcp::test_reply(kind, xid, mac, &lease);
        let udp = udp_datagram(lease.server, Ip::BROADCAST, 67, 68, &msg);
        let ip = ipv4_packet(lease.server, Ip::BROADCAST, PROTO_UDP, 1, &udp);
        s.receive(&eth_frame(Mac::BROADCAST, server_mac, ETH_IPV4, &ip), 10);
    };
    deliver(&mut s, dhcp::OFFER);
    let req = s.transmit().expect("REQUEST");
    let eth = Eth::parse(&req).unwrap();
    let ip = Ipv4::parse(eth.payload).unwrap();
    let udp = Udp::parse(ip.src, ip.dst, ip.payload).unwrap();
    assert_eq!(dhcp::parse_reply(&udp.data, xid), None); // a request, not a reply
    assert_eq!(udp.data[242], dhcp::REQUEST);
    deliver(&mut s, dhcp::ACK);
    assert_eq!(s.dhcp_state(), "bound");
    assert_eq!(s.config, IfConfig { ip: lease.ip, netmask: MASK, gateway: lease.router, dns: lease.dns });

    // Off-subnet traffic goes to the gateway, after ARP.
    let u = s.socket(Proto::Udp);
    s.send_to(u, b"q", Ip([8, 8, 8, 8]), 53, 20).unwrap();
    let arp = s.transmit().unwrap();
    let a = Arp::parse(Eth::parse(&arp).unwrap().payload).unwrap();
    assert_eq!((a.op, a.target_ip), (ARP_REQUEST, lease.router));
    let reply = Arp { op: ARP_REPLY, sender_mac: server_mac, sender_ip: lease.router, target_mac: mac, target_ip: lease.ip };
    s.receive(&eth_frame(mac, server_mac, ETH_ARP, &reply.build()), 30);
    let f = s.transmit().unwrap();
    assert_eq!(Eth::parse(&f).unwrap().dst, server_mac);
    let _ = vec![0u8];
}
