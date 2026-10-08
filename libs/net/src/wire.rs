//! Packet formats: Ethernet, ARP, IPv4, ICMP, UDP and TCP headers, and the
//! Internet checksum.

use alloc::vec::Vec;
use core::fmt;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
pub struct Ip(pub [u8; 4]);

impl Ip {
    pub const UNSPECIFIED: Ip = Ip([0, 0, 0, 0]);
    pub const BROADCAST: Ip = Ip([255, 255, 255, 255]);
    pub const LOCALHOST: Ip = Ip([127, 0, 0, 1]);

    pub fn from_u32(v: u32) -> Ip {
        Ip(v.to_be_bytes())
    }

    pub fn to_u32(self) -> u32 {
        u32::from_be_bytes(self.0)
    }

    pub fn is_unspecified(self) -> bool {
        self == Ip::UNSPECIFIED
    }

    pub fn is_loopback(self) -> bool {
        self.0[0] == 127
    }

    pub fn is_broadcast(self) -> bool {
        self == Ip::BROADCAST
    }

    pub fn same_subnet(self, other: Ip, mask: Ip) -> bool {
        self.to_u32() & mask.to_u32() == other.to_u32() & mask.to_u32()
    }

    pub fn parse(s: &str) -> Option<Ip> {
        let mut out = [0u8; 4];
        let mut parts = s.split('.');
        for b in &mut out {
            *b = parts.next()?.parse().ok()?;
        }
        parts.next().is_none().then_some(Ip(out))
    }

    /// Prefix length of a netmask.
    pub fn prefix_len(self) -> u32 {
        self.to_u32().leading_ones()
    }
}

impl fmt::Display for Ip {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mac(pub [u8; 6]);

impl Mac {
    pub const BROADCAST: Mac = Mac([0xFF; 6]);
}

impl fmt::Display for Mac {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let m = self.0;
        write!(f, "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", m[0], m[1], m[2], m[3], m[4], m[5])
    }
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn ip_at(b: &[u8], at: usize) -> Ip {
    Ip([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// One's complement sum of 16-bit words, starting from `initial`.
pub fn sum(data: &[u8], initial: u32) -> u32 {
    let mut s = initial;
    let mut chunks = data.chunks_exact(2);
    for c in &mut chunks {
        s += u16::from_be_bytes([c[0], c[1]]) as u32;
    }
    if let [last] = chunks.remainder() {
        s += (*last as u32) << 8;
    }
    s
}

pub fn fold(mut s: u32) -> u16 {
    while s >> 16 != 0 {
        s = (s & 0xFFFF) + (s >> 16);
    }
    !(s as u16)
}

pub fn checksum(data: &[u8]) -> u16 {
    fold(sum(data, 0))
}

fn pseudo_sum(src: Ip, dst: Ip, proto: u8, len: usize) -> u32 {
    sum(&src.0, 0) + sum(&dst.0, 0) + proto as u32 + len as u32
}

// ---- Ethernet --------------------------------------------------------------

pub const ETH_IPV4: u16 = 0x0800;
pub const ETH_ARP: u16 = 0x0806;
pub const ETH_HEADER: usize = 14;

pub struct Eth<'a> {
    pub dst: Mac,
    pub src: Mac,
    pub ethertype: u16,
    pub payload: &'a [u8],
}

impl<'a> Eth<'a> {
    pub fn parse(frame: &'a [u8]) -> Option<Eth<'a>> {
        if frame.len() < ETH_HEADER {
            return None;
        }
        let mac = |at: usize| Mac(frame[at..at + 6].try_into().unwrap());
        Some(Eth { dst: mac(0), src: mac(6), ethertype: be16(frame, 12), payload: &frame[ETH_HEADER..] })
    }
}

pub fn eth_frame(dst: Mac, src: Mac, ethertype: u16, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(ETH_HEADER + payload.len().max(46));
    f.extend_from_slice(&dst.0);
    f.extend_from_slice(&src.0);
    f.extend_from_slice(&ethertype.to_be_bytes());
    f.extend_from_slice(payload);
    // Minimum frame size (without the FCS).
    if f.len() < 60 {
        f.resize(60, 0);
    }
    f
}

// ---- ARP -----------------------------------------------------------------------

pub const ARP_REQUEST: u16 = 1;
pub const ARP_REPLY: u16 = 2;

pub struct Arp {
    pub op: u16,
    pub sender_mac: Mac,
    pub sender_ip: Ip,
    pub target_mac: Mac,
    pub target_ip: Ip,
}

impl Arp {
    pub fn parse(p: &[u8]) -> Option<Arp> {
        // Ethernet + IPv4 only.
        if p.len() < 28 || be16(p, 0) != 1 || be16(p, 2) != ETH_IPV4 || p[4] != 6 || p[5] != 4 {
            return None;
        }
        Some(Arp {
            op: be16(p, 6),
            sender_mac: Mac(p[8..14].try_into().unwrap()),
            sender_ip: ip_at(p, 14),
            target_mac: Mac(p[18..24].try_into().unwrap()),
            target_ip: ip_at(p, 24),
        })
    }

    pub fn build(&self) -> Vec<u8> {
        let mut p = Vec::with_capacity(28);
        p.extend_from_slice(&[0, 1, 8, 0, 6, 4]);
        p.extend_from_slice(&self.op.to_be_bytes());
        p.extend_from_slice(&self.sender_mac.0);
        p.extend_from_slice(&self.sender_ip.0);
        p.extend_from_slice(&self.target_mac.0);
        p.extend_from_slice(&self.target_ip.0);
        p
    }
}

// ---- IPv4 ----------------------------------------------------------------------

pub const PROTO_ICMP: u8 = 1;
pub const PROTO_TCP: u8 = 6;
pub const PROTO_UDP: u8 = 17;

pub struct Ipv4<'a> {
    pub src: Ip,
    pub dst: Ip,
    pub proto: u8,
    pub ttl: u8,
    pub payload: &'a [u8],
}

impl<'a> Ipv4<'a> {
    /// Parses and validates an IPv4 packet; fragments are not supported.
    pub fn parse(p: &'a [u8]) -> Option<Ipv4<'a>> {
        if p.len() < 20 || p[0] >> 4 != 4 {
            return None;
        }
        let ihl = (p[0] & 0xF) as usize * 4;
        let total = be16(p, 2) as usize;
        if ihl < 20 || total < ihl || total > p.len() || checksum(&p[..ihl]) != 0 {
            return None;
        }
        let frag = be16(p, 6);
        if frag & 0x2000 != 0 || frag & 0x1FFF != 0 {
            return None; // more fragments / non-zero offset
        }
        Some(Ipv4 { src: ip_at(p, 12), dst: ip_at(p, 16), proto: p[9], ttl: p[8], payload: &p[ihl..total] })
    }
}

pub fn ipv4_packet(src: Ip, dst: Ip, proto: u8, id: u16, payload: &[u8]) -> Vec<u8> {
    let total = 20 + payload.len();
    let mut p = Vec::with_capacity(total);
    p.extend_from_slice(&[0x45, 0]);
    p.extend_from_slice(&(total as u16).to_be_bytes());
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&0x4000u16.to_be_bytes()); // don't fragment
    p.extend_from_slice(&[64, proto, 0, 0]);
    p.extend_from_slice(&src.0);
    p.extend_from_slice(&dst.0);
    let c = checksum(&p);
    p[10..12].copy_from_slice(&c.to_be_bytes());
    p.extend_from_slice(payload);
    p
}

// ---- ICMP ----------------------------------------------------------------------

pub const ICMP_ECHO_REPLY: u8 = 0;
pub const ICMP_UNREACHABLE: u8 = 3;
pub const ICMP_ECHO_REQUEST: u8 = 8;

pub struct Icmp<'a> {
    pub kind: u8,
    pub code: u8,
    /// Identifier and sequence number for echo messages.
    pub id: u16,
    pub seq: u16,
    pub data: &'a [u8],
}

impl<'a> Icmp<'a> {
    pub fn parse(p: &'a [u8]) -> Option<Icmp<'a>> {
        if p.len() < 8 || checksum(p) != 0 {
            return None;
        }
        Some(Icmp { kind: p[0], code: p[1], id: be16(p, 4), seq: be16(p, 6), data: &p[8..] })
    }
}

pub fn icmp_message(kind: u8, code: u8, id: u16, seq: u16, data: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(8 + data.len());
    p.extend_from_slice(&[kind, code, 0, 0]);
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&seq.to_be_bytes());
    p.extend_from_slice(data);
    let c = checksum(&p);
    p[2..4].copy_from_slice(&c.to_be_bytes());
    p
}

// ---- UDP -----------------------------------------------------------------------

pub struct Udp<'a> {
    pub src_port: u16,
    pub dst_port: u16,
    pub data: &'a [u8],
}

impl<'a> Udp<'a> {
    pub fn parse(src: Ip, dst: Ip, p: &'a [u8]) -> Option<Udp<'a>> {
        if p.len() < 8 {
            return None;
        }
        let len = be16(p, 4) as usize;
        if len < 8 || len > p.len() {
            return None;
        }
        if be16(p, 6) != 0 && fold(sum(&p[..len], pseudo_sum(src, dst, PROTO_UDP, len))) != 0 {
            return None;
        }
        Some(Udp { src_port: be16(p, 0), dst_port: be16(p, 2), data: &p[8..len] })
    }
}

pub fn udp_datagram(src: Ip, dst: Ip, src_port: u16, dst_port: u16, data: &[u8]) -> Vec<u8> {
    let len = 8 + data.len();
    let mut p = Vec::with_capacity(len);
    p.extend_from_slice(&src_port.to_be_bytes());
    p.extend_from_slice(&dst_port.to_be_bytes());
    p.extend_from_slice(&(len as u16).to_be_bytes());
    p.extend_from_slice(&[0, 0]);
    p.extend_from_slice(data);
    let mut c = fold(sum(&p, pseudo_sum(src, dst, PROTO_UDP, len)));
    if c == 0 {
        c = 0xFFFF;
    }
    p[6..8].copy_from_slice(&c.to_be_bytes());
    p
}

// ---- TCP -----------------------------------------------------------------------

pub const TCP_FIN: u8 = 0x01;
pub const TCP_SYN: u8 = 0x02;
pub const TCP_RST: u8 = 0x04;
pub const TCP_PSH: u8 = 0x08;
pub const TCP_ACK: u8 = 0x10;

/// A TCP segment without addresses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Segment {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq: u32,
    pub ack: u32,
    pub flags: u8,
    pub window: u16,
    pub mss: Option<u16>,
    pub data: Vec<u8>,
}

impl Segment {
    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    /// Sequence space used: data plus one for each of SYN and FIN.
    pub fn len(&self) -> u32 {
        self.data.len() as u32 + self.has(TCP_SYN) as u32 + self.has(TCP_FIN) as u32
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn parse(src: Ip, dst: Ip, p: &[u8]) -> Option<Segment> {
        if p.len() < 20 || fold(sum(p, pseudo_sum(src, dst, PROTO_TCP, p.len()))) != 0 {
            return None;
        }
        let off = (p[12] >> 4) as usize * 4;
        if off < 20 || off > p.len() {
            return None;
        }
        let mut mss = None;
        let mut i = 20;
        while i < off {
            match p[i] {
                0 => break,
                1 => i += 1,
                kind => {
                    let len = *p.get(i + 1)? as usize;
                    if len < 2 || i + len > off {
                        break;
                    }
                    if kind == 2 && len == 4 {
                        mss = Some(be16(p, i + 2));
                    }
                    i += len;
                }
            }
        }
        Some(Segment {
            src_port: be16(p, 0),
            dst_port: be16(p, 2),
            seq: be32(p, 4),
            ack: be32(p, 8),
            flags: p[13],
            window: be16(p, 14),
            mss,
            data: p[off..].to_vec(),
        })
    }

    pub fn build(&self, src: Ip, dst: Ip) -> Vec<u8> {
        let off = if self.mss.is_some() { 24 } else { 20 };
        let mut p = Vec::with_capacity(off + self.data.len());
        p.extend_from_slice(&self.src_port.to_be_bytes());
        p.extend_from_slice(&self.dst_port.to_be_bytes());
        p.extend_from_slice(&self.seq.to_be_bytes());
        p.extend_from_slice(&self.ack.to_be_bytes());
        p.extend_from_slice(&[(off as u8 / 4) << 4, self.flags]);
        p.extend_from_slice(&self.window.to_be_bytes());
        p.extend_from_slice(&[0, 0, 0, 0]);
        if let Some(mss) = self.mss {
            p.extend_from_slice(&[2, 4]);
            p.extend_from_slice(&mss.to_be_bytes());
        }
        p.extend_from_slice(&self.data);
        let c = fold(sum(&p, pseudo_sum(src, dst, PROTO_TCP, p.len())));
        p[16..18].copy_from_slice(&c.to_be_bytes());
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_known_header() {
        // Example IPv4 header from RFC 1071 discussions.
        let h = [0x45, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8, 0x00, 0x01, 0xc0, 0xa8, 0x00, 0xc7];
        assert_eq!(checksum(&h), 0xb861);
    }

    #[test]
    fn ipv4_round_trip() {
        let p = ipv4_packet(Ip([10, 0, 2, 15]), Ip([10, 0, 2, 2]), PROTO_UDP, 7, b"payload");
        let ip = Ipv4::parse(&p).unwrap();
        assert_eq!((ip.src, ip.dst, ip.proto, ip.payload), (Ip([10, 0, 2, 15]), Ip([10, 0, 2, 2]), PROTO_UDP, &b"payload"[..]));
    }

    #[test]
    fn udp_and_tcp_round_trip() {
        let (a, b) = (Ip([1, 2, 3, 4]), Ip([5, 6, 7, 8]));
        let d = udp_datagram(a, b, 1000, 53, b"query");
        let u = Udp::parse(a, b, &d).unwrap();
        assert_eq!((u.src_port, u.dst_port, u.data), (1000, 53, &b"query"[..]));
        assert!(Udp::parse(b, a, &d).is_some()); // pseudo header is symmetric
        let mut bad = d.clone();
        bad[9] ^= 1;
        assert!(Udp::parse(a, b, &bad).is_none());

        let s = Segment { src_port: 1, dst_port: 2, seq: 100, ack: 200, flags: TCP_SYN | TCP_ACK, window: 512, mss: Some(1460), data: b"x".to_vec() };
        assert_eq!(Segment::parse(a, b, &s.build(a, b)), Some(s));
    }

    #[test]
    fn arp_and_icmp() {
        let arp = Arp { op: ARP_REQUEST, sender_mac: Mac([1; 6]), sender_ip: Ip([10, 0, 0, 1]), target_mac: Mac::default(), target_ip: Ip([10, 0, 0, 2]) };
        let p = Arp::parse(&arp.build()).unwrap();
        assert_eq!((p.op, p.sender_ip, p.target_ip), (ARP_REQUEST, arp.sender_ip, arp.target_ip));
        let m = icmp_message(ICMP_ECHO_REQUEST, 0, 0x1234, 7, b"ping");
        let i = Icmp::parse(&m).unwrap();
        assert_eq!((i.kind, i.id, i.seq, i.data), (ICMP_ECHO_REQUEST, 0x1234, 7, &b"ping"[..]));
        assert_eq!(Ip::parse("10.0.2.15"), Some(Ip([10, 0, 2, 15])));
        assert_eq!(Ip([255, 255, 255, 0]).prefix_len(), 24);
    }
}
