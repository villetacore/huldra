//! Certificate checks with the chain in `testdata/` (made by
//! `testdata/generate.py`). The TLS handshake itself is tested against a
//! real server in QEMU (`tests/net.txt`) and with `examples/fetch.rs`.

extern crate std;

use super::x509::*;
use alloc::vec;

const NOW: i64 = 1_800_000_000; // 2027

fn load(name: &str) -> Certificate {
    let der = match name {
        "root" => &include_bytes!("../testdata/root.der")[..],
        "inter" => include_bytes!("../testdata/inter.der"),
        "leaf" => include_bytes!("../testdata/leaf.der"),
        "expired" => include_bytes!("../testdata/expired.der"),
        "other" => include_bytes!("../testdata/other.der"),
        _ => include_bytes!("../testdata/p384.der"),
    };
    Certificate::parse(der).unwrap()
}

fn roots() -> RootStore {
    RootStore::from_pem(include_str!("../testdata/root.pem"))
}

#[test]
fn parsing() {
    let leaf = load("leaf");
    assert_eq!(leaf.subject_name(), "test.huldra");
    assert_eq!(leaf.issuer_name(), "Huldra Test Intermediate");
    assert_eq!(leaf.dns_names, ["test.huldra", "*.wild.huldra"]);
    assert_eq!(leaf.ip_addresses, [vec![10, 0, 2, 2]]);
    assert!(!leaf.is_ca && load("inter").is_ca);
    assert!(matches!(leaf.key, PublicKey::Rsa { .. }));
    assert!(matches!(load("root").key, PublicKey::Ec { .. }));
    assert_eq!(roots().len(), 1);
}

#[test]
fn host_names() {
    let leaf = load("leaf");
    assert!(leaf.matches_host("test.huldra"));
    assert!(leaf.matches_host("TEST.huldra."));
    assert!(leaf.matches_host("a.wild.huldra"));
    assert!(!leaf.matches_host("a.b.wild.huldra"));
    assert!(!leaf.matches_host("wild.huldra"));
    assert!(!leaf.matches_host("evil.huldra"));
    assert!(leaf.matches_host("10.0.2.2"));
    assert!(!leaf.matches_host("10.0.2.3"));
}

#[test]
fn chains() {
    let r = roots();
    let (leaf, inter) = (load("leaf"), load("inter"));
    // RSA leaf <- RSA intermediate <- ECDSA root.
    r.verify(&[leaf.clone(), inter.clone()], "test.huldra", NOW).unwrap();
    r.verify(&[load("p384")], "p384.huldra", NOW).unwrap();
    let e = r.verify(&[leaf.clone()], "test.huldra", NOW).unwrap_err();
    assert!(e.contains("not trusted"), "{e}");
    let e = r.verify(&[leaf.clone(), inter.clone()], "other.huldra", NOW).unwrap_err();
    assert!(e.contains("not other.huldra"), "{e}");
    let e = r.verify(&[load("expired"), inter.clone()], "test.huldra", NOW).unwrap_err();
    assert!(e.contains("expired"), "{e}");
    let e = r.verify(&[leaf.clone(), inter.clone()], "test.huldra", 1_600_000_000).unwrap_err();
    assert!(e.contains("not valid yet"), "{e}");
    let e = r.verify(&[load("other"), inter.clone()], "test.huldra", NOW).unwrap_err();
    assert!(e.contains("Unknown CA"), "{e}");
    // Extra certificates that are not CAs are passed over.
    let e = r.verify(&[leaf.clone(), leaf.clone(), inter.clone()], "test.huldra", NOW);
    assert!(e.is_ok());
    assert!(RootStore::default().verify(&[leaf, inter], "test.huldra", NOW).is_err());
}

#[test]
fn tampering() {
    let mut der = include_bytes!("../testdata/leaf.der").to_vec();
    // Flip a byte inside the subject public key: the signature breaks.
    let pos = der.len() / 2;
    der[pos] ^= 1;
    if let Ok(bad) = Certificate::parse(&der) {
        assert!(roots().verify(&[bad, load("inter")], "test.huldra", NOW).is_err());
    }
}

#[test]
fn time_and_oids() {
    assert_eq!(oid_string(&[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b]), "1.2.840.113549.1.1.11");
    assert_eq!(oid_string(&[0x2b, 0x81, 0x04, 0x00, 0x22]), "1.3.132.0.34");
}
