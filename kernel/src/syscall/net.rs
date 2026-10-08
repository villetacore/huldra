//! Socket system calls (IPv4 TCP, UDP and ICMP echo).

use super::{value, Args, Ret};
use crate::fs::{KResult, OpenFile};
use crate::net::socket::{errno, Socket};
use crate::net::{with, EVENTS};
use crate::proc::uaccess;
use crate::task::sched;
use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use huldra_abi::errno::Errno;
use huldra_abi::fs::{O_CLOEXEC, O_NONBLOCK, O_RDWR};
use huldra_abi::net::*;
use huldra_net::{Ip, NetError, Proto};

fn socket_of(fd: u64) -> KResult<Arc<OpenFile>> {
    let f = sched::current().files.lock().get(fd as i32)?;
    if f.inode.as_any().downcast_ref::<Socket>().is_none() {
        return Err(Errno::ENOTSOCK);
    }
    Ok(f)
}

fn sock(f: &OpenFile) -> &Socket {
    f.inode.as_any().downcast_ref::<Socket>().unwrap()
}

fn nonblock(f: &OpenFile) -> bool {
    f.flags() & O_NONBLOCK != 0
}

fn read_addr(addr: u64, len: u64) -> KResult<(Ip, u16)> {
    if (len as usize) < core::mem::size_of::<SockaddrIn>() {
        return Err(Errno::EINVAL);
    }
    let sa: SockaddrIn = uaccess::read_user(addr)?;
    if sa.family != AF_INET {
        return Err(Errno::EAFNOSUPPORT);
    }
    Ok((Ip(sa.addr), sa.port()))
}

/// Stores an address for accept/recvfrom/getsockname (`len_ptr` is in/out).
fn write_addr(addr: u64, len_ptr: u64, a: (Ip, u16)) -> KResult<()> {
    if addr == 0 || len_ptr == 0 {
        return Ok(());
    }
    let cap: u32 = uaccess::read_user(len_ptr)?;
    let sa = SockaddrIn::new(a.0 .0, a.1);
    let bytes = unsafe { core::slice::from_raw_parts(&sa as *const SockaddrIn as *const u8, 16) };
    uaccess::copy_to_user(addr, &bytes[..(cap as usize).min(16)])?;
    uaccess::write_user(len_ptr, &16u32)
}

fn install(sock: Arc<Socket>, flags: u32) -> KResult<Ret> {
    let file = OpenFile::new(sock.clone(), O_RDWR | (flags & O_NONBLOCK), format!("socket:[{}]", sock.id))?;
    let fd = sched::current().files.lock().alloc(file, flags & O_CLOEXEC != 0)?;
    value(fd as u64)
}

pub fn socket(a: &mut Args) -> KResult<Ret> {
    let (domain, ty, protocol) = (a.a0() as u16, a.a1() as u32, a.a2() as u32);
    if domain != AF_INET {
        return Err(Errno::EAFNOSUPPORT);
    }
    let proto = match (ty & 0xF, protocol) {
        (SOCK_STREAM, IPPROTO_IP | IPPROTO_TCP) => Proto::Tcp,
        (SOCK_DGRAM, IPPROTO_IP | IPPROTO_UDP) => Proto::Udp,
        (SOCK_DGRAM, IPPROTO_ICMP) => Proto::Icmp,
        (SOCK_STREAM | SOCK_DGRAM, _) => return Err(Errno::EPROTONOSUPPORT),
        _ => return Err(Errno::EPROTOTYPE),
    };
    let flags = (if ty & SOCK_NONBLOCK != 0 { O_NONBLOCK } else { 0 }) | (if ty & SOCK_CLOEXEC != 0 { O_CLOEXEC } else { 0 });
    install(Socket::new(proto), flags)
}

pub fn bind(a: &mut Args) -> KResult<Ret> {
    let f = socket_of(a.a0())?;
    let s = sock(&f);
    let (ip, port) = read_addr(a.a1(), a.a2())?;
    let id = s.id;
    with(|st, _| st.bind(id, ip, port)).map_err(errno)?;
    value(0)
}

pub fn listen(a: &mut Args) -> KResult<Ret> {
    let f = socket_of(a.a0())?;
    let s = sock(&f);
    let (id, backlog) = (s.id, a.a1() as usize);
    with(|st, _| st.listen(id, backlog)).map_err(errno)?;
    value(0)
}

pub fn connect(a: &mut Args) -> KResult<Ret> {
    let f = socket_of(a.a0())?;
    let s = sock(&f);
    let (ip, port) = read_addr(a.a1(), a.a2())?;
    let id = s.id;
    match with(|st, now| st.connect(id, ip, port, now)) {
        Ok(()) => {}
        Err(NetError::InProgress) if !nonblock(&f) => {}
        Err(e) => return Err(errno(e)),
    }
    if s.proto != Proto::Tcp {
        return value(0);
    }
    if nonblock(&f) {
        return Err(Errno::EINPROGRESS);
    }
    let r = EVENTS.wait_until(|| with(|st, _| st.connect_result(id)))?;
    r.map_err(errno)?;
    value(0)
}

pub fn accept4(a: &mut Args, flags: u32) -> KResult<Ret> {
    let f = socket_of(a.a0())?;
    let s = sock(&f);
    let id = s.id;
    let child = s.block(nonblock(&f), 0, |st, _| st.accept(id))?;
    let peer = with(|st, _| st.peer_addr(child)).unwrap_or_default();
    write_addr(a.a1(), a.a2(), peer)?;
    let flags = (if flags & SOCK_NONBLOCK != 0 { O_NONBLOCK } else { 0 }) | (if flags & SOCK_CLOEXEC != 0 { O_CLOEXEC } else { 0 });
    install(Socket::wrap(child, Proto::Tcp), flags)
}

pub fn sendto(a: &mut Args) -> KResult<Ret> {
    let f = socket_of(a.a0())?;
    let s = sock(&f);
    let len = (a.a2() as usize).min(1 << 20);
    let mut data = alloc::vec![0u8; len];
    uaccess::copy_from_user(&mut data, a.a1())?;
    let to = if a.a4() != 0 { Some(read_addr(a.a4(), a.a5())?) } else { None };
    let nb = nonblock(&f) || a.a3() as u32 & MSG_DONTWAIT != 0;
    value(s.send_to(&data, to, nb)? as u64)
}

pub fn recvfrom(a: &mut Args) -> KResult<Ret> {
    let f = socket_of(a.a0())?;
    let s = sock(&f);
    let len = (a.a2() as usize).min(1 << 20);
    uaccess::check(a.a1(), len, true)?;
    let mut buf = alloc::vec![0u8; len];
    let nb = nonblock(&f) || a.a3() as u32 & MSG_DONTWAIT != 0;
    let (n, ip, port) = s.recv_from(&mut buf, nb)?;
    uaccess::copy_to_user(a.a1(), &buf[..n])?;
    write_addr(a.a4(), a.a5(), (ip, port))?;
    value(n as u64)
}

#[derive(Clone, Copy)]
#[repr(C)]
struct IoVec {
    base: u64,
    len: u64,
}

fn iovecs(msg: &MsgHdr) -> KResult<Vec<IoVec>> {
    if msg.iovlen > 1024 {
        return Err(Errno::EMSGSIZE);
    }
    (0..msg.iovlen).map(|i| uaccess::read_user::<IoVec>(msg.iov + i * 16)).collect()
}

pub fn sendmsg(a: &mut Args) -> KResult<Ret> {
    let f = socket_of(a.a0())?;
    let s = sock(&f);
    let msg: MsgHdr = uaccess::read_user(a.a1())?;
    let mut data = Vec::new();
    for v in iovecs(&msg)? {
        let start = data.len();
        data.resize(start + v.len as usize, 0);
        uaccess::copy_from_user(&mut data[start..], v.base)?;
    }
    let to = if msg.name != 0 { Some(read_addr(msg.name, msg.namelen as u64)?) } else { None };
    let nb = nonblock(&f) || a.a2() as u32 & MSG_DONTWAIT != 0;
    value(s.send_to(&data, to, nb)? as u64)
}

pub fn recvmsg(a: &mut Args) -> KResult<Ret> {
    let f = socket_of(a.a0())?;
    let s = sock(&f);
    let mut msg: MsgHdr = uaccess::read_user(a.a1())?;
    let vecs = iovecs(&msg)?;
    let total: usize = vecs.iter().map(|v| v.len as usize).sum::<usize>().min(1 << 20);
    let mut buf = alloc::vec![0u8; total];
    let nb = nonblock(&f) || a.a2() as u32 & MSG_DONTWAIT != 0;
    let (n, ip, port) = s.recv_from(&mut buf, nb)?;
    let mut off = 0;
    for v in vecs {
        if off >= n {
            break;
        }
        let k = (v.len as usize).min(n - off);
        uaccess::copy_to_user(v.base, &buf[off..off + k])?;
        off += k;
    }
    if msg.name != 0 {
        let sa = SockaddrIn::new(ip.0, port);
        let bytes = unsafe { core::slice::from_raw_parts(&sa as *const SockaddrIn as *const u8, 16) };
        uaccess::copy_to_user(msg.name, &bytes[..(msg.namelen as usize).min(16)])?;
        msg.namelen = 16;
    }
    msg.controllen = 0;
    msg.flags = 0;
    uaccess::write_user(a.a1(), &msg)?;
    value(n as u64)
}

pub fn shutdown(a: &mut Args) -> KResult<Ret> {
    let f = socket_of(a.a0())?;
    let s = sock(&f);
    let id = s.id;
    match a.a1() as u32 {
        SHUT_RD => {}
        SHUT_WR | SHUT_RDWR => with(|st, now| st.shutdown_write(id, now)).map_err(errno)?,
        _ => return Err(Errno::EINVAL),
    }
    value(0)
}

pub fn getsockname(a: &mut Args) -> KResult<Ret> {
    let f = socket_of(a.a0())?;
    let s = sock(&f);
    let id = s.id;
    let addr = with(|st, _| st.local_addr(id)).unwrap_or_default();
    write_addr(a.a1(), a.a2(), addr)?;
    value(0)
}

pub fn getpeername(a: &mut Args) -> KResult<Ret> {
    let f = socket_of(a.a0())?;
    let s = sock(&f);
    let id = s.id;
    let addr = with(|st, _| st.peer_addr(id)).ok_or(Errno::ENOTCONN)?;
    write_addr(a.a1(), a.a2(), addr)?;
    value(0)
}

fn timeval_ms(addr: u64) -> KResult<u64> {
    let tv: [i64; 2] = uaccess::read_user(addr)?;
    Ok((tv[0].max(0) as u64) * 1000 + (tv[1].max(0) as u64) / 1000)
}

pub fn setsockopt(a: &mut Args) -> KResult<Ret> {
    let f = socket_of(a.a0())?;
    let s = sock(&f);
    if a.a1() as u32 == SOL_SOCKET {
        match a.a2() as u32 {
            SO_RCVTIMEO => s.recv_timeout.store(timeval_ms(a.a3())?, Ordering::Relaxed),
            SO_SNDTIMEO => s.send_timeout.store(timeval_ms(a.a3())?, Ordering::Relaxed),
            _ => {}
        }
    }
    // Other options (SO_REUSEADDR, TCP_NODELAY, buffer sizes...) are
    // accepted and have no effect.
    value(0)
}

pub fn getsockopt(a: &mut Args) -> KResult<Ret> {
    let f = socket_of(a.a0())?;
    let s = sock(&f);
    let id = s.id;
    let v: i32 = match (a.a1() as u32, a.a2() as u32) {
        (SOL_SOCKET, SO_ERROR) => match with(|st, _| st.connect_result(id)) {
            Some(Err(e)) if s.proto == Proto::Tcp => errno(e).code(),
            _ => 0,
        },
        (SOL_SOCKET, SO_TYPE) => (if s.proto == Proto::Tcp { SOCK_STREAM } else { SOCK_DGRAM }) as i32,
        (SOL_SOCKET, SO_RCVBUF | SO_SNDBUF) => 65536,
        _ => 0,
    };
    if a.a3() != 0 && a.a4() != 0 {
        uaccess::write_user(a.a3(), &v)?;
        uaccess::write_user(a.a4(), &4u32)?;
    }
    value(0)
}
