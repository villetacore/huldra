//! The smart HTTP protocol (version 0/1): pkt-lines, the reference
//! advertisement, upload-pack (fetch) and receive-pack (push) requests
//! and responses.

use crate::object::Id;
use crate::{Error, Result};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// A pkt-line with `data`.
pub fn pkt(data: &[u8]) -> Vec<u8> {
    let mut out = format!("{:04x}", data.len() + 4).into_bytes();
    out.extend_from_slice(data);
    out
}

pub const FLUSH: &[u8] = b"0000";

#[derive(Debug, PartialEq, Eq)]
pub enum Pkt<'a> {
    Data(&'a [u8]),
    Flush,
}

/// Splits a buffer into pkt-lines.
pub fn parse_pkts(mut data: &[u8]) -> Result<Vec<Pkt<'_>>> {
    let mut out = Vec::new();
    while !data.is_empty() {
        let len = data.get(..4).and_then(|h| core::str::from_utf8(h).ok()).and_then(|h| usize::from_str_radix(h, 16).ok());
        let len = len.ok_or_else(|| Error("protocol: bad pkt-line".into()))?;
        if len < 4 {
            out.push(Pkt::Flush); // 0000 flush, 0001 delim, 0002 response end
            data = &data[4..];
            continue;
        }
        let body = data.get(4..len).ok_or_else(|| Error("protocol: truncated pkt-line".into()))?;
        out.push(Pkt::Data(body));
        data = &data[len..];
    }
    Ok(out)
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).trim_end_matches('\n').to_string()
}

/// What a server announces for `info/refs?service=...`.
#[derive(Debug, Default)]
pub struct Advertisement {
    pub refs: Vec<(String, Id)>,
    pub caps: Vec<String>,
}

impl Advertisement {
    pub fn parse(body: &[u8]) -> Result<Advertisement> {
        let mut adv = Advertisement::default();
        let mut first = true;
        for p in parse_pkts(body)? {
            let Pkt::Data(d) = p else { continue };
            let line = text(d);
            if line.starts_with("# service=") {
                continue;
            }
            if line.starts_with("ERR ") {
                return Err(Error(format!("server: {}", &line[4..])));
            }
            let (refpart, caps) = match line.split_once('\0') {
                Some((r, c)) => (r.to_string(), Some(c.to_string())),
                None => (line, None),
            };
            if first {
                first = false;
                if let Some(c) = caps {
                    adv.caps = c.split_whitespace().map(String::from).collect();
                }
            }
            let (id, name) = refpart.split_once(' ').ok_or_else(|| Error("protocol: bad ref line".into()))?;
            let id = Id::from_hex(id).ok_or_else(|| Error("protocol: bad ref id".into()))?;
            if name != "capabilities^{}" {
                adv.refs.push((name.to_string(), id));
            }
        }
        Ok(adv)
    }

    pub fn has(&self, cap: &str) -> bool {
        self.caps.iter().any(|c| c == cap || c.starts_with(&format!("{}=", cap)))
    }

    /// Where HEAD points (`symref=HEAD:refs/heads/main`).
    pub fn head_target(&self) -> Option<&str> {
        self.caps.iter().find_map(|c| c.strip_prefix("symref=HEAD:"))
    }

    pub fn get(&self, name: &str) -> Option<Id> {
        self.refs.iter().find(|(n, _)| n == name).map(|(_, id)| *id)
    }
}

/// The body of `POST git-upload-pack`.
pub fn upload_request(wants: &[Id], haves: &[Id], depth: Option<u32>, adv: &Advertisement) -> Vec<u8> {
    // No multi_ack: the client sends all its haves, then "done".
    let mut caps: Vec<&str> = ["side-band-64k", "ofs-delta"].into_iter().filter(|c| adv.has(c)).collect();
    caps.push("agent=huldra-git");
    let mut out = Vec::new();
    for (i, w) in wants.iter().enumerate() {
        let line = if i == 0 { format!("want {} {}\n", w, caps.join(" ")) } else { format!("want {}\n", w) };
        out.extend_from_slice(&pkt(line.as_bytes()));
    }
    if let Some(d) = depth {
        out.extend_from_slice(&pkt(format!("deepen {}\n", d).as_bytes()));
    }
    out.extend_from_slice(FLUSH);
    for h in haves {
        out.extend_from_slice(&pkt(format!("have {}\n", h).as_bytes()));
    }
    out.extend_from_slice(&pkt(b"done\n"));
    out
}

/// What came back from upload-pack.
#[derive(Debug, Default)]
pub struct UploadResponse {
    pub pack: Vec<u8>,
    /// Progress messages (side-band channel 2).
    pub progress: String,
    pub shallow: Vec<Id>,
}

/// Parses the upload-pack response: shallow lines, ACK/NAK, then the pack,
/// either raw or multiplexed in side-band packets.
pub fn parse_upload_response(body: &[u8], sideband: bool) -> Result<UploadResponse> {
    let mut r = UploadResponse::default();
    let mut p = 0;
    // Lines before the pack.
    loop {
        if body.get(p..p + 4) == Some(b"PACK") {
            r.pack = body[p..].to_vec();
            return Ok(r);
        }
        let len = body.get(p..p + 4).and_then(|h| core::str::from_utf8(h).ok()).and_then(|h| usize::from_str_radix(h, 16).ok());
        let Some(len) = len else {
            return Err(Error("protocol: unexpected data in the response".into()));
        };
        if len == 0 {
            p += 4;
            continue;
        }
        let d = body.get(p + 4..p + len).ok_or_else(|| Error("protocol: truncated response".into()))?;
        p += len;
        if sideband && d.first().is_some_and(|&c| c == 1 || c == 2 || c == 3) {
            // From here on everything is side-band.
            p -= len;
            break;
        }
        let line = text(d);
        if let Some(id) = line.strip_prefix("shallow ") {
            r.shallow.push(Id::from_hex(id).ok_or_else(|| Error("protocol: bad shallow".into()))?);
        } else if let Some(msg) = line.strip_prefix("ERR ") {
            return Err(Error(format!("server: {}", msg)));
        }
        // NAK, ACK ..., unshallow: nothing to do.
    }
    for pk in parse_pkts(&body[p..])? {
        let Pkt::Data(d) = pk else { continue };
        match d.first() {
            Some(1) => r.pack.extend_from_slice(&d[1..]),
            Some(2) => r.progress.push_str(&String::from_utf8_lossy(&d[1..])),
            Some(3) => return Err(Error(format!("server: {}", text(&d[1..])))),
            _ => {}
        }
    }
    Ok(r)
}

/// The body of `POST git-receive-pack`: one command per ref, then the pack.
pub fn push_request(updates: &[(Id, Id, String)], pack: &[u8], adv: &Advertisement) -> Vec<u8> {
    let mut caps = Vec::new();
    if adv.has("report-status") {
        caps.push("report-status");
    }
    if adv.has("side-band-64k") {
        caps.push("side-band-64k");
    }
    caps.push("agent=huldra-git");
    let mut out = Vec::new();
    for (i, (old, new, name)) in updates.iter().enumerate() {
        let line = if i == 0 { format!("{} {} {}\0{}\n", old, new, name, caps.join(" ")) } else { format!("{} {} {}\n", old, new, name) };
        out.extend_from_slice(&pkt(line.as_bytes()));
    }
    out.extend_from_slice(FLUSH);
    out.extend_from_slice(pack);
    out
}

/// Reads report-status: Ok(()) if every ref was updated.
pub fn parse_push_response(body: &[u8], sideband: bool) -> Result<String> {
    let mut report = Vec::new();
    let mut messages = String::new();
    if sideband {
        for pk in parse_pkts(body)? {
            let Pkt::Data(d) = pk else { continue };
            match d.first() {
                Some(1) => report.extend_from_slice(&d[1..]),
                Some(2) => messages.push_str(&String::from_utf8_lossy(&d[1..])),
                Some(3) => return Err(Error(format!("server: {}", text(&d[1..])))),
                _ => {}
            }
        }
    } else {
        report = body.to_vec();
    }
    for pk in parse_pkts(&report)? {
        let Pkt::Data(d) = pk else { continue };
        let line = text(d);
        if let Some(rest) = line.strip_prefix("unpack ") {
            if rest != "ok" {
                return Err(Error(format!("server could not unpack: {}", rest)));
            }
        } else if let Some(rest) = line.strip_prefix("ng ") {
            return Err(Error(format!("rejected: {}", rest)));
        }
    }
    Ok(messages)
}
