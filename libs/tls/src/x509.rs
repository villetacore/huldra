//! X.509 certificates (RFC 5280): just enough DER to read names, validity,
//! keys and the extensions that matter for a TLS client, and to verify a
//! chain up to a trusted root.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use huldra_crypto::sig::{ecdsa_verify, Curve, HashAlg, RsaPublicKey};

pub type Result<T> = core::result::Result<T, String>;

// ----------------------------------------------------------------------- DER

/// One TLV.
#[derive(Clone, Copy, Debug)]
pub struct Der<'a> {
    pub tag: u8,
    pub value: &'a [u8],
    /// The whole encoding including tag and length.
    pub raw: &'a [u8],
}

/// Reads the first TLV of `data`; returns it and the rest.
pub fn read(data: &[u8]) -> Result<(Der<'_>, &[u8])> {
    let tag = *data.first().ok_or("DER: empty")?;
    let first = *data.get(1).ok_or("DER: truncated length")?;
    let (len, hdr) = if first < 0x80 {
        (first as usize, 2)
    } else {
        let n = (first & 0x7F) as usize;
        if n == 0 || n > 4 {
            return Err("DER: bad length".into());
        }
        let mut len = 0usize;
        for i in 0..n {
            len = len << 8 | *data.get(2 + i).ok_or("DER: truncated length")? as usize;
        }
        (len, 2 + n)
    };
    let end = hdr.checked_add(len).filter(|&e| e <= data.len()).ok_or("DER: value runs past the end")?;
    Ok((Der { tag, value: &data[hdr..end], raw: &data[..end] }, &data[end..]))
}

impl<'a> Der<'a> {
    /// The elements of a SEQUENCE or SET.
    pub fn children(&self) -> Result<Vec<Der<'a>>> {
        let mut out = Vec::new();
        let mut rest = self.value;
        while !rest.is_empty() {
            let (d, r) = read(rest)?;
            out.push(d);
            rest = r;
        }
        Ok(out)
    }

    fn expect(self, tag: u8) -> Result<Der<'a>> {
        if self.tag == tag {
            Ok(self)
        } else {
            Err(format!("DER: expected tag {:#x}, found {:#x}", tag, self.tag))
        }
    }
}

const SEQUENCE: u8 = 0x30;
const OID: u8 = 0x06;
const BIT_STRING: u8 = 0x03;
const OCTET_STRING: u8 = 0x04;
const INTEGER: u8 = 0x02;
const BOOLEAN: u8 = 0x01;

/// An object identifier in dotted form.
pub fn oid_string(v: &[u8]) -> String {
    let mut parts: Vec<u64> = Vec::new();
    let mut acc = 0u64;
    for (i, &b) in v.iter().enumerate() {
        acc = acc << 7 | (b & 0x7F) as u64;
        if b & 0x80 == 0 {
            if i == 0 || parts.is_empty() {
                let (a, rest) = if acc < 40 { (0, acc) } else if acc < 80 { (1, acc - 40) } else { (2, acc - 80) };
                parts.push(a);
                parts.push(rest);
            } else {
                parts.push(acc);
            }
            acc = 0;
        }
    }
    parts.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(".")
}

fn bit_string(d: Der<'_>) -> Result<&[u8]> {
    let d = d.expect(BIT_STRING)?;
    match d.value.split_first() {
        Some((0, bits)) => Ok(bits),
        _ => Err("DER: unsupported BIT STRING".into()),
    }
}

/// Days since 1970-01-01 for a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// UTCTime or GeneralizedTime as Unix seconds.
fn time(d: Der) -> Result<i64> {
    let s = core::str::from_utf8(d.value).map_err(|_| "bad time")?;
    let digits = |r: core::ops::Range<usize>| -> Result<i64> { s.get(r).and_then(|x| x.parse().ok()).ok_or_else(|| format!("bad time '{}'", s)) };
    let (year, rest) = match d.tag {
        0x17 => {
            let y = digits(0..2)?;
            (if y >= 50 { 1900 + y } else { 2000 + y }, 2)
        }
        0x18 => (digits(0..4)?, 4),
        _ => return Err("bad time type".into()),
    };
    let (mo, da, h, mi, se) = (digits(rest..rest + 2)?, digits(rest + 2..rest + 4)?, digits(rest + 4..rest + 6)?, digits(rest + 6..rest + 8)?, digits(rest + 8..rest + 10)?);
    Ok(days_from_civil(year, mo, da) * 86400 + h * 3600 + mi * 60 + se)
}

// ----------------------------------------------------------------------- keys

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicKey {
    Rsa { n: Vec<u8>, e: Vec<u8> },
    Ec { curve: Curve, point: Vec<u8> },
}

fn public_key(spki: Der) -> Result<PublicKey> {
    let parts = spki.expect(SEQUENCE)?.children()?;
    let alg = parts.first().ok_or("SPKI: no algorithm")?.children()?;
    let oid = oid_string(alg.first().ok_or("SPKI: no OID")?.expect(OID)?.value);
    let bits = bit_string(*parts.get(1).ok_or("SPKI: no key")?)?;
    match oid.as_str() {
        "1.2.840.113549.1.1.1" => {
            let (seq, _) = read(bits)?;
            let ne = seq.expect(SEQUENCE)?.children()?;
            let int = |d: &Der| -> Result<Vec<u8>> { Ok(strip(d.expect(INTEGER)?.value).to_vec()) };
            Ok(PublicKey::Rsa { n: int(ne.first().ok_or("RSA: no n")?)?, e: int(ne.get(1).ok_or("RSA: no e")?)? })
        }
        "1.2.840.10045.2.1" => {
            let curve = match alg.get(1).map(|d| oid_string(d.value)).as_deref() {
                Some("1.2.840.10045.3.1.7") => Curve::P256,
                Some("1.3.132.0.34") => Curve::P384,
                other => return Err(format!("unsupported curve {:?}", other)),
            };
            Ok(PublicKey::Ec { curve, point: bits.to_vec() })
        }
        other => Err(format!("unsupported key type {}", other)),
    }
}

fn strip(v: &[u8]) -> &[u8] {
    let n = v.iter().take_while(|&&b| b == 0).count();
    &v[n.min(v.len().saturating_sub(1))..]
}

/// How something was signed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigAlg {
    RsaPkcs1(HashAlg),
    RsaPss(HashAlg),
    Ecdsa(HashAlg),
}

impl PublicKey {
    /// Verifies `sig` over `message`. ECDSA signatures are DER `(r, s)`.
    pub fn verify(&self, alg: SigAlg, message: &[u8], sig: &[u8]) -> bool {
        match (self, alg) {
            (PublicKey::Rsa { n, e }, SigAlg::RsaPkcs1(h)) => RsaPublicKey { n: n.clone(), e: e.clone() }.verify_pkcs1(h, message, sig),
            (PublicKey::Rsa { n, e }, SigAlg::RsaPss(h)) => RsaPublicKey { n: n.clone(), e: e.clone() }.verify_pss(h, message, sig),
            (PublicKey::Ec { curve, point }, SigAlg::Ecdsa(h)) => {
                let Ok((seq, _)) = read(sig) else { return false };
                let Ok(rs) = seq.children() else { return false };
                if rs.len() != 2 || rs[0].tag != INTEGER || rs[1].tag != INTEGER {
                    return false;
                }
                ecdsa_verify(*curve, point, &h.digest(message), rs[0].value, rs[1].value)
            }
            _ => false,
        }
    }
}

fn sig_alg(d: Der) -> Result<SigAlg> {
    let parts = d.expect(SEQUENCE)?.children()?;
    let oid = oid_string(parts.first().ok_or("no signature algorithm")?.expect(OID)?.value);
    Ok(match oid.as_str() {
        "1.2.840.113549.1.1.11" => SigAlg::RsaPkcs1(HashAlg::Sha256),
        "1.2.840.113549.1.1.12" => SigAlg::RsaPkcs1(HashAlg::Sha384),
        "1.2.840.113549.1.1.13" => SigAlg::RsaPkcs1(HashAlg::Sha512),
        "1.2.840.113549.1.1.5" => SigAlg::RsaPkcs1(HashAlg::Sha1),
        "1.2.840.10045.4.3.2" => SigAlg::Ecdsa(HashAlg::Sha256),
        "1.2.840.10045.4.3.3" => SigAlg::Ecdsa(HashAlg::Sha384),
        "1.2.840.10045.4.3.4" => SigAlg::Ecdsa(HashAlg::Sha512),
        "1.2.840.113549.1.1.10" => {
            // RSASSA-PSS-params: [0] hashAlgorithm; salt length assumed = hash length.
            let mut hash = HashAlg::Sha1;
            if let Some(params) = parts.get(1) {
                for p in params.children()? {
                    if p.tag == 0xA0 {
                        let (alg, _) = read(p.value)?;
                        let o = oid_string(alg.children()?.first().ok_or("PSS: no hash")?.value);
                        hash = match o.as_str() {
                            "2.16.840.1.101.3.4.2.1" => HashAlg::Sha256,
                            "2.16.840.1.101.3.4.2.2" => HashAlg::Sha384,
                            "2.16.840.1.101.3.4.2.3" => HashAlg::Sha512,
                            _ => return Err(format!("PSS hash {}", o)),
                        };
                    }
                }
            }
            SigAlg::RsaPss(hash)
        }
        other => return Err(format!("unsupported signature algorithm {}", other)),
    })
}

// ---------------------------------------------------------------- certificates

#[derive(Clone, Debug)]
pub struct Certificate {
    pub der: Vec<u8>,
    tbs_range: (usize, usize),
    pub sig_alg: SigAlg,
    pub signature: Vec<u8>,
    pub issuer: Vec<u8>,
    pub subject: Vec<u8>,
    pub not_before: i64,
    pub not_after: i64,
    pub key: PublicKey,
    pub dns_names: Vec<String>,
    pub ip_addresses: Vec<Vec<u8>>,
    pub is_ca: bool,
}

impl Certificate {
    pub fn parse(der: &[u8]) -> Result<Certificate> {
        let (cert, _) = read(der)?;
        let parts = cert.expect(SEQUENCE)?.children()?;
        if parts.len() < 3 {
            return Err("certificate: too few fields".into());
        }
        let tbs = parts[0].expect(SEQUENCE)?;
        let start = tbs.raw.as_ptr() as usize - der.as_ptr() as usize;
        let sig_alg = sig_alg(parts[1]).unwrap_or(SigAlg::RsaPkcs1(HashAlg::Sha1));
        let signature = bit_string(parts[2])?.to_vec();
        let mut fields = tbs.children()?.into_iter();
        let mut f = fields.next().ok_or("tbs: empty")?;
        if f.tag == 0xA0 {
            f = fields.next().ok_or("tbs: no serial")?; // version
        }
        let _serial = f;
        let _alg = fields.next().ok_or("tbs: no algorithm")?;
        let issuer = fields.next().ok_or("tbs: no issuer")?.raw.to_vec();
        let validity = fields.next().ok_or("tbs: no validity")?.children()?;
        let subject = fields.next().ok_or("tbs: no subject")?.raw.to_vec();
        let key = public_key(fields.next().ok_or("tbs: no key")?)?;
        let mut c = Certificate {
            der: der[..cert.raw.len()].to_vec(),
            tbs_range: (start, start + tbs.raw.len()),
            sig_alg,
            signature,
            issuer,
            subject,
            not_before: time(*validity.first().ok_or("no notBefore")?)?,
            not_after: time(*validity.get(1).ok_or("no notAfter")?)?,
            key,
            dns_names: Vec::new(),
            ip_addresses: Vec::new(),
            is_ca: false,
        };
        for f in fields {
            if f.tag != 0xA3 {
                continue;
            }
            let (exts, _) = read(f.value)?;
            for ext in exts.children()? {
                let e = ext.children()?;
                let oid = oid_string(e.first().ok_or("extension without OID")?.value);
                let value = e.last().ok_or("extension without value")?.expect(OCTET_STRING)?.value;
                match oid.as_str() {
                    "2.5.29.17" => {
                        let (names, _) = read(value)?;
                        for n in names.children()? {
                            match n.tag {
                                0x82 => c.dns_names.push(String::from_utf8_lossy(n.value).to_ascii_lowercase()),
                                0x87 => c.ip_addresses.push(n.value.to_vec()),
                                _ => {}
                            }
                        }
                    }
                    "2.5.29.19" => {
                        let (bc, _) = read(value)?;
                        c.is_ca = bc.children()?.first().is_some_and(|b| b.tag == BOOLEAN && b.value.first() == Some(&0xFF));
                    }
                    _ => {}
                }
            }
        }
        Ok(c)
    }

    fn tbs(&self) -> &[u8] {
        &self.der[self.tbs_range.0..self.tbs_range.1]
    }

    /// True if `issuer`'s key made this certificate's signature.
    pub fn signed_by(&self, issuer: &PublicKey) -> bool {
        if matches!(self.sig_alg, SigAlg::RsaPkcs1(HashAlg::Sha1)) {
            return false; // SHA-1 signatures are not trusted
        }
        issuer.verify(self.sig_alg, self.tbs(), &self.signature)
    }

    /// Does the certificate cover `host` (a name or an IPv4 address)?
    pub fn matches_host(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if let Some(ip) = parse_ipv4(&host) {
            return self.ip_addresses.iter().any(|a| a[..] == ip[..]);
        }
        self.dns_names.iter().any(|n| {
            if let Some(suffix) = n.strip_prefix("*.") {
                // A wildcard covers exactly one label.
                host.split_once('.').is_some_and(|(label, rest)| !label.is_empty() && rest == suffix)
            } else {
                *n == host
            }
        })
    }

    /// The subject's common name, for messages.
    pub fn subject_name(&self) -> String {
        common_name(&self.subject).unwrap_or_else(|| "(unnamed)".into())
    }

    pub fn issuer_name(&self) -> String {
        common_name(&self.issuer).unwrap_or_else(|| "(unnamed)".into())
    }
}

fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = s.split('.');
    for o in out.iter_mut() {
        *o = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(out)
}

/// CN (or else O) of a Name.
fn common_name(name: &[u8]) -> Option<String> {
    let (seq, _) = read(name).ok()?;
    let mut org = None;
    for set in seq.children().ok()? {
        for atv in set.children().ok()? {
            let kv = atv.children().ok()?;
            let oid = oid_string(kv.first()?.value);
            let v = String::from_utf8_lossy(kv.get(1)?.value).into_owned();
            match oid.as_str() {
                "2.5.4.3" => return Some(v),
                "2.5.4.10" => org = Some(v),
                _ => {}
            }
        }
    }
    org
}

// -------------------------------------------------------------------- trust

/// Trusted root certificates.
#[derive(Default)]
pub struct RootStore {
    roots: Vec<Certificate>,
}

impl RootStore {
    /// Reads a PEM bundle (like /etc/ssl/certs/ca-certificates.crt);
    /// certificates that do not parse are skipped.
    pub fn from_pem(pem: &str) -> RootStore {
        let mut roots = Vec::new();
        let mut rest = pem;
        while let Some(start) = rest.find("-----BEGIN CERTIFICATE-----") {
            let body = &rest[start + 27..];
            let Some(end) = body.find("-----END CERTIFICATE-----") else { break };
            if let Some(der) = huldra_crypto::base64_decode(&body[..end]) {
                if let Ok(c) = Certificate::parse(&der) {
                    roots.push(c);
                }
            }
            rest = &body[end..];
        }
        RootStore { roots }
    }

    pub fn add(&mut self, der: &[u8]) -> Result<()> {
        self.roots.push(Certificate::parse(der)?);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.roots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// Checks that `chain[0]` is valid for `host` at time `now` and leads to
    /// a trusted root through certificates of the chain.
    pub fn verify(&self, chain: &[Certificate], host: &str, now: i64) -> Result<()> {
        let leaf = chain.first().ok_or("the server sent no certificate")?;
        if !leaf.matches_host(host) {
            return Err(format!("certificate is for {} , not {}", if leaf.dns_names.is_empty() { leaf.subject_name() } else { leaf.dns_names.join(", ") }, host));
        }
        let mut current = leaf;
        for depth in 0..10 {
            if now < current.not_before || now > current.not_after {
                return Err(format!("certificate '{}' is {} (check the clock: date)", current.subject_name(), if now < current.not_before { "not valid yet" } else { "expired" }));
            }
            // Issued by a trusted root?
            if self.roots.iter().any(|r| r.subject == current.issuer && current.signed_by(&r.key)) {
                return Ok(());
            }
            // The leaf itself may be a root (self-signed and trusted).
            if self.roots.iter().any(|r| r.der == current.der) {
                return Ok(());
            }
            let next = chain[1..].iter().find(|c| c.subject == current.issuer && c.is_ca && current.signed_by(&c.key));
            match next {
                Some(c) => current = c,
                None => {
                    return Err(format!(
                        "certificate '{}' (depth {}) was issued by '{}', which is not trusted",
                        current.subject_name(),
                        depth,
                        current.issuer_name()
                    ))
                }
            }
        }
        Err("certificate chain too long".into())
    }
}
