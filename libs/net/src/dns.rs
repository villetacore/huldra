//! DNS (RFC 1035) A-record queries, for the resolver in user space.

use crate::wire::Ip;
use alloc::vec::Vec;

pub const PORT: u16 = 53;

/// A recursive query for the IPv4 addresses of `name`.
pub fn query(id: u16, name: &str) -> Option<Vec<u8>> {
    let mut q = Vec::with_capacity(18 + name.len());
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]); // RD, 1 question
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&[0, 1, 0, 1]); // A, IN
    Some(q)
}

fn skip_name(m: &[u8], mut i: usize) -> Option<usize> {
    loop {
        let len = *m.get(i)? as usize;
        if len == 0 {
            return Some(i + 1);
        }
        if len & 0xC0 == 0xC0 {
            return Some(i + 2);
        }
        i += 1 + len;
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Addresses(Vec<Ip>),
    /// The name does not exist.
    NotFound,
    /// Server failure or refusal (rcode).
    Error(u8),
}

/// Parses the reply to query `id`.
pub fn parse_reply(m: &[u8], id: u16) -> Option<Answer> {
    if m.len() < 12 || m[0..2] != id.to_be_bytes() || m[2] & 0x80 == 0 {
        return None;
    }
    let rcode = m[3] & 0xF;
    if rcode == 3 {
        return Some(Answer::NotFound);
    }
    if rcode != 0 {
        return Some(Answer::Error(rcode));
    }
    let qd = u16::from_be_bytes([m[4], m[5]]);
    let an = u16::from_be_bytes([m[6], m[7]]);
    let mut i = 12;
    for _ in 0..qd {
        i = skip_name(m, i)? + 4;
    }
    let mut out = Vec::new();
    for _ in 0..an {
        i = skip_name(m, i)?;
        let rr = m.get(i..i + 10)?;
        let kind = u16::from_be_bytes([rr[0], rr[1]]);
        let len = u16::from_be_bytes([rr[8], rr[9]]) as usize;
        let data = m.get(i + 10..i + 10 + len)?;
        if kind == 1 && len == 4 {
            out.push(Ip(data.try_into().unwrap()));
        }
        i += 10 + len;
    }
    Some(if out.is_empty() { Answer::NotFound } else { Answer::Addresses(out) })
}

/// Looks `name` up in hosts-file text.
pub fn hosts_lookup(hosts: &str, name: &str) -> Option<Ip> {
    for line in hosts.lines() {
        let line = line.split('#').next().unwrap_or("");
        let mut fields = line.split_whitespace();
        let Some(addr) = fields.next().and_then(Ip::parse) else { continue };
        if fields.any(|f| f.eq_ignore_ascii_case(name)) {
            return Some(addr);
        }
    }
    None
}

/// Name servers listed in resolv.conf text.
pub fn nameservers(resolv: &str) -> Vec<Ip> {
    resolv
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            (f.next() == Some("nameserver")).then(|| f.next().and_then(Ip::parse)).flatten()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_and_reply() {
        let q = query(0x1234, "example.com").unwrap();
        assert_eq!(&q[12..], b"\x07example\x03com\x00\x00\x01\x00\x01");
        // Reply: the question, then an answer using a compression pointer.
        let mut r = q.clone();
        r[2] = 0x81;
        r[3] = 0x80;
        r[7] = 1;
        r.extend_from_slice(&[0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 93, 184, 216, 34]);
        assert_eq!(parse_reply(&r, 0x1234), Some(Answer::Addresses(alloc::vec![Ip([93, 184, 216, 34])])));
        r[3] = 0x83;
        assert_eq!(parse_reply(&r, 0x1234), Some(Answer::NotFound));
        assert_eq!(parse_reply(&r, 1), None);
    }

    #[test]
    fn config_files() {
        assert_eq!(hosts_lookup("127.0.0.1 localhost huldra\n# x\n10.0.0.1 gw", "huldra"), Some(Ip::LOCALHOST));
        assert_eq!(nameservers("# c\nnameserver 10.0.2.3\nsearch x\n"), alloc::vec![Ip([10, 0, 2, 3])]);
    }
}
