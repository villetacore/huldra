//! A TLS 1.3 client (RFC 8446) as a state machine without I/O.
//!
//! Offered: TLS_CHACHA20_POLY1305_SHA256 and TLS_AES_128_GCM_SHA256, key
//! exchange with X25519, server signatures with ECDSA (P-256, P-384) and
//! RSA-PSS; certificates may also be signed with RSA PKCS #1 v1.5. The
//! server's certificate chain is checked against a [`RootStore`] and the
//! host name, and its CertificateVerify and Finished messages are checked
//! against the handshake transcript.
//!
//! The program moves bytes: [`Client::take_output`] gives what to send,
//! [`Client::feed`] takes what arrived, [`Client::read`] returns
//! decrypted application data and [`Client::write`] encrypts it.

use crate::x509::{Certificate, RootStore, SigAlg};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use huldra_crypto::aead::Aead;
use huldra_crypto::sha::{hkdf_expand, hkdf_extract, hmac, Hash, Sha256};
use huldra_crypto::sig::HashAlg;
use huldra_crypto::x25519;

pub type Result<T> = core::result::Result<T, String>;

const CHACHA20_POLY1305_SHA256: u16 = 0x1303;
const AES_128_GCM_SHA256: u16 = 0x1301;
const X25519: u16 = 0x001D;

const CT_CHANGE_CIPHER_SPEC: u8 = 20;
const CT_ALERT: u8 = 21;
const CT_HANDSHAKE: u8 = 22;
const CT_APPLICATION_DATA: u8 = 23;

const HS_CLIENT_HELLO: u8 = 1;
const HS_SERVER_HELLO: u8 = 2;
const HS_NEW_SESSION_TICKET: u8 = 4;
const HS_ENCRYPTED_EXTENSIONS: u8 = 8;
const HS_CERTIFICATE: u8 = 11;
const HS_CERTIFICATE_REQUEST: u8 = 13;
const HS_CERTIFICATE_VERIFY: u8 = 15;
const HS_FINISHED: u8 = 20;
const HS_KEY_UPDATE: u8 = 24;

/// The random value a ServerHello carries when it is a HelloRetryRequest.
const HRR_RANDOM: [u8; 32] = [
    0xCF, 0x21, 0xAD, 0x74, 0xE5, 0x9A, 0x61, 0x11, 0xBE, 0x1D, 0x8C, 0x02, 0x1E, 0x65, 0xB8, 0x91, 0xC2, 0xA2, 0x11, 0x16, 0x7A, 0xBB, 0x8C, 0x5E, 0x07, 0x9E, 0x09, 0xE2, 0xC8, 0xA8, 0x33, 0x9C,
];

fn alert_text(code: u8) -> &'static str {
    match code {
        0 => "close notify",
        10 => "unexpected message",
        20 => "bad record MAC",
        40 => "handshake failure (no common cipher or group)",
        42 => "bad certificate",
        45 => "certificate expired",
        47 => "illegal parameter",
        48 => "unknown CA",
        50 => "decode error",
        51 => "decrypt error",
        70 => "protocol version (TLS 1.3 not supported by the server)",
        80 => "internal error",
        109 => "missing extension",
        112 => "unrecognized name",
        120 => "no application protocol",
        _ => "alert",
    }
}

/// Encryption state for one direction.
struct Keys {
    aead: Aead,
    iv: [u8; 12],
    seq: u64,
    secret: Vec<u8>,
}

impl Keys {
    fn new(suite: u16, secret: &[u8]) -> Keys {
        let key_len = if suite == AES_128_GCM_SHA256 { 16 } else { 32 };
        let key = expand_label(secret, b"key", b"", key_len);
        let iv = expand_label(secret, b"iv", b"", 12);
        let aead = if suite == AES_128_GCM_SHA256 { Aead::aes128_gcm(&key) } else { Aead::chacha20_poly1305(&key) };
        Keys { aead, iv: iv.try_into().unwrap(), seq: 0, secret: secret.to_vec() }
    }

    fn nonce(&mut self) -> [u8; 12] {
        let mut n = self.iv;
        for (i, b) in self.seq.to_be_bytes().iter().enumerate() {
            n[4 + i] ^= b;
        }
        self.seq += 1;
        n
    }

    /// The next generation of traffic keys (KeyUpdate).
    fn update(&self, suite: u16) -> Keys {
        Keys::new(suite, &expand_label(&self.secret, b"traffic upd", b"", 32))
    }
}

/// HKDF-Expand-Label (section 7.1).
fn expand_label(secret: &[u8], label: &[u8], context: &[u8], len: usize) -> Vec<u8> {
    let mut info = Vec::with_capacity(10 + label.len() + context.len());
    info.extend_from_slice(&(len as u16).to_be_bytes());
    info.push(6 + label.len() as u8);
    info.extend_from_slice(b"tls13 ");
    info.extend_from_slice(label);
    info.push(context.len() as u8);
    info.extend_from_slice(context);
    hkdf_expand::<Sha256>(secret, &info, len)
}

fn derive_secret(secret: &[u8], label: &[u8], transcript: &Sha256) -> Vec<u8> {
    expand_label(secret, label, &transcript.clone().finish(), 32)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    ServerHello,
    EncryptedExtensions,
    Certificate,
    CertificateVerify,
    Finished,
    Connected,
    Closed,
}

/// How carefully to check the server.
pub struct Config<'a> {
    pub roots: &'a RootStore,
    /// Unix time for certificate validity.
    pub now: i64,
    /// Skip certificate checks (still encrypted, but anyone could be on the
    /// other end). For `--insecure`.
    pub insecure: bool,
    /// ALPN protocols to offer, e.g. `http/1.1`.
    pub alpn: &'a [&'a str],
}

pub struct Client<'a> {
    host: String,
    config: Config<'a>,
    state: State,
    private: [u8; 32],
    suite: u16,
    transcript: Sha256,
    handshake_secret: Vec<u8>,
    read: Option<Keys>,
    write: Option<Keys>,
    /// Application keys, installed after our Finished.
    pending_write: Option<Keys>,
    incoming: Vec<u8>,
    handshake_buf: Vec<u8>,
    outgoing: Vec<u8>,
    plaintext: Vec<u8>,
    certificates: Vec<Certificate>,
    /// Data written before the handshake finished.
    queued: Vec<u8>,
    /// Negotiated ALPN protocol.
    pub alpn: Option<String>,
}

impl<'a> Client<'a> {
    /// Starts a handshake with `host`. `random` must be 64 fresh random
    /// bytes (client random and key share).
    pub fn new(host: &str, config: Config<'a>, random: &[u8; 64]) -> Client<'a> {
        let mut private = [0u8; 32];
        private.copy_from_slice(&random[32..]);
        let mut c = Client {
            host: host.to_string(),
            config,
            state: State::ServerHello,
            private,
            suite: 0,
            transcript: Sha256::new(),
            handshake_secret: Vec::new(),
            read: None,
            write: None,
            pending_write: None,
            incoming: Vec::new(),
            handshake_buf: Vec::new(),
            outgoing: Vec::new(),
            plaintext: Vec::new(),
            certificates: Vec::new(),
            queued: Vec::new(),
            alpn: None,
        };
        let hello = c.client_hello(&random[..32]);
        c.transcript.update(&hello);
        c.send_record(CT_HANDSHAKE, &hello);
        c
    }

    fn client_hello(&self, random: &[u8]) -> Vec<u8> {
        let mut ext = Vec::new();
        let mut push_ext = |ty: u16, body: &[u8]| {
            ext.extend_from_slice(&ty.to_be_bytes());
            ext.extend_from_slice(&(body.len() as u16).to_be_bytes());
            ext.extend_from_slice(body);
        };
        // server_name, unless the host is an IP address.
        if !self.host.chars().all(|c| c.is_ascii_digit() || c == '.') {
            let name = self.host.as_bytes();
            let mut sni = Vec::new();
            sni.extend_from_slice(&((name.len() + 3) as u16).to_be_bytes());
            sni.push(0);
            sni.extend_from_slice(&(name.len() as u16).to_be_bytes());
            sni.extend_from_slice(name);
            push_ext(0, &sni);
        }
        push_ext(10, &[0, 2, 0x00, 0x1D]); // supported_groups: x25519
        let sigs: [u16; 8] = [0x0403, 0x0503, 0x0804, 0x0805, 0x0806, 0x0401, 0x0501, 0x0601];
        let mut s = vec![0, 16];
        for v in sigs {
            s.extend_from_slice(&v.to_be_bytes());
        }
        push_ext(13, &s); // signature_algorithms
        if !self.config.alpn.is_empty() {
            let mut list = Vec::new();
            for p in self.config.alpn {
                list.push(p.len() as u8);
                list.extend_from_slice(p.as_bytes());
            }
            let mut a = (list.len() as u16).to_be_bytes().to_vec();
            a.extend_from_slice(&list);
            push_ext(16, &a); // ALPN
        }
        push_ext(43, &[2, 0x03, 0x04]); // supported_versions: TLS 1.3
        push_ext(45, &[1, 1]); // psk_key_exchange_modes: psk_dhe_ke
        let public = x25519::public_key(&self.private);
        let mut ks = vec![0, 36, 0x00, 0x1D, 0, 32];
        ks.extend_from_slice(&public);
        push_ext(51, &ks); // key_share

        let mut body = vec![0x03, 0x03];
        body.extend_from_slice(random);
        body.push(32); // legacy_session_id: random-looking, for middlebox compatibility
        body.extend_from_slice(&huldra_crypto::sha::Sha256::digest(random)[..32]);
        body.extend_from_slice(&[0, 4]);
        body.extend_from_slice(&CHACHA20_POLY1305_SHA256.to_be_bytes());
        body.extend_from_slice(&AES_128_GCM_SHA256.to_be_bytes());
        body.extend_from_slice(&[1, 0]); // compression: null
        body.extend_from_slice(&(ext.len() as u16).to_be_bytes());
        body.extend_from_slice(&ext);
        handshake_message(HS_CLIENT_HELLO, &body)
    }

    pub fn is_connected(&self) -> bool {
        self.state == State::Connected
    }

    /// The peer sent close_notify (or the connection failed).
    pub fn is_closed(&self) -> bool {
        self.state == State::Closed
    }

    /// The server's certificate chain (after the handshake).
    pub fn certificates(&self) -> &[Certificate] {
        &self.certificates
    }

    pub fn cipher_suite(&self) -> &'static str {
        match self.suite {
            CHACHA20_POLY1305_SHA256 => "TLS_CHACHA20_POLY1305_SHA256",
            AES_128_GCM_SHA256 => "TLS_AES_128_GCM_SHA256",
            _ => "none",
        }
    }

    /// Bytes to send to the server.
    pub fn take_output(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.outgoing)
    }

    /// Decrypted application data received so far.
    pub fn read(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.plaintext)
    }

    /// Encrypts application data (queued until the handshake is done).
    pub fn write(&mut self, data: &[u8]) {
        if self.state != State::Connected {
            self.queued.extend_from_slice(data);
            return;
        }
        for chunk in data.chunks(16384) {
            self.send_record(CT_APPLICATION_DATA, chunk);
        }
    }

    /// Sends close_notify.
    pub fn close(&mut self) {
        if self.write.is_some() {
            self.send_record(CT_ALERT, &[1, 0]);
        }
    }

    fn send_record(&mut self, ty: u8, data: &[u8]) {
        match self.write.as_mut() {
            None => {
                self.outgoing.extend_from_slice(&[ty, 0x03, if ty == CT_HANDSHAKE && self.read.is_none() { 0x01 } else { 0x03 }]);
                self.outgoing.extend_from_slice(&(data.len() as u16).to_be_bytes());
                self.outgoing.extend_from_slice(data);
            }
            Some(keys) => {
                let mut inner = data.to_vec();
                inner.push(ty);
                let len = inner.len() + Aead::TAG;
                let header = [CT_APPLICATION_DATA, 0x03, 0x03, (len >> 8) as u8, len as u8];
                let nonce = keys.nonce();
                let sealed = keys.aead.seal(&nonce, &header, &inner);
                self.outgoing.extend_from_slice(&header);
                self.outgoing.extend_from_slice(&sealed);
            }
        }
    }

    /// Processes bytes from the server.
    pub fn feed(&mut self, data: &[u8]) -> Result<()> {
        self.incoming.extend_from_slice(data);
        let r = self.process();
        if r.is_err() {
            self.state = State::Closed;
        }
        r
    }

    fn process(&mut self) -> Result<()> {
        while self.incoming.len() >= 5 && self.state != State::Closed {
            let len = u16::from_be_bytes([self.incoming[3], self.incoming[4]]) as usize;
            if len > 16384 + 256 {
                return Err("record too long".into());
            }
            if self.incoming.len() < 5 + len {
                break;
            }
            let record: Vec<u8> = self.incoming.drain(..5 + len).collect();
            let (header, body) = record.split_at(5);
            let ty = header[0];
            if ty == CT_CHANGE_CIPHER_SPEC {
                continue; // compatibility mode: ignored
            }
            let (ty, content) = match (&mut self.read, ty) {
                (Some(keys), CT_APPLICATION_DATA) => {
                    let nonce = keys.nonce();
                    let mut inner = keys.aead.open(&nonce, header, body).map_err(|_| String::from("bad record MAC (decryption failed)"))?;
                    let end = inner.iter().rposition(|&b| b != 0).ok_or("empty inner record")?;
                    let ty = inner[end];
                    inner.truncate(end);
                    (ty, inner)
                }
                (Some(_), CT_ALERT) | (None, _) => (ty, body.to_vec()),
                (Some(_), other) => return Err(format!("unexpected plaintext record type {}", other)),
            };
            match ty {
                CT_ALERT => {
                    let code = *content.get(1).ok_or("short alert")?;
                    if code == 0 {
                        self.state = State::Closed;
                        return Ok(());
                    }
                    return Err(format!("server alert: {} ({})", alert_text(code), code));
                }
                CT_HANDSHAKE => {
                    self.handshake_buf.extend_from_slice(&content);
                    self.handshake_messages()?;
                }
                CT_APPLICATION_DATA if self.state == State::Connected => self.plaintext.extend_from_slice(&content),
                _ => return Err(format!("unexpected record type {} during the handshake", ty)),
            }
        }
        Ok(())
    }

    fn handshake_messages(&mut self) -> Result<()> {
        while self.handshake_buf.len() >= 4 {
            let len = (self.handshake_buf[1] as usize) << 16 | (self.handshake_buf[2] as usize) << 8 | self.handshake_buf[3] as usize;
            if self.handshake_buf.len() < 4 + len {
                return Ok(());
            }
            let msg: Vec<u8> = self.handshake_buf.drain(..4 + len).collect();
            self.handshake(&msg)?;
        }
        Ok(())
    }

    fn handshake(&mut self, msg: &[u8]) -> Result<()> {
        let ty = msg[0];
        let body = &msg[4..];
        match (self.state, ty) {
            (State::ServerHello, HS_SERVER_HELLO) => {
                self.server_hello(body)?;
                self.transcript.update(msg);
                self.handshake_keys()?;
                self.state = State::EncryptedExtensions;
            }
            (State::EncryptedExtensions, HS_ENCRYPTED_EXTENSIONS) => {
                self.encrypted_extensions(body)?;
                self.transcript.update(msg);
                self.state = State::Certificate;
            }
            (State::Certificate, HS_CERTIFICATE_REQUEST) => {
                return Err("the server asks for a client certificate, which is not supported".into());
            }
            (State::Certificate, HS_CERTIFICATE) => {
                self.certificate(body)?;
                self.transcript.update(msg);
                self.state = State::CertificateVerify;
            }
            (State::CertificateVerify, HS_CERTIFICATE_VERIFY) => {
                self.certificate_verify(body)?;
                self.transcript.update(msg);
                self.state = State::Finished;
            }
            (State::Finished, HS_FINISHED) => {
                self.server_finished(body)?;
                self.transcript.update(msg);
                self.finish_handshake();
            }
            (State::Connected, HS_NEW_SESSION_TICKET) => {}
            (State::Connected, HS_KEY_UPDATE) => {
                let suite = self.suite;
                let read = self.read.as_ref().ok_or("no keys")?.update(suite);
                self.read = Some(read);
                if body.first() == Some(&1) {
                    self.send_record(CT_HANDSHAKE, &handshake_message(HS_KEY_UPDATE, &[0]));
                    let write = self.write.as_ref().ok_or("no keys")?.update(suite);
                    self.write = Some(write);
                }
            }
            (state, ty) => return Err(format!("unexpected handshake message {} in state {:?}", ty, state)),
        }
        Ok(())
    }

    fn server_hello(&mut self, body: &[u8]) -> Result<()> {
        let mut r = Reader(body);
        r.take(2)?; // legacy_version
        if r.take(32)? == HRR_RANDOM {
            return Err("the server wants another key exchange group (only X25519 is supported)".into());
        }
        let sid = r.u8()? as usize;
        r.take(sid)?;
        self.suite = r.u16()?;
        if self.suite != CHACHA20_POLY1305_SHA256 && self.suite != AES_128_GCM_SHA256 {
            return Err(format!("server chose cipher suite {:#06x}", self.suite));
        }
        r.take(1)?; // compression
        let mut exts = Reader(r.vec16()?);
        let mut version = 0x0303;
        let mut share: Option<Vec<u8>> = None;
        while !exts.0.is_empty() {
            let ty = exts.u16()?;
            let data = exts.vec16()?;
            let mut d = Reader(data);
            match ty {
                43 => version = d.u16()?,
                51 => {
                    if d.u16()? != X25519 {
                        return Err("server chose a key exchange group other than X25519".into());
                    }
                    share = Some(d.vec16()?.to_vec());
                }
                _ => {}
            }
        }
        if version != 0x0304 {
            return Err("the server does not speak TLS 1.3".into());
        }
        let share = share.ok_or("server sent no key share")?;
        let peer: [u8; 32] = share.as_slice().try_into().map_err(|_| "bad X25519 key share")?;
        let shared = x25519::x25519(&self.private, &peer);
        if shared == [0u8; 32] {
            return Err("bad X25519 key share".into());
        }
        let zeros = [0u8; 32];
        let early = hkdf_extract::<Sha256>(&zeros, &zeros);
        let derived = derive_secret(&early, b"derived", &Sha256::new());
        self.handshake_secret = hkdf_extract::<Sha256>(&derived, &shared);
        self.private = [0; 32];
        Ok(())
    }

    fn handshake_keys(&mut self) -> Result<()> {
        let c = derive_secret(&self.handshake_secret, b"c hs traffic", &self.transcript);
        let s = derive_secret(&self.handshake_secret, b"s hs traffic", &self.transcript);
        self.read = Some(Keys::new(self.suite, &s));
        // Our handshake keys are needed only for Finished.
        self.pending_write = Some(Keys::new(self.suite, &c));
        Ok(())
    }

    fn encrypted_extensions(&mut self, body: &[u8]) -> Result<()> {
        let mut r = Reader(body);
        let mut exts = Reader(r.vec16()?);
        while !exts.0.is_empty() {
            let ty = exts.u16()?;
            let data = exts.vec16()?;
            if ty == 16 {
                let mut d = Reader(data);
                let mut list = Reader(d.vec16()?);
                let n = list.u8()? as usize;
                self.alpn = Some(String::from_utf8_lossy(list.take(n)?).into_owned());
            }
        }
        Ok(())
    }

    fn certificate(&mut self, body: &[u8]) -> Result<()> {
        let mut r = Reader(body);
        let ctx = r.u8()? as usize;
        r.take(ctx)?;
        let mut list = Reader(r.vec24()?);
        let mut chain = Vec::new();
        while !list.0.is_empty() {
            let der = list.vec24()?;
            list.vec16()?; // extensions
            chain.push(Certificate::parse(der).map_err(|e| format!("server certificate: {}", e))?);
        }
        if !self.config.insecure {
            self.config.roots.verify(&chain, &self.host, self.config.now)?;
        } else if chain.is_empty() {
            return Err("the server sent no certificate".into());
        }
        self.certificates = chain;
        Ok(())
    }

    fn certificate_verify(&mut self, body: &[u8]) -> Result<()> {
        let mut r = Reader(body);
        let scheme = r.u16()?;
        let sig = r.vec16()?;
        let mut content = vec![0x20u8; 64];
        content.extend_from_slice(b"TLS 1.3, server CertificateVerify\0");
        content.extend_from_slice(&self.transcript.clone().finish());
        let alg = match scheme {
            0x0403 => SigAlg::Ecdsa(HashAlg::Sha256),
            0x0503 => SigAlg::Ecdsa(HashAlg::Sha384),
            0x0603 => SigAlg::Ecdsa(HashAlg::Sha512),
            0x0804 => SigAlg::RsaPss(HashAlg::Sha256),
            0x0805 => SigAlg::RsaPss(HashAlg::Sha384),
            0x0806 => SigAlg::RsaPss(HashAlg::Sha512),
            other => return Err(format!("unsupported signature scheme {:#06x}", other)),
        };
        let leaf = self.certificates.first().ok_or("no certificate")?;
        if !leaf.key.verify(alg, &content, sig) {
            return Err("the server's handshake signature does not match its certificate".into());
        }
        Ok(())
    }

    fn server_finished(&mut self, body: &[u8]) -> Result<()> {
        let s_hs = &self.read.as_ref().ok_or("no keys")?.secret;
        let key = expand_label(s_hs, b"finished", b"", 32);
        let expect = hmac::<Sha256>(&key, &[&self.transcript.clone().finish()]);
        if !huldra_crypto::ct_eq(&expect, body) {
            return Err("server Finished does not match the handshake".into());
        }
        Ok(())
    }

    fn finish_handshake(&mut self) {
        // Application secrets from the transcript through server Finished.
        let zeros = [0u8; 32];
        let derived = derive_secret(&self.handshake_secret, b"derived", &Sha256::new());
        let master = hkdf_extract::<Sha256>(&derived, &zeros);
        let c_ap = derive_secret(&master, b"c ap traffic", &self.transcript);
        let s_ap = derive_secret(&master, b"s ap traffic", &self.transcript);
        // Client Finished with the client handshake key.
        let c_hs = self.pending_write.take().unwrap();
        let fkey = expand_label(&c_hs.secret, b"finished", b"", 32);
        let verify = hmac::<Sha256>(&fkey, &[&self.transcript.clone().finish()]);
        let fin = handshake_message(HS_FINISHED, &verify);
        self.outgoing.extend_from_slice(&[CT_CHANGE_CIPHER_SPEC, 3, 3, 0, 1, 1]);
        self.write = Some(c_hs);
        self.send_record(CT_HANDSHAKE, &fin);
        self.transcript.update(&fin);
        self.write = Some(Keys::new(self.suite, &c_ap));
        self.read = Some(Keys::new(self.suite, &s_ap));
        self.handshake_secret.clear();
        self.state = State::Connected;
        let queued = core::mem::take(&mut self.queued);
        if !queued.is_empty() {
            self.write(&queued);
        }
    }
}

fn handshake_message(ty: u8, body: &[u8]) -> Vec<u8> {
    let mut m = vec![ty, (body.len() >> 16) as u8, (body.len() >> 8) as u8, body.len() as u8];
    m.extend_from_slice(body);
    m
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.0.len() < n {
            return Err("handshake message truncated".into());
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn vec16(&mut self) -> Result<&'a [u8]> {
        let n = self.u16()? as usize;
        self.take(n)
    }
    fn vec24(&mut self) -> Result<&'a [u8]> {
        let b = self.take(3)?;
        let n = (b[0] as usize) << 16 | (b[1] as usize) << 8 | b[2] as usize;
        self.take(n)
    }
}
