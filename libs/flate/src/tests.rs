extern crate std;

use super::*;

const TEXT: &[u8] = include_bytes!("../testdata/text.txt");
const BINARY: &[u8] = include_bytes!("../testdata/binary.bin");

#[test]
fn zlib_streams_from_python() {
    for (name, data) in [
        ("stored", &include_bytes!("../testdata/text.z0")[..]),
        ("level 1", &include_bytes!("../testdata/text.z1")[..]),
        ("level 6", &include_bytes!("../testdata/text.z6")[..]),
        ("level 9", &include_bytes!("../testdata/text.z9")[..]),
    ] {
        let (out, used) = zlib_decompress(data).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(out, TEXT, "{name}");
        assert_eq!(used, data.len(), "{name}");
    }
    let (out, _) = zlib_decompress(include_bytes!("../testdata/binary.z6")).unwrap();
    assert_eq!(out, BINARY);
}

#[test]
fn fixed_huffman_and_trailing_data() {
    let raw = include_bytes!("../testdata/text.fixed.raw");
    let mut with_tail = raw.to_vec();
    with_tail.extend_from_slice(b"NEXT OBJECT");
    let (out, used) = inflate(&with_tail).unwrap();
    assert_eq!(out, TEXT);
    assert_eq!(used, raw.len());
}

#[test]
fn gzip_members() {
    let out = gzip_decompress(include_bytes!("../testdata/two.gz")).unwrap();
    assert_eq!(out, &TEXT[..3000]);
}

#[test]
fn errors() {
    let z = include_bytes!("../testdata/text.z6");
    assert_eq!(zlib_decompress(&z[..z.len() / 2]).unwrap_err(), Error::Truncated);
    let mut bad = z.to_vec();
    let n = bad.len();
    bad[n - 1] ^= 1;
    assert_eq!(zlib_decompress(&bad).unwrap_err(), Error::Checksum);
    assert!(zlib_decompress(b"hello").is_err());
    assert!(inflate(&[0xFF; 16]).is_err());
}

#[test]
fn compression_round_trips() {
    for data in [&TEXT[..], BINARY, b"", b"a", b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", &[0u8; 100_000][..]] {
        let z = zlib_compress(data);
        let (out, used) = zlib_decompress(&z).unwrap();
        assert_eq!(out, data);
        assert_eq!(used, z.len());
    }
    // Text compresses well, random data does not grow much.
    assert!(zlib_compress(TEXT).len() < TEXT.len() / 4);
    assert!(zlib_compress(BINARY).len() < BINARY.len() * 9 / 8 + 16);
}

#[test]
fn checksums() {
    assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}
