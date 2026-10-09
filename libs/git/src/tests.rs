//! Against data made by real git (`testdata/generate.py`).

extern crate std;

use super::*;
use alloc::string::String;
use alloc::vec::Vec;
use object::{Commit, Kind};

const OBJECTS: &str = include_str!("../testdata/objects.txt");
const HEAD: &str = include_str!("../testdata/HEAD.txt");

fn expected() -> Vec<(Id, &'static str, usize)> {
    OBJECTS
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (Id::from_hex(f[0]).unwrap(), f[1], f[2].parse().unwrap())
        })
        .collect()
}

fn check_objects(objs: &[pack::Object]) {
    let want = expected();
    assert_eq!(objs.len(), want.len());
    for (id, kind, size) in want {
        let o = objs.iter().find(|o| o.id == id).unwrap_or_else(|| panic!("{id} missing"));
        assert_eq!((o.kind.name(), o.data.len()), (kind, size), "{id}");
        assert_eq!(object::hash(o.kind, &o.data), id);
    }
}

#[test]
fn packs_with_ofs_and_ref_deltas() {
    let pack = include_bytes!("../testdata/repo.pack");
    let objs = pack::parse(pack, &|_| None).unwrap();
    check_objects(&objs);
    let objs2 = pack::parse(include_bytes!("../testdata/refdelta.pack"), &|_| None).unwrap();
    check_objects(&objs2);
    // Our index is byte for byte the one git wrote.
    let idx = pack::write_index(&objs, &pack[pack.len() - 20..]);
    assert_eq!(idx, include_bytes!("../testdata/repo.idx"));
    // Random access through the index.
    let index = pack::Index::parse(idx).unwrap();
    for (id, kind, size) in expected() {
        let off = index.find(&id).unwrap();
        let (k, data) = pack::read_at(pack, off, &|_| None).unwrap();
        assert_eq!((k.name(), data.len()), (kind, size));
    }
    assert!(index.find(&Id([7; 20])).is_none());
    let mut bad = pack.to_vec();
    bad[100] ^= 1;
    assert!(pack::parse(&bad, &|_| None).is_err());
}

#[test]
fn building_packs() {
    let objs = pack::parse(include_bytes!("../testdata/repo.pack"), &|_| None).unwrap();
    let whole: Vec<(Kind, Vec<u8>)> = objs.iter().map(|o| (o.kind, o.data.clone())).collect();
    let built = pack::build(&whole);
    check_objects(&pack::parse(&built, &|_| None).unwrap());
}

#[test]
fn commits_and_trees() {
    let mut lines = HEAD.lines();
    let head = Id::from_hex(lines.next().unwrap()).unwrap();
    let tree = Id::from_hex(lines.next().unwrap()).unwrap();
    let text: String = lines.map(|l| alloc::format!("{}\n", l)).collect();
    let objs = pack::parse(include_bytes!("../testdata/repo.pack"), &|_| None).unwrap();
    let raw = &objs.iter().find(|o| o.id == head).unwrap().data;
    assert_eq!(String::from_utf8_lossy(raw), text);
    let c = Commit::parse(raw).unwrap();
    assert_eq!(c.tree, tree);
    assert_eq!(c.summary(), "commit 5");
    assert_eq!(c.author.tz, 60);
    assert_eq!(&c.to_bytes(), raw);
    assert_eq!(object::hash(Kind::Commit, &c.to_bytes()), head);
    let t = &objs.iter().find(|o| o.id == tree).unwrap().data;
    let entries = object::parse_tree(t).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["big.txt", "data.bin", "src"]);
    assert!(entries[2].is_tree());
    assert_eq!(&object::write_tree(&entries), t);
    // Loose objects round trip.
    let loose = object::encode_loose(Kind::Commit, raw);
    assert_eq!(object::decode_loose(&loose).unwrap(), (Kind::Commit, raw.clone()));
}

#[test]
fn tree_order() {
    let e = |name: &str, tree: bool| object::TreeEntry { mode: if tree { object::MODE_TREE } else { object::MODE_FILE }, name: name.into(), id: Id::ZERO };
    // "a.c" sorts before the directory "a" (compared as "a/") but after "a-".
    let data = object::write_tree(&[e("a", true), e("a.c", false), e("a-", false)]);
    let names: Vec<String> = object::parse_tree(&data).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["a-", "a.c", "a"]);
}

#[test]
fn index_files() {
    let data = include_bytes!("../testdata/index.bin");
    let entries = index::parse(data).unwrap();
    let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(&paths[..3], ["big.txt", "data.bin", "src/deep/f0.c"]);
    assert_eq!(entries.len(), 8);
    let expected_ids: Vec<Id> = expected().into_iter().map(|e| e.0).collect();
    assert!(entries.iter().all(|e| expected_ids.contains(&e.id)));
    // Without extensions our output equals git's, up to the extension block.
    let ours = index::write(&entries);
    assert_eq!(index::parse(&ours).unwrap(), entries);
    let body = ours.len() - 20;
    assert_eq!(&ours[..body], &data[..body]);
}

#[test]
fn diffs_match_git() {
    let a = include_str!("../testdata/diff-a.txt");
    let b = include_str!("../testdata/diff-b.txt");
    let ours = diff::unified("a", "b", a, b, 3);
    let theirs = include_str!("../testdata/diff.txt");
    let strip = |s: &str| -> String {
        s.lines()
            .filter(|l| !l.starts_with("---") && !l.starts_with("+++"))
            .map(|l| if l.starts_with("@@") { alloc::format!("{}@@\n", &l[..l.rfind("@@").unwrap()]) } else { alloc::format!("{}\n", l) })
            .collect()
    };
    assert_eq!(strip(&ours), strip(theirs));
    assert_eq!(diff::unified("a", "b", "same\n", "same\n", 3), "");
    assert_eq!(diff::unified("a", "b", "", "new\n", 3), "--- a\n+++ b\n@@ -0,0 +1 @@\n+new\n");
}

#[test]
fn protocol_lines() {
    assert_eq!(protocol::pkt(b"hello\n"), b"000ahello\n");
    let adv_body = [
        protocol::pkt(b"# service=git-upload-pack\n"),
        protocol::FLUSH.to_vec(),
        protocol::pkt(b"c8e830abbdc4057ede1ac8e0be423d4e723caa84 HEAD\0multi_ack side-band-64k ofs-delta symref=HEAD:refs/heads/main shallow\n"),
        protocol::pkt(b"c8e830abbdc4057ede1ac8e0be423d4e723caa84 refs/heads/main\n"),
        protocol::FLUSH.to_vec(),
    ]
    .concat();
    let adv = protocol::Advertisement::parse(&adv_body).unwrap();
    assert_eq!(adv.head_target(), Some("refs/heads/main"));
    assert!(adv.has("side-band-64k") && adv.has("shallow") && !adv.has("thin-pack"));
    assert_eq!(adv.refs.len(), 2);
    let want = adv.get("refs/heads/main").unwrap();
    let req = protocol::upload_request(&[want], &[], Some(1), &adv);
    let text = String::from_utf8_lossy(&req);
    assert!(text.starts_with("005bwant c8e830abbdc4057ede1ac8e0be423d4e723caa84 side-band-64k ofs-delta agent=huldra-git\n"), "{text}");
    assert!(text.contains("deepen 1\n00000009done\n"));
    // A side-band response with progress and the pack split in two.
    let pack = include_bytes!("../testdata/repo.pack");
    let mut sb = |ch: u8, d: &[u8]| -> Vec<u8> { protocol::pkt(&[&[ch][..], d].concat()) };
    let resp = [protocol::pkt(b"shallow c8e830abbdc4057ede1ac8e0be423d4e723caa84\n"), protocol::FLUSH.to_vec(), protocol::pkt(b"NAK\n"), sb(2, b"Counting objects\n"), sb(1, &pack[..1000]), sb(1, &pack[1000..]), protocol::FLUSH.to_vec()].concat();
    let r = protocol::parse_upload_response(&resp, true).unwrap();
    assert_eq!(r.pack, pack);
    assert_eq!(r.shallow.len(), 1);
    assert!(r.progress.contains("Counting"));
    assert!(protocol::parse_upload_response(&[protocol::pkt(b"NAK\n"), sb(3, b"boom")].concat(), true).unwrap_err().0.contains("boom"));
    // Push report.
    let ok = [protocol::pkt(b"unpack ok\n"), protocol::pkt(b"ok refs/heads/main\n"), protocol::FLUSH.to_vec()].concat();
    assert!(protocol::parse_push_response(&ok, false).is_ok());
    let ng = [protocol::pkt(b"unpack ok\n"), protocol::pkt(b"ng refs/heads/main non-fast-forward\n")].concat();
    assert!(protocol::parse_push_response(&ng, false).unwrap_err().0.contains("non-fast-forward"));
}
