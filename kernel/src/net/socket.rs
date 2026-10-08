//! Sockets as files: reads and writes on a socket descriptor are stream or
//! datagram I/O, `poll` reports readiness, the last `close` closes it.

use super::{with, EVENTS};
use crate::fs::vfs::*;
use alloc::sync::Arc;
use core::any::Any;
use core::sync::atomic::{AtomicU64, Ordering};
use huldra_abi::errno::Errno;
use huldra_net::{Ip, NetError, Proto, SocketId};

static NEXT_INO: AtomicU64 = AtomicU64::new(1);

pub struct Socket {
    pub id: SocketId,
    pub proto: Proto,
    ino: u64,
    /// SO_RCVTIMEO / SO_SNDTIMEO in milliseconds (0: none).
    pub recv_timeout: AtomicU64,
    pub send_timeout: AtomicU64,
}

pub fn errno(e: NetError) -> Errno {
    match e {
        NetError::WouldBlock => Errno::EAGAIN,
        NetError::InProgress => Errno::EALREADY,
        NetError::AddrInUse => Errno::EADDRINUSE,
        NetError::AddrNotAvailable => Errno::EADDRNOTAVAIL,
        NetError::NotConnected => Errno::ENOTCONN,
        NetError::IsConnected => Errno::EISCONN,
        NetError::Refused => Errno::ECONNREFUSED,
        NetError::Reset => Errno::ECONNRESET,
        NetError::TimedOut => Errno::ETIMEDOUT,
        NetError::NetUnreachable => Errno::ENETUNREACH,
        NetError::Invalid => Errno::EINVAL,
        NetError::NotSupported => Errno::EOPNOTSUPP,
        NetError::MessageSize => Errno::EMSGSIZE,
        NetError::BadSocket => Errno::EBADF,
    }
}

impl Socket {
    pub fn new(proto: Proto) -> Arc<Socket> {
        let id = with(|s, _| s.socket(proto));
        Self::wrap(id, proto)
    }

    pub fn wrap(id: SocketId, proto: Proto) -> Arc<Socket> {
        Arc::new(Socket { id, proto, ino: NEXT_INO.fetch_add(1, Ordering::Relaxed), recv_timeout: AtomicU64::new(0), send_timeout: AtomicU64::new(0) })
    }

    /// Blocks until `op` stops returning WouldBlock (or the timeout).
    pub fn block<T>(&self, nonblock: bool, timeout_ms: u64, mut op: impl FnMut(&mut huldra_net::Stack, u64) -> Result<T, NetError>) -> Result<T, Errno> {
        if nonblock {
            return with(&mut op).map_err(errno);
        }
        let mut cond = || match with(&mut op) {
            Err(NetError::WouldBlock) => None,
            r => Some(r),
        };
        let r = if timeout_ms == 0 {
            EVENTS.wait_until(cond)?
        } else {
            let deadline = crate::time::ticks() + crate::time::ms_to_ticks(timeout_ms);
            EVENTS.wait_until_deadline(&mut cond, deadline)?.ok_or(Errno::EAGAIN)?
        };
        r.map_err(errno)
    }

    pub fn recv_from(&self, buf: &mut [u8], nonblock: bool) -> Result<(usize, Ip, u16), Errno> {
        let id = self.id;
        self.block(nonblock, self.recv_timeout.load(Ordering::Relaxed), |s, _| s.recv_from(id, buf))
    }

    /// Sends all of `data` (streams block while the buffer is full).
    pub fn send_to(&self, data: &[u8], to: Option<(Ip, u16)>, nonblock: bool) -> Result<usize, Errno> {
        let id = self.id;
        let mut sent = 0;
        let timeout = self.send_timeout.load(Ordering::Relaxed);
        while sent < data.len() || (data.is_empty() && sent == 0) {
            let r = self.block(nonblock, timeout, |s, now| match to {
                Some((ip, port)) => s.send_to(id, &data[sent..], ip, port, now),
                None => s.send(id, &data[sent..], now),
            });
            match r {
                Ok(n) => {
                    sent += n;
                    if self.proto != Proto::Tcp || n == 0 {
                        break;
                    }
                }
                Err(Errno::EAGAIN) if sent > 0 => break,
                Err(Errno::ENOTCONN) | Err(Errno::ECONNRESET) if self.proto == Proto::Tcp => {
                    if sent > 0 {
                        break;
                    }
                    crate::proc::signal::send_current(huldra_abi::signal::SIGPIPE);
                    return Err(Errno::EPIPE);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(sent)
    }
}

impl Inode for Socket {
    fn metadata(&self) -> Metadata {
        Metadata::new(0, self.ino, FileType::Socket, 0o777)
    }

    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> KResult<usize> {
        self.recv_from(buf, false).map(|r| r.0)
    }

    fn write_at(&self, _offset: u64, buf: &[u8]) -> KResult<usize> {
        self.send_to(buf, None, false)
    }

    fn poll(&self) -> PollState {
        let r = with(|s, _| s.readiness(self.id));
        PollState { readable: r.readable, writable: r.writable, hangup: r.hangup }
    }

    fn release(&self, _flags: u32) {
        let id = self.id;
        with(|s, now| s.close(id, now));
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
