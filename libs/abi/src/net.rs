//! Sockets: Linux constants and `struct sockaddr_in`.

pub const AF_UNSPEC: u16 = 0;
pub const AF_INET: u16 = 2;

pub const SOCK_STREAM: u32 = 1;
pub const SOCK_DGRAM: u32 = 2;
pub const SOCK_RAW: u32 = 3;
pub const SOCK_NONBLOCK: u32 = 0o4000;
pub const SOCK_CLOEXEC: u32 = 0o2000000;

pub const IPPROTO_IP: u32 = 0;
pub const IPPROTO_ICMP: u32 = 1;
pub const IPPROTO_TCP: u32 = 6;
pub const IPPROTO_UDP: u32 = 17;

pub const SHUT_RD: u32 = 0;
pub const SHUT_WR: u32 = 1;
pub const SHUT_RDWR: u32 = 2;

pub const SOL_SOCKET: u32 = 1;
pub const SO_REUSEADDR: u32 = 2;
pub const SO_TYPE: u32 = 3;
pub const SO_ERROR: u32 = 4;
pub const SO_BROADCAST: u32 = 6;
pub const SO_SNDBUF: u32 = 7;
pub const SO_RCVBUF: u32 = 8;
pub const SO_KEEPALIVE: u32 = 9;
pub const SO_RCVTIMEO: u32 = 20;
pub const SO_SNDTIMEO: u32 = 21;

pub const MSG_PEEK: u32 = 2;
pub const MSG_DONTWAIT: u32 = 0x40;
pub const MSG_NOSIGNAL: u32 = 0x4000;

/// `struct sockaddr_in` (port and address in network byte order).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(C)]
pub struct SockaddrIn {
    pub family: u16,
    pub port: [u8; 2],
    pub addr: [u8; 4],
    pub zero: [u8; 8],
}

impl SockaddrIn {
    pub fn new(addr: [u8; 4], port: u16) -> SockaddrIn {
        SockaddrIn { family: AF_INET, port: port.to_be_bytes(), addr, zero: [0; 8] }
    }

    pub fn port(&self) -> u16 {
        u16::from_be_bytes(self.port)
    }
}

/// `struct msghdr`
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct MsgHdr {
    pub name: u64,
    pub namelen: u32,
    pub _pad: u32,
    pub iov: u64,
    pub iovlen: u64,
    pub control: u64,
    pub controllen: u64,
    pub flags: i32,
    pub _pad2: u32,
}
