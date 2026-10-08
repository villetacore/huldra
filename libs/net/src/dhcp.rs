//! DHCP (RFC 2131) messages for a client.

use crate::wire::{Ip, Mac};
use alloc::vec::Vec;

pub const SERVER_PORT: u16 = 67;
pub const CLIENT_PORT: u16 = 68;

pub const DISCOVER: u8 = 1;
pub const OFFER: u8 = 2;
pub const REQUEST: u8 = 3;
pub const ACK: u8 = 5;
pub const NAK: u8 = 6;

const MAGIC: [u8; 4] = [99, 130, 83, 99];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lease {
    pub ip: Ip,
    pub netmask: Ip,
    pub router: Ip,
    pub dns: Ip,
    pub server: Ip,
    pub lease_secs: u32,
}

/// A client message (DISCOVER, or REQUEST for `requested` from `server`).
pub fn client_message(kind: u8, xid: u32, mac: Mac, requested: Option<(Ip, Ip)>) -> Vec<u8> {
    let mut m = alloc::vec![0u8; 236];
    m[0] = 1; // BOOTREQUEST
    m[1] = 1; // Ethernet
    m[2] = 6;
    m[4..8].copy_from_slice(&xid.to_be_bytes());
    m[10] = 0x80; // broadcast replies: we have no address yet
    m[28..34].copy_from_slice(&mac.0);
    m.extend_from_slice(&MAGIC);
    m.extend_from_slice(&[53, 1, kind]);
    if let Some((ip, server)) = requested {
        m.extend_from_slice(&[50, 4]);
        m.extend_from_slice(&ip.0);
        m.extend_from_slice(&[54, 4]);
        m.extend_from_slice(&server.0);
    }
    m.extend_from_slice(&[55, 3, 1, 3, 6]); // want netmask, router, DNS
    m.extend_from_slice(&[12, 6]);
    m.extend_from_slice(b"huldra");
    m.push(255);
    m
}

/// Parses a server reply for transaction `xid`: (message type, lease).
pub fn parse_reply(m: &[u8], xid: u32) -> Option<(u8, Lease)> {
    if m.len() < 240 || m[0] != 2 || m[4..8] != xid.to_be_bytes() || m[236..240] != MAGIC {
        return None;
    }
    let mut lease = Lease { ip: Ip(m[16..20].try_into().ok()?), ..Lease::default() };
    let mut kind = 0;
    let mut i = 240;
    let ip = |v: &[u8]| v.get(..4).map(|b| Ip(b.try_into().unwrap()));
    while i < m.len() {
        let code = m[i];
        if code == 255 {
            break;
        }
        if code == 0 {
            i += 1;
            continue;
        }
        let len = *m.get(i + 1)? as usize;
        let v = m.get(i + 2..i + 2 + len)?;
        match code {
            53 => kind = *v.first()?,
            1 => lease.netmask = ip(v)?,
            3 => lease.router = ip(v)?,
            6 => lease.dns = ip(v)?,
            54 => lease.server = ip(v)?,
            51 => lease.lease_secs = u32::from_be_bytes(v.get(..4)?.try_into().ok()?),
            _ => {}
        }
        i += 2 + len;
    }
    (kind != 0).then_some((kind, lease))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server reply, as a test fixture and for the stack tests.
    pub fn reply(kind: u8, xid: u32, mac: Mac, lease: &Lease) -> Vec<u8> {
        let mut m = alloc::vec![0u8; 236];
        m[0] = 2;
        m[1] = 1;
        m[2] = 6;
        m[4..8].copy_from_slice(&xid.to_be_bytes());
        m[16..20].copy_from_slice(&lease.ip.0);
        m[28..34].copy_from_slice(&mac.0);
        m.extend_from_slice(&MAGIC);
        m.extend_from_slice(&[53, 1, kind, 1, 4]);
        m.extend_from_slice(&lease.netmask.0);
        m.extend_from_slice(&[3, 4]);
        m.extend_from_slice(&lease.router.0);
        m.extend_from_slice(&[6, 4]);
        m.extend_from_slice(&lease.dns.0);
        m.extend_from_slice(&[54, 4]);
        m.extend_from_slice(&lease.server.0);
        m.extend_from_slice(&[51, 4]);
        m.extend_from_slice(&lease.lease_secs.to_be_bytes());
        m.push(255);
        m
    }

    #[test]
    fn round_trip() {
        let mac = Mac([2, 0, 0, 0, 0, 1]);
        let d = client_message(DISCOVER, 42, mac, None);
        assert_eq!(&d[236..240], &MAGIC);
        let lease = Lease { ip: Ip([10, 0, 2, 15]), netmask: Ip([255, 255, 255, 0]), router: Ip([10, 0, 2, 2]), dns: Ip([10, 0, 2, 3]), server: Ip([10, 0, 2, 2]), lease_secs: 86400 };
        let r = reply(OFFER, 42, mac, &lease);
        assert_eq!(parse_reply(&r, 42), Some((OFFER, lease)));
        assert_eq!(parse_reply(&r, 43), None);
    }
}

#[cfg(test)]
pub use tests::reply as test_reply;
