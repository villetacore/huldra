//! IPv4 sockets and name resolution.

use crate::sys::{check, syscall6};
use crate::{fs, Errno, Result, Vec};
use huldra_abi::net::*;
use huldra_abi::syscall as nr;
pub use huldra_net::Ip;
use huldra_net::dns;

/// A socket descriptor, closed on drop.
pub struct Socket {
    fd: i32,
}

fn sockaddr(ip: Ip, port: u16) -> SockaddrIn {
    SockaddrIn::new(ip.0, port)
}

unsafe fn call(n: usize, a: [usize; 6]) -> Result<usize> {
    check(syscall6(n, a[0], a[1], a[2], a[3], a[4], a[5]))
}

impl Socket {
    pub fn new(ty: u32, proto: u32) -> Result<Socket> {
        let fd = unsafe { call(nr::SOCKET, [AF_INET as usize, (ty | SOCK_CLOEXEC) as usize, proto as usize, 0, 0, 0])? };
        Ok(Socket { fd: fd as i32 })
    }

    pub fn tcp() -> Result<Socket> {
        Socket::new(SOCK_STREAM, 0)
    }

    pub fn udp() -> Result<Socket> {
        Socket::new(SOCK_DGRAM, 0)
    }

    /// A "ping socket" for ICMP echo.
    pub fn icmp() -> Result<Socket> {
        Socket::new(SOCK_DGRAM, IPPROTO_ICMP)
    }

    pub fn fd(&self) -> i32 {
        self.fd
    }

    pub fn connect(&self, ip: Ip, port: u16) -> Result<()> {
        let sa = sockaddr(ip, port);
        unsafe { call(nr::CONNECT, [self.fd as usize, &sa as *const _ as usize, 16, 0, 0, 0]).map(drop) }
    }

    pub fn bind(&self, ip: Ip, port: u16) -> Result<()> {
        let sa = sockaddr(ip, port);
        unsafe { call(nr::BIND, [self.fd as usize, &sa as *const _ as usize, 16, 0, 0, 0]).map(drop) }
    }

    pub fn listen(&self, backlog: usize) -> Result<()> {
        unsafe { call(nr::LISTEN, [self.fd as usize, backlog, 0, 0, 0, 0]).map(drop) }
    }

    pub fn accept(&self) -> Result<(Socket, Ip, u16)> {
        let mut sa = SockaddrIn::default();
        let mut len = 16u32;
        let fd = unsafe { call(nr::ACCEPT4, [self.fd as usize, &mut sa as *mut _ as usize, &mut len as *mut _ as usize, SOCK_CLOEXEC as usize, 0, 0])? };
        Ok((Socket { fd: fd as i32 }, Ip(sa.addr), sa.port()))
    }

    pub fn send(&self, data: &[u8]) -> Result<usize> {
        unsafe { call(nr::SENDTO, [self.fd as usize, data.as_ptr() as usize, data.len(), MSG_NOSIGNAL as usize, 0, 0]) }
    }

    pub fn send_all(&self, mut data: &[u8]) -> Result<()> {
        while !data.is_empty() {
            let n = self.send(data)?;
            data = &data[n..];
        }
        Ok(())
    }

    pub fn send_to(&self, data: &[u8], ip: Ip, port: u16) -> Result<usize> {
        let sa = sockaddr(ip, port);
        unsafe { call(nr::SENDTO, [self.fd as usize, data.as_ptr() as usize, data.len(), 0, &sa as *const _ as usize, 16]) }
    }

    pub fn recv(&self, buf: &mut [u8]) -> Result<usize> {
        unsafe { call(nr::RECVFROM, [self.fd as usize, buf.as_mut_ptr() as usize, buf.len(), 0, 0, 0]) }
    }

    pub fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, Ip, u16)> {
        let mut sa = SockaddrIn::default();
        let mut len = 16u32;
        let n = unsafe { call(nr::RECVFROM, [self.fd as usize, buf.as_mut_ptr() as usize, buf.len(), 0, &mut sa as *mut _ as usize, &mut len as *mut _ as usize])? };
        Ok((n, Ip(sa.addr), sa.port()))
    }

    /// Receive timeout (0 = wait forever); expiry gives EAGAIN.
    pub fn set_timeout(&self, ms: u64) -> Result<()> {
        let tv = [(ms / 1000) as i64, ((ms % 1000) * 1000) as i64];
        unsafe { call(nr::SETSOCKOPT, [self.fd as usize, SOL_SOCKET as usize, SO_RCVTIMEO as usize, tv.as_ptr() as usize, 16, 0]).map(drop) }
    }

    pub fn shutdown_write(&self) -> Result<()> {
        unsafe { call(nr::SHUTDOWN, [self.fd as usize, SHUT_WR as usize, 0, 0, 0, 0]).map(drop) }
    }

    pub fn local_addr(&self) -> Result<(Ip, u16)> {
        let mut sa = SockaddrIn::default();
        let mut len = 16u32;
        unsafe { call(nr::GETSOCKNAME, [self.fd as usize, &mut sa as *mut _ as usize, &mut len as *mut _ as usize, 0, 0, 0])? };
        Ok((Ip(sa.addr), sa.port()))
    }

    /// Reads until the peer closes the connection.
    pub fn read_to_end(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = self.recv(&mut buf)?;
            if n == 0 {
                return Ok(out);
            }
            out.extend_from_slice(&buf[..n]);
        }
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        let _ = crate::sys::close(self.fd);
    }
}

/// Name servers: /etc/resolv.conf, else the one learned by DHCP.
pub fn nameservers() -> Vec<Ip> {
    let mut v = fs::read_to_string("/etc/resolv.conf").map(|s| dns::nameservers(&s)).unwrap_or_default();
    if v.is_empty() {
        v = fs::read_to_string("/proc/net/dns").map(|s| dns::nameservers(&s)).unwrap_or_default();
    }
    v
}

/// Asks `server` for the addresses of `name`.
pub fn dns_query(server: Ip, name: &str) -> Result<dns::Answer> {
    let id = (crate::time::uptime_ms() as u16) ^ (crate::sys::getpid() as u16).rotate_left(7);
    let q = dns::query(id, name).ok_or(Errno::EINVAL)?;
    let s = Socket::udp()?;
    s.set_timeout(1500)?;
    let mut buf = [0u8; 1500];
    for _ in 0..3 {
        s.send_to(&q, server, dns::PORT)?;
        loop {
            match s.recv_from(&mut buf) {
                Ok((n, from, _)) if from == server => {
                    if let Some(a) = dns::parse_reply(&buf[..n], id) {
                        return Ok(a);
                    }
                }
                Ok(_) => {}
                Err(Errno::EAGAIN) => break,
                Err(e) => return Err(e),
            }
        }
    }
    Err(Errno::ETIMEDOUT)
}

/// Resolves a host name: dotted quad, /etc/hosts, then DNS. Unknown
/// names give ENOENT.
pub fn resolve(name: &str) -> Result<Ip> {
    if let Some(ip) = Ip::parse(name) {
        return Ok(ip);
    }
    if let Ok(hosts) = fs::read_to_string("/etc/hosts") {
        if let Some(ip) = dns::hosts_lookup(&hosts, name) {
            return Ok(ip);
        }
    }
    let servers = nameservers();
    if servers.is_empty() {
        return Err(Errno::ENETUNREACH);
    }
    let mut last = Errno::ENOENT;
    for server in servers {
        match dns_query(server, name) {
            Ok(dns::Answer::Addresses(a)) => return Ok(a[0]),
            Ok(_) => return Err(Errno::ENOENT),
            Err(e) => last = e,
        }
    }
    Err(last)
}

pub fn tcp_connect(host: &str, port: u16) -> Result<Socket> {
    let ip = resolve(host)?;
    let s = Socket::tcp()?;
    s.connect(ip, port)?;
    Ok(s)
}

/// `recv` with flags (`MSG_DONTWAIT`...).
pub fn recv_flags(s: &Socket, buf: &mut [u8], flags: u32) -> Result<usize> {
    unsafe { call(nr::RECVFROM, [s.fd as usize, buf.as_mut_ptr() as usize, buf.len(), flags as usize, 0, 0]) }
}

/// `send` with flags; `MSG_NOSIGNAL` is always added.
pub fn send_flags(s: &Socket, data: &[u8], flags: u32) -> Result<usize> {
    unsafe { call(nr::SENDTO, [s.fd as usize, data.as_ptr() as usize, data.len(), (flags | MSG_NOSIGNAL) as usize, 0, 0]) }
}
