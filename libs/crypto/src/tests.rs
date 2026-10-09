//! Checks every primitive against `testdata/vectors.txt`, produced by the
//! Python `cryptography` package (OpenSSL) with `testdata/generate.py`.

extern crate std;

use super::aead::Aead;
use super::sha::*;
use super::sig::*;
use super::*;
use alloc::vec::Vec;

const VECTORS: &str = include_str!("../testdata/vectors.txt");

fn un(s: &str) -> Vec<u8> {
    if s == "-" { Vec::new() } else { from_hex(s).unwrap() }
}

fn lines(kind: &str) -> impl Iterator<Item = Vec<&'static str>> + '_ {
    VECTORS.lines().map(|l| l.split(' ').collect::<Vec<_>>()).filter(move |f| f[0] == kind)
}

fn hash_alg(name: &str) -> HashAlg {
    match name {
        "sha1" => HashAlg::Sha1,
        "sha256" => HashAlg::Sha256,
        "sha384" => HashAlg::Sha384,
        _ => HashAlg::Sha512,
    }
}

#[test]
fn hashes() {
    let mut n = 0;
    for name in ["sha1", "sha256", "sha384", "sha512"] {
        for f in lines(name) {
            let msg = un(f[1]);
            assert_eq!(hex(&hash_alg(name).digest(&msg)), f[2], "{name} of {} bytes", msg.len());
            // Streaming in odd pieces gives the same digest.
            let mut h = Sha256::new();
            for c in msg.chunks(7) {
                h.update(c);
            }
            if name == "sha256" {
                assert_eq!(hex(&h.finish()), f[2]);
            }
            n += 1;
        }
    }
    assert_eq!(n, 40);
}

#[test]
fn macs_and_kdf() {
    for f in lines("hmac-sha256") {
        assert_eq!(hex(&hmac::<Sha256>(&un(f[1]), &[&un(f[2])])), f[3]);
    }
    for f in lines("hmac-sha384") {
        assert_eq!(hex(&hmac::<Sha384>(&un(f[1]), &[&un(f[2])])), f[3]);
    }
    for f in lines("hkdf-sha256") {
        let prk = hkdf_extract::<Sha256>(&un(f[1]), &un(f[2]));
        assert_eq!(hex(&hkdf_expand::<Sha256>(&prk, &un(f[3]), f[4].parse().unwrap())), f[5]);
    }
}

#[test]
fn aeads() {
    for (kind, make) in [("chacha20poly1305", Aead::chacha20_poly1305 as fn(&[u8]) -> Aead), ("aes128gcm", Aead::aes128_gcm)] {
        let mut n = 0;
        for f in lines(kind) {
            let aead = make(&un(f[1]));
            let nonce: [u8; 12] = un(f[2]).try_into().unwrap();
            let (aad, pt, sealed) = (un(f[3]), un(f[4]), un(f[5]));
            assert_eq!(hex(&aead.seal(&nonce, &aad, &pt)), f[5], "{kind} seal of {} bytes", pt.len());
            assert_eq!(aead.open(&nonce, &aad, &sealed).unwrap(), pt);
            let mut bad = sealed.clone();
            bad[0] ^= 1;
            assert!(aead.open(&nonce, &aad, &bad).is_err(), "{kind}: tampering not detected");
            n += 1;
        }
        assert_eq!(n, 10);
    }
}

#[test]
fn key_exchange() {
    for f in lines("x25519") {
        let private: [u8; 32] = un(f[1]).try_into().unwrap();
        let peer: [u8; 32] = un(f[3]).try_into().unwrap();
        assert_eq!(hex(&x25519::public_key(&private)), f[2]);
        assert_eq!(hex(&x25519::x25519(&private, &peer)), f[4]);
    }
}

#[test]
fn rsa_signatures() {
    let mut n = 0;
    for f in lines("rsa-pkcs1").chain(lines("rsa-pss")) {
        let key = RsaPublicKey { n: un(f[2]), e: un(f[3]) };
        let (alg, msg, sig) = (hash_alg(f[1]), un(f[4]), un(f[5]));
        let verify = |m: &[u8], s: &[u8]| if f[0] == "rsa-pss" { key.verify_pss(alg, m, s) } else { key.verify_pkcs1(alg, m, s) };
        assert!(verify(&msg, &sig), "{} {} with {} bits", f[0], f[1], key.n.len() * 8);
        let mut bad = sig.clone();
        bad[10] ^= 4;
        assert!(!verify(&msg, &bad));
        assert!(!verify(b"another message", &sig));
        n += 1;
    }
    assert_eq!(n, 21);
}

#[test]
fn ecdsa_signatures() {
    let mut n = 0;
    for f in lines("ecdsa") {
        let curve = if f[1] == "p256" { Curve::P256 } else { Curve::P384 };
        let digest = hash_alg(f[2]).digest(&un(f[4]));
        let (point, r, s) = (un(f[3]), un(f[5]), un(f[6]));
        assert!(ecdsa_verify(curve, &point, &digest, &r, &s), "{} {}", f[1], f[2]);
        let mut bad = s.clone();
        bad[5] ^= 1;
        assert!(!ecdsa_verify(curve, &point, &digest, &r, &bad));
        assert!(!ecdsa_verify(curve, &point, &hash_alg(f[2]).digest(b"x"), &r, &s));
        n += 1;
    }
    assert_eq!(n, 12);
}

#[test]
fn encodings() {
    assert_eq!(base64_encode(b"Many hands make light work."), "TWFueSBoYW5kcyBtYWtlIGxpZ2h0IHdvcmsu");
    for data in [&b""[..], b"f", b"fo", b"foo", b"foob"] {
        assert_eq!(base64_decode(&base64_encode(data)).unwrap(), data);
    }
    assert_eq!(base64_decode("Zm9v\nYmFy").unwrap(), b"foobar");
    assert!(base64_decode("@@").is_none());
    assert_eq!(from_hex("00ff"), Some(alloc::vec![0, 255]));
    assert!(ct_eq(b"ab", b"ab") && !ct_eq(b"ab", b"ac") && !ct_eq(b"a", b"ab"));
}
