//! The network stack for one Ethernet interface plus loopback: ARP,
//! routing, ICMP echo, UDP and TCP sockets, and a DHCP client.
//!
//! Like the TCP control block it does no I/O and keeps no clock: the
//! driver feeds received frames to [`Stack::receive`], calls
//! [`Stack::poll`] with the time in milliseconds, and sends whatever
//! [`Stack::transmit`] returns.

use crate::dhcp::{self, Lease};
use crate::tcp::{self, State, Tcb, TcpError};
use crate::wire::*;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub type SocketId = usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetError {
    WouldBlock,
    InProgress,
    AddrInUse,
    AddrNotAvailable,
    NotConnected,
    IsConnected,
    Refused,
    Reset,
    TimedOut,
    NetUnreachable,
    Invalid,
    NotSupported,
    MessageSize,
    BadSocket,
}

impl From<TcpError> for NetError {
    fn from(e: TcpError) -> NetError {
        match e {
            TcpError::Refused => NetError::Refused,
            TcpError::Reset => NetError::Reset,
            TcpError::TimedOut => NetError::TimedOut,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Proto {
    Udp,
    Tcp,
    /// Linux-style "ping socket": SOCK_DGRAM + IPPROTO_ICMP, echo only.
    Icmp,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfConfig {
    pub ip: Ip,
    pub netmask: Ip,
    pub gateway: Ip,
    pub dns: Ip,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub rx_packets: u64,
    pub rx_bytes: u64,
    pub tx_packets: u64,
    pub tx_bytes: u64,
    pub rx_dropped: u64,
    pub loopback: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Readiness {
    pub readable: bool,
    pub writable: bool,
    pub hangup: bool,
}

enum Kind {
    Udp(VecDeque<(Ip, u16, Vec<u8>)>),
    Icmp(VecDeque<(Ip, Vec<u8>)>),
    TcpIdle,
    TcpListen { backlog: usize, pending: VecDeque<SocketId> },
    Tcp(Tcb),
}

struct Socket {
    kind: Kind,
    local: Option<(Ip, u16)>,
    remote: Option<(Ip, u16)>,
    /// Closed by its owner: removed once the connection has finished.
    orphan: bool,
    /// Accepted from a listener but not yet handed out by `accept`.
    in_backlog: bool,
    /// Error from a failed connection, reported once.
    error: Option<NetError>,
}

enum Dhcp {
    Off,
    Discovering { xid: u32, next: u64, interval: u64 },
    Requesting { xid: u32, offer: Lease, next: u64, tries: u32 },
    Bound(Lease),
}

const DATAGRAM_QUEUE: usize = 64;
const EPHEMERAL: core::ops::RangeInclusive<u16> = 49152..=65535;

pub struct Stack {
    pub mac: Mac,
    /// An Ethernet interface is present (otherwise loopback only).
    pub has_link: bool,
    pub config: IfConfig,
    pub stats: Stats,
    arp: BTreeMap<Ip, Mac>,
    /// Packets waiting for an ARP reply: next hop, packet, first attempt.
    arp_wait: Vec<(Ip, Vec<u8>, u64)>,
    arp_asked: BTreeMap<Ip, u64>,
    tx: VecDeque<Vec<u8>>,
    loopback: VecDeque<Vec<u8>>,
    sockets: BTreeMap<SocketId, Socket>,
    next_socket: SocketId,
    next_port: u16,
    ip_id: u16,
    seed: u32,
    dhcp: Dhcp,
}

impl Stack {
    pub fn new(mac: Option<Mac>, seed: u32) -> Stack {
        Stack {
            mac: mac.unwrap_or_default(),
            has_link: mac.is_some(),
            config: IfConfig::default(),
            stats: Stats::default(),
            arp: BTreeMap::new(),
            arp_wait: Vec::new(),
            arp_asked: BTreeMap::new(),
            tx: VecDeque::new(),
            loopback: VecDeque::new(),
            sockets: BTreeMap::new(),
            next_socket: 1,
            next_port: *EPHEMERAL.start() + (seed % 1000) as u16,
            ip_id: seed as u16,
            seed: seed | 1,
            dhcp: Dhcp::Off,
        }
    }

    fn random(&mut self) -> u32 {
        // xorshift32
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        x
    }

    pub fn configure(&mut self, config: IfConfig) {
        self.config = config;
        self.dhcp = Dhcp::Off;
    }

    pub fn start_dhcp(&mut self, now: u64) {
        if self.has_link {
            let xid = self.random();
            self.dhcp = Dhcp::Discovering { xid, next: now, interval: 1000 };
        }
    }

    pub fn dhcp_state(&self) -> &'static str {
        match self.dhcp {
            Dhcp::Off => "off",
            Dhcp::Discovering { .. } => "discovering",
            Dhcp::Requesting { .. } => "requesting",
            Dhcp::Bound(_) => "bound",
        }
    }

    pub fn is_local(&self, ip: Ip) -> bool {
        ip.is_loopback() || (!ip.is_unspecified() && ip == self.config.ip)
    }

    // ---- output ---------------------------------------------------------

    /// Frames ready for the wire.
    pub fn transmit(&mut self) -> Option<Vec<u8>> {
        let f = self.tx.pop_front()?;
        self.stats.tx_packets += 1;
        self.stats.tx_bytes += f.len() as u64;
        Some(f)
    }

    fn send_frame(&mut self, dst: Mac, ethertype: u16, payload: &[u8]) {
        if self.has_link {
            self.tx.push_back(eth_frame(dst, self.mac, ethertype, payload));
        }
    }

    fn send_ip(&mut self, src: Ip, dst: Ip, proto: u8, payload: &[u8], now: u64) -> Result<(), NetError> {
        self.ip_id = self.ip_id.wrapping_add(1);
        if self.is_local(dst) {
            let src = if src.is_unspecified() { if dst.is_loopback() { Ip::LOCALHOST } else { dst } } else { src };
            self.loopback.push_back(ipv4_packet(src, dst, proto, self.ip_id, payload));
            return Ok(());
        }
        if !self.has_link {
            return Err(NetError::NetUnreachable);
        }
        let src = if src.is_unspecified() { self.config.ip } else { src };
        let packet = ipv4_packet(src, dst, proto, self.ip_id, payload);
        if dst.is_broadcast() {
            self.send_frame(Mac::BROADCAST, ETH_IPV4, &packet);
            return Ok(());
        }
        if self.config.ip.is_unspecified() {
            return Err(NetError::NetUnreachable);
        }
        let hop = if dst.same_subnet(self.config.ip, self.config.netmask) {
            dst
        } else if !self.config.gateway.is_unspecified() {
            self.config.gateway
        } else {
            return Err(NetError::NetUnreachable);
        };
        match self.arp.get(&hop) {
            Some(&mac) => self.send_frame(mac, ETH_IPV4, &packet),
            None => {
                if self.arp_wait.len() < 64 {
                    self.arp_wait.push((hop, packet, now));
                }
                self.arp_request(hop, now);
            }
        }
        Ok(())
    }

    fn arp_request(&mut self, ip: Ip, now: u64) {
        if self.arp_asked.get(&ip).is_some_and(|&t| now < t + 1000) {
            return;
        }
        self.arp_asked.insert(ip, now);
        let req = Arp { op: ARP_REQUEST, sender_mac: self.mac, sender_ip: self.config.ip, target_mac: Mac::default(), target_ip: ip };
        self.send_frame(Mac::BROADCAST, ETH_ARP, &req.build());
    }

    fn send_tcp(&mut self, local: Ip, remote: Ip, segs: Vec<Segment>, now: u64) {
        for s in segs {
            let local = if local.is_unspecified() { self.source_for(remote) } else { local };
            let _ = self.send_ip(local, remote, PROTO_TCP, &s.build(local, remote), now);
        }
    }

    /// Source address used to reach `dst`.
    fn source_for(&self, dst: Ip) -> Ip {
        if dst.is_loopback() {
            Ip::LOCALHOST
        } else {
            self.config.ip
        }
    }

    // ---- input ----------------------------------------------------------

    pub fn receive(&mut self, frame: &[u8], now: u64) {
        self.stats.rx_packets += 1;
        self.stats.rx_bytes += frame.len() as u64;
        let Some(eth) = Eth::parse(frame) else { return };
        if eth.dst != self.mac && eth.dst != Mac::BROADCAST {
            return;
        }
        match eth.ethertype {
            ETH_ARP => self.receive_arp(eth.payload, now),
            ETH_IPV4 => self.receive_ip(eth.payload, false, now),
            _ => self.stats.rx_dropped += 1,
        }
    }

    fn receive_arp(&mut self, p: &[u8], now: u64) {
        let Some(arp) = Arp::parse(p) else { return };
        if arp.sender_ip.is_unspecified() {
            return;
        }
        self.arp.insert(arp.sender_ip, arp.sender_mac);
        self.arp_asked.remove(&arp.sender_ip);
        // Release packets waiting for this address.
        let mut i = 0;
        while i < self.arp_wait.len() {
            if self.arp_wait[i].0 == arp.sender_ip {
                let (_, packet, _) = self.arp_wait.remove(i);
                self.send_frame(arp.sender_mac, ETH_IPV4, &packet);
            } else {
                i += 1;
            }
        }
        if arp.op == ARP_REQUEST && !self.config.ip.is_unspecified() && arp.target_ip == self.config.ip {
            let reply = Arp { op: ARP_REPLY, sender_mac: self.mac, sender_ip: self.config.ip, target_mac: arp.sender_mac, target_ip: arp.sender_ip };
            self.send_frame(arp.sender_mac, ETH_ARP, &reply.build());
        }
        let _ = now;
    }

    fn receive_ip(&mut self, p: &[u8], looped: bool, now: u64) {
        let Some(ip) = Ipv4::parse(p) else {
            self.stats.rx_dropped += 1;
            return;
        };
        let for_us = looped || ip.dst == self.config.ip || ip.dst.is_broadcast() || (self.config.ip.is_unspecified() && ip.proto == PROTO_UDP);
        if !for_us {
            self.stats.rx_dropped += 1;
            return;
        }
        match ip.proto {
            PROTO_ICMP => self.receive_icmp(ip.src, ip.dst, ip.payload, now),
            PROTO_UDP => self.receive_udp(ip.src, ip.dst, ip.payload, now),
            PROTO_TCP => self.receive_tcp(ip.src, ip.dst, ip.payload, now),
            _ => self.stats.rx_dropped += 1,
        }
    }

    fn receive_icmp(&mut self, src: Ip, dst: Ip, p: &[u8], now: u64) {
        let Some(icmp) = Icmp::parse(p) else { return };
        match icmp.kind {
            ICMP_ECHO_REQUEST if !dst.is_broadcast() => {
                let reply = icmp_message(ICMP_ECHO_REPLY, 0, icmp.id, icmp.seq, icmp.data);
                let _ = self.send_ip(dst, src, PROTO_ICMP, &reply, now);
            }
            ICMP_ECHO_REPLY => {
                for s in self.sockets.values_mut() {
                    if let Kind::Icmp(q) = &mut s.kind {
                        if s.local.is_some_and(|l| l.1 == icmp.id) && q.len() < DATAGRAM_QUEUE {
                            q.push_back((src, p.to_vec()));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn receive_udp(&mut self, src: Ip, dst: Ip, p: &[u8], now: u64) {
        let Some(udp) = Udp::parse(src, dst, p) else { return };
        if udp.dst_port == dhcp::CLIENT_PORT && udp.src_port == dhcp::SERVER_PORT {
            self.receive_dhcp(udp.data, now);
            return;
        }
        let target = self.sockets.iter_mut().find(|(_, s)| {
            matches!(s.kind, Kind::Udp(_)) && s.local.is_some_and(|(ip, port)| port == udp.dst_port && (ip.is_unspecified() || ip == dst || dst.is_broadcast())) && s.remote.is_none_or(|r| r == (src, udp.src_port))
        });
        match target {
            Some((_, s)) => {
                if let Kind::Udp(q) = &mut s.kind {
                    if q.len() < DATAGRAM_QUEUE {
                        q.push_back((src, udp.src_port, udp.data.to_vec()));
                    }
                }
            }
            None => self.stats.rx_dropped += 1,
        }
    }

    fn find_tcp(&self, local_port: u16, remote: (Ip, u16)) -> Option<SocketId> {
        self.sockets.iter().find(|(_, s)| matches!(s.kind, Kind::Tcp(_)) && s.local.is_some_and(|l| l.1 == local_port) && s.remote == Some(remote)).map(|(id, _)| *id)
    }

    fn receive_tcp(&mut self, src: Ip, dst: Ip, p: &[u8], now: u64) {
        let Some(seg) = Segment::parse(src, dst, p) else {
            self.stats.rx_dropped += 1;
            return;
        };
        if let Some(id) = self.find_tcp(seg.dst_port, (src, seg.src_port)) {
            let s = self.sockets.get_mut(&id).unwrap();
            let local = s.local.unwrap().0;
            let Kind::Tcp(tcb) = &mut s.kind else { unreachable!() };
            tcb.on_segment(&seg, now);
            tcb.poll(now);
            let out = tcb.take_output();
            self.send_tcp(if local.is_unspecified() { dst } else { local }, src, out, now);
            return;
        }
        // A listening socket?
        let listener = self.sockets.iter().find(|(_, s)| matches!(s.kind, Kind::TcpListen { .. }) && s.local.is_some_and(|(ip, port)| port == seg.dst_port && (ip.is_unspecified() || ip == dst))).map(|(id, _)| *id);
        if let Some(lid) = listener {
            if seg.has(TCP_SYN) && !seg.has(TCP_ACK) && !seg.has(TCP_RST) {
                let full = match &self.sockets[&lid].kind {
                    Kind::TcpListen { backlog, pending } => pending.len() >= *backlog,
                    _ => true,
                };
                if full {
                    return; // the client will retry
                }
                let iss = self.random();
                let mut tcb = Tcb::accept(&seg, iss, now);
                let out = tcb.take_output();
                let child = self.next_socket;
                self.next_socket += 1;
                self.sockets.insert(child, Socket { kind: Kind::Tcp(tcb), local: Some((dst, seg.dst_port)), remote: Some((src, seg.src_port)), orphan: false, in_backlog: true, error: None });
                if let Some(Socket { kind: Kind::TcpListen { pending, .. }, .. }) = self.sockets.get_mut(&lid) {
                    pending.push_back(child);
                }
                self.send_tcp(dst, src, out, now);
                return;
            }
        }
        if !seg.has(TCP_RST) {
            let rst = Tcb::reset_for(&seg);
            let _ = self.send_ip(dst, src, PROTO_TCP, &rst.build(dst, src), now);
        }
    }

    fn receive_dhcp(&mut self, data: &[u8], now: u64) {
        match self.dhcp {
            Dhcp::Discovering { xid, .. } => {
                if let Some((dhcp::OFFER, offer)) = dhcp::parse_reply(data, xid) {
                    self.dhcp = Dhcp::Requesting { xid, offer, next: now, tries: 0 };
                    self.poll_dhcp(now);
                }
            }
            Dhcp::Requesting { xid, ref offer, .. } => match dhcp::parse_reply(data, xid) {
                Some((dhcp::ACK, mut lease)) => {
                    if lease.server.is_unspecified() {
                        lease.server = offer.server;
                    }
                    let netmask = if lease.netmask.is_unspecified() { Ip([255, 255, 255, 0]) } else { lease.netmask };
                    self.config = IfConfig { ip: lease.ip, netmask, gateway: lease.router, dns: lease.dns };
                    self.dhcp = Dhcp::Bound(lease);
                }
                Some((dhcp::NAK, _)) => self.start_dhcp(now),
                _ => {}
            },
            _ => {}
        }
    }

    fn poll_dhcp(&mut self, now: u64) {
        let msg = match &mut self.dhcp {
            Dhcp::Discovering { xid, next, interval } if now >= *next => {
                *next = now + *interval;
                *interval = (*interval * 2).min(8000);
                dhcp::client_message(dhcp::DISCOVER, *xid, self.mac, None)
            }
            Dhcp::Requesting { xid, offer, next, tries } if now >= *next => {
                *tries += 1;
                *next = now + 2000;
                if *tries > 4 {
                    self.start_dhcp(now);
                    return;
                }
                dhcp::client_message(dhcp::REQUEST, *xid, self.mac, Some((offer.ip, offer.server)))
            }
            _ => return,
        };
        let udp = udp_datagram(Ip::UNSPECIFIED, Ip::BROADCAST, dhcp::CLIENT_PORT, dhcp::SERVER_PORT, &msg);
        let packet = ipv4_packet(Ip::UNSPECIFIED, Ip::BROADCAST, PROTO_UDP, 0, &udp);
        self.send_frame(Mac::BROADCAST, ETH_IPV4, &packet);
    }

    // ---- timers -----------------------------------------------------------

    /// Runs timers and delivers looped-back packets.
    pub fn poll(&mut self, now: u64) {
        for _ in 0..1024 {
            let Some(p) = self.loopback.pop_front() else { break };
            self.stats.loopback += 1;
            self.receive_ip(&p, true, now);
        }
        self.poll_dhcp(now);
        // Give up on addresses that never answered ARP.
        self.arp_wait.retain(|(_, _, since)| now < since + 3000);
        let waiting: Vec<Ip> = self.arp_wait.iter().map(|w| w.0).collect();
        for ip in waiting {
            self.arp_request(ip, now);
        }
        let ids: Vec<SocketId> = self.sockets.keys().copied().collect();
        for id in ids {
            let s = self.sockets.get_mut(&id).unwrap();
            let Kind::Tcp(tcb) = &mut s.kind else { continue };
            tcb.poll(now);
            let out = tcb.take_output();
            let closed = tcb.state == State::Closed;
            let (local, remote) = (s.local.unwrap().0, s.remote.unwrap().0);
            if closed && s.orphan {
                self.sockets.remove(&id);
            }
            self.send_tcp(local, remote, out, now);
        }
        if !self.loopback.is_empty() {
            for _ in 0..1024 {
                let Some(p) = self.loopback.pop_front() else { break };
                self.stats.loopback += 1;
                self.receive_ip(&p, true, now);
            }
        }
    }

    /// When `poll` next has timed work to do.
    pub fn next_deadline(&self) -> Option<u64> {
        let mut d: Option<u64> = None;
        let mut at = |t: u64| d = Some(d.map_or(t, |x: u64| x.min(t)));
        for s in self.sockets.values() {
            if let Kind::Tcp(tcb) = &s.kind {
                if let Some(t) = tcb.next_deadline() {
                    at(t);
                }
            }
        }
        match &self.dhcp {
            Dhcp::Discovering { next, .. } | Dhcp::Requesting { next, .. } => at(*next),
            _ => {}
        }
        if let Some(w) = self.arp_wait.first() {
            at(w.2 + 1000);
        }
        if !self.loopback.is_empty() {
            at(0);
        }
        d
    }

    // ---- sockets ------------------------------------------------------------

    pub fn socket(&mut self, proto: Proto) -> SocketId {
        let kind = match proto {
            Proto::Udp => Kind::Udp(VecDeque::new()),
            Proto::Icmp => Kind::Icmp(VecDeque::new()),
            Proto::Tcp => Kind::TcpIdle,
        };
        let id = self.next_socket;
        self.next_socket += 1;
        self.sockets.insert(id, Socket { kind, local: None, remote: None, orphan: false, in_backlog: false, error: None });
        id
    }

    fn sock(&mut self, id: SocketId) -> Result<&mut Socket, NetError> {
        self.sockets.get_mut(&id).ok_or(NetError::BadSocket)
    }

    fn same_proto(a: &Kind, b: &Kind) -> bool {
        matches!((a, b), (Kind::Udp(_), Kind::Udp(_)) | (Kind::Icmp(_), Kind::Icmp(_)))
            || (matches!(a, Kind::TcpIdle | Kind::TcpListen { .. } | Kind::Tcp(_)) && matches!(b, Kind::TcpIdle | Kind::TcpListen { .. } | Kind::Tcp(_)))
    }

    fn port_in_use(&self, kind: &Kind, port: u16, exclude: SocketId) -> bool {
        self.sockets.iter().any(|(id, s)| *id != exclude && !s.orphan && !s.in_backlog && Self::same_proto(&s.kind, kind) && s.local.is_some_and(|l| l.1 == port) && !(matches!(s.kind, Kind::Tcp(_)) && s.remote.is_some() && matches!(kind, Kind::TcpListen { .. })))
    }

    fn ephemeral(&mut self, id: SocketId) -> Result<u16, NetError> {
        let span = (*EPHEMERAL.end() - *EPHEMERAL.start()) as u32 + 1;
        for _ in 0..span {
            let p = self.next_port;
            self.next_port = if p == *EPHEMERAL.end() { *EPHEMERAL.start() } else { p + 1 };
            let kind = &self.sockets[&id].kind;
            if !self.port_in_use(kind, p, id) {
                return Ok(p);
            }
        }
        Err(NetError::AddrInUse)
    }

    pub fn bind(&mut self, id: SocketId, ip: Ip, port: u16) -> Result<(), NetError> {
        let s = self.sockets.get(&id).ok_or(NetError::BadSocket)?;
        if s.local.is_some() {
            return Err(NetError::Invalid);
        }
        if !ip.is_unspecified() && !self.is_local(ip) {
            return Err(NetError::AddrNotAvailable);
        }
        let port = if port == 0 {
            self.ephemeral(id)?
        } else {
            if self.port_in_use(&self.sockets[&id].kind, port, id) {
                return Err(NetError::AddrInUse);
            }
            port
        };
        self.sock(id)?.local = Some((ip, port));
        Ok(())
    }

    fn ensure_bound(&mut self, id: SocketId) -> Result<(), NetError> {
        if self.sock(id)?.local.is_none() {
            if matches!(self.sockets[&id].kind, Kind::Icmp(_)) {
                // The "port" of a ping socket is its echo identifier.
                let echo_id = (self.random() & 0xFFFF) as u16;
                self.sock(id)?.local = Some((Ip::UNSPECIFIED, echo_id));
            } else {
                self.bind(id, Ip::UNSPECIFIED, 0)?;
            }
        }
        Ok(())
    }

    pub fn listen(&mut self, id: SocketId, backlog: usize) -> Result<(), NetError> {
        self.ensure_bound(id)?;
        let s = self.sock(id)?;
        match s.kind {
            Kind::TcpIdle => {
                s.kind = Kind::TcpListen { backlog: backlog.clamp(1, 128), pending: VecDeque::new() };
                Ok(())
            }
            Kind::TcpListen { ref mut backlog, .. } => {
                *backlog = (*backlog).max(1);
                Ok(())
            }
            Kind::Tcp(_) => Err(NetError::IsConnected),
            _ => Err(NetError::NotSupported),
        }
    }

    pub fn accept(&mut self, id: SocketId) -> Result<SocketId, NetError> {
        let ready = {
            let Kind::TcpListen { pending, .. } = &self.sockets.get(&id).ok_or(NetError::BadSocket)?.kind else { return Err(NetError::Invalid) };
            pending.iter().position(|c| match self.sockets.get(c).map(|s| &s.kind) {
                Some(Kind::Tcp(t)) => !t.is_connecting(),
                _ => true,
            })
        };
        let Some(pos) = ready else { return Err(NetError::WouldBlock) };
        let Kind::TcpListen { pending, .. } = &mut self.sock(id)?.kind else { unreachable!() };
        let child = pending.remove(pos).unwrap();
        match self.sockets.get_mut(&child) {
            Some(c) => {
                c.in_backlog = false;
                Ok(child)
            }
            None => Err(NetError::WouldBlock),
        }
    }

    pub fn connect(&mut self, id: SocketId, ip: Ip, port: u16, now: u64) -> Result<(), NetError> {
        if port == 0 && !matches!(self.sock(id)?.kind, Kind::Icmp(_)) {
            return Err(NetError::Invalid);
        }
        self.ensure_bound(id)?;
        let s = self.sock(id)?;
        match &s.kind {
            Kind::Udp(_) | Kind::Icmp(_) => {
                s.remote = Some((ip, port));
                return Ok(());
            }
            Kind::TcpIdle => {}
            Kind::Tcp(t) if t.is_connecting() => return Err(NetError::InProgress),
            Kind::Tcp(_) => return Err(NetError::IsConnected),
            Kind::TcpListen { .. } => return Err(NetError::Invalid),
        }
        if ip.is_unspecified() || ip.is_broadcast() {
            return Err(NetError::NetUnreachable);
        }
        let local_port = self.sockets[&id].local.unwrap().1;
        let iss = self.random();
        let mut tcb = Tcb::connect(local_port, port, iss, now);
        let out = tcb.take_output();
        let local_ip = match self.sockets[&id].local.unwrap().0 {
            l if l.is_unspecified() => self.source_for(ip),
            l => l,
        };
        if local_ip.is_unspecified() {
            return Err(NetError::NetUnreachable);
        }
        let s = self.sock(id)?;
        s.kind = Kind::Tcp(tcb);
        s.remote = Some((ip, port));
        s.local = Some((local_ip, local_port));
        self.send_tcp(local_ip, ip, out, now);
        Ok(())
    }

    /// For a TCP socket in `connect`: `None` while the handshake runs.
    pub fn connect_result(&mut self, id: SocketId) -> Option<Result<(), NetError>> {
        let s = self.sockets.get_mut(&id)?;
        match &s.kind {
            Kind::Tcp(t) if t.is_connecting() => None,
            Kind::Tcp(t) => match t.error {
                Some(e) => {
                    s.error = None;
                    Some(Err(e.into()))
                }
                None => Some(Ok(())),
            },
            _ => Some(Err(NetError::NotConnected)),
        }
    }

    pub fn send(&mut self, id: SocketId, data: &[u8], now: u64) -> Result<usize, NetError> {
        let s = self.sock(id)?;
        match &mut s.kind {
            Kind::Tcp(tcb) => {
                if let Some(e) = tcb.error {
                    return Err(e.into());
                }
                if tcb.is_connecting() {
                    return Err(NetError::WouldBlock);
                }
                if !tcb.writable() {
                    return Err(if tcb.unsent() >= tcp::BUFFER { NetError::WouldBlock } else { NetError::NotConnected });
                }
                let n = tcb.send(data);
                tcb.poll(now);
                let out = tcb.take_output();
                let (local, remote) = (s.local.unwrap().0, s.remote.unwrap().0);
                self.send_tcp(local, remote, out, now);
                Ok(n)
            }
            Kind::TcpIdle | Kind::TcpListen { .. } => Err(NetError::NotConnected),
            Kind::Udp(_) | Kind::Icmp(_) => {
                let (ip, port) = s.remote.ok_or(NetError::NotConnected)?;
                self.send_to(id, data, ip, port, now)
            }
        }
    }

    pub fn send_to(&mut self, id: SocketId, data: &[u8], ip: Ip, port: u16, now: u64) -> Result<usize, NetError> {
        if matches!(self.sock(id)?.kind, Kind::Tcp(_) | Kind::TcpIdle | Kind::TcpListen { .. }) {
            return self.send(id, data, now);
        }
        if data.len() > 65000 {
            return Err(NetError::MessageSize);
        }
        self.ensure_bound(id)?;
        let (local_ip, local_port) = self.sockets[&id].local.unwrap();
        let src = if local_ip.is_unspecified() { self.source_for(ip) } else { local_ip };
        match self.sockets[&id].kind {
            Kind::Udp(_) => {
                let udp = udp_datagram(src, ip, local_port, port, data);
                self.send_ip(src, ip, PROTO_UDP, &udp, now)?;
            }
            _ => {
                // Ping socket: the caller supplies the ICMP header; the
                // identifier is the socket's.
                if data.len() < 8 || data[0] != ICMP_ECHO_REQUEST {
                    return Err(NetError::Invalid);
                }
                let seq = u16::from_be_bytes([data[6], data[7]]);
                let msg = icmp_message(ICMP_ECHO_REQUEST, 0, local_port, seq, &data[8..]);
                self.send_ip(src, ip, PROTO_ICMP, &msg, now)?;
            }
        }
        Ok(data.len())
    }

    pub fn recv(&mut self, id: SocketId, buf: &mut [u8]) -> Result<usize, NetError> {
        self.recv_from(id, buf).map(|(n, _, _)| n)
    }

    /// Receives data; for datagrams also the sender. TCP returns 0 at end
    /// of stream.
    pub fn recv_from(&mut self, id: SocketId, buf: &mut [u8]) -> Result<(usize, Ip, u16), NetError> {
        let s = self.sock(id)?;
        let remote = s.remote.unwrap_or_default();
        match &mut s.kind {
            Kind::Tcp(tcb) => {
                if tcb.readable() {
                    return Ok((tcb.recv(buf), remote.0, remote.1));
                }
                if let Some(e) = tcb.error {
                    if e != TcpError::TimedOut || tcb.state == State::Closed {
                        return Err(e.into());
                    }
                }
                if tcb.at_eof() {
                    return Ok((0, remote.0, remote.1));
                }
                Err(NetError::WouldBlock)
            }
            Kind::Udp(q) => {
                let (ip, port, d) = q.pop_front().ok_or(NetError::WouldBlock)?;
                let n = d.len().min(buf.len());
                buf[..n].copy_from_slice(&d[..n]);
                Ok((n, ip, port))
            }
            Kind::Icmp(q) => {
                let (ip, d) = q.pop_front().ok_or(NetError::WouldBlock)?;
                let n = d.len().min(buf.len());
                buf[..n].copy_from_slice(&d[..n]);
                Ok((n, ip, 0))
            }
            _ => Err(NetError::NotConnected),
        }
    }

    /// Stops sending (TCP: FIN after the queued data).
    pub fn shutdown_write(&mut self, id: SocketId, now: u64) -> Result<(), NetError> {
        let s = self.sock(id)?;
        let Kind::Tcp(tcb) = &mut s.kind else { return Err(NetError::NotConnected) };
        tcb.close();
        tcb.poll(now);
        let out = tcb.take_output();
        let (local, remote) = (s.local.unwrap().0, s.remote.unwrap().0);
        self.send_tcp(local, remote, out, now);
        Ok(())
    }

    /// The owner closed the socket. TCP connections finish in the
    /// background; unread data makes it an abortive close (RST).
    pub fn close(&mut self, id: SocketId, now: u64) {
        let Some(mut s) = self.sockets.remove(&id) else { return };
        match &mut s.kind {
            Kind::Tcp(tcb) => {
                if tcb.readable() {
                    tcb.abort();
                } else {
                    tcb.close();
                }
                tcb.poll(now);
                let out = tcb.take_output();
                let (local, remote) = (s.local.unwrap().0, s.remote.unwrap().0);
                let done = tcb.state == State::Closed;
                self.send_tcp(local, remote, out, now);
                if !done {
                    s.orphan = true;
                    self.sockets.insert(id, s);
                }
            }
            Kind::TcpListen { pending, .. } => {
                let pending: Vec<SocketId> = pending.drain(..).collect();
                for c in pending {
                    if let Some(mut child) = self.sockets.remove(&c) {
                        if let Kind::Tcp(t) = &mut child.kind {
                            t.abort();
                            let out = t.take_output();
                            self.send_tcp(child.local.unwrap().0, child.remote.unwrap().0, out, now);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub fn readiness(&self, id: SocketId) -> Readiness {
        let Some(s) = self.sockets.get(&id) else { return Readiness { readable: true, writable: true, hangup: true } };
        match &s.kind {
            Kind::Tcp(t) => Readiness {
                readable: t.readable() || t.at_eof() || t.error.is_some(),
                writable: (t.writable() && !t.is_connecting()) || t.state == State::Closed,
                hangup: t.state == State::Closed || (t.at_eof() && !t.writable()),
            },
            Kind::TcpListen { pending, .. } => Readiness {
                readable: pending.iter().any(|c| match self.sockets.get(c).map(|s| &s.kind) {
                    Some(Kind::Tcp(t)) => !t.is_connecting(),
                    _ => true,
                }),
                writable: false,
                hangup: false,
            },
            Kind::TcpIdle => Readiness { readable: false, writable: false, hangup: true },
            Kind::Udp(q) => Readiness { readable: !q.is_empty(), writable: true, hangup: false },
            Kind::Icmp(q) => Readiness { readable: !q.is_empty(), writable: true, hangup: false },
        }
    }

    pub fn local_addr(&self, id: SocketId) -> Option<(Ip, u16)> {
        self.sockets.get(&id)?.local
    }

    pub fn peer_addr(&self, id: SocketId) -> Option<(Ip, u16)> {
        let s = self.sockets.get(&id)?;
        match &s.kind {
            Kind::Tcp(t) if t.is_connecting() || t.state == State::Closed => None,
            _ => s.remote,
        }
    }

    /// Bytes waiting to be read (FIONREAD).
    pub fn pending_bytes(&self, id: SocketId) -> usize {
        match self.sockets.get(&id).map(|s| &s.kind) {
            Some(Kind::Udp(q)) => q.front().map_or(0, |d| d.2.len()),
            Some(Kind::Icmp(q)) => q.front().map_or(0, |d| d.1.len()),
            _ => 0,
        }
    }

    /// A line per socket for /proc/net/sockets.
    pub fn socket_table(&self) -> String {
        let mut out = String::from("proto local                 remote                state\n");
        let addr = |a: Option<(Ip, u16)>| a.map_or(String::from("*"), |(ip, p)| format!("{}:{}", ip, p));
        for s in self.sockets.values() {
            let (proto, state) = match &s.kind {
                Kind::Udp(_) => ("udp", String::new()),
                Kind::Icmp(_) => ("icmp", String::new()),
                Kind::TcpIdle => ("tcp", String::from("CLOSED")),
                Kind::TcpListen { .. } => ("tcp", String::from("LISTEN")),
                Kind::Tcp(t) => ("tcp", format!("{:?}", t.state).to_uppercase()),
            };
            out.push_str(&format!("{:<5} {:<21} {:<21} {}\n", proto, addr(s.local), addr(s.remote), state));
        }
        out
    }

    pub fn lease(&self) -> Option<&Lease> {
        match &self.dhcp {
            Dhcp::Bound(l) => Some(l),
            _ => None,
        }
    }
}
