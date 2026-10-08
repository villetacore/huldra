//! Building the initrd (cpio) from `rootfs/` plus the user-space binaries.

use crate::Result;
use std::fs;
use std::path::{Path, PathBuf};

/// A file to put into the image at `dest` (relative path, `/` separated).
pub struct ImageFile {
    pub dest: String,
    pub source: PathBuf,
    pub mode: u32,
}

/// Collects every file below `dir` (sorted, so the archive is reproducible).
pub fn collect_tree(dir: &Path) -> Result<Vec<ImageFile>> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<ImageFile>) -> Result {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .flatten()
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            let rel = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                walk(base, &path, out)?;
            } else if e.file_name() != ".keep" {
                let mode = if rel.starts_with("bin/") || rel.starts_with("sbin/") {
                    0o755
                } else {
                    0o644
                };
                out.push(ImageFile {
                    dest: rel,
                    source: path,
                    mode,
                });
            } else {
                // Keep empty directories: record the directory itself.
                let parent = Path::new(&rel)
                    .parent()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.push(ImageFile {
                    dest: format!("{parent}/"),
                    source: path,
                    mode: 0o755,
                });
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    if dir.is_dir() {
        walk(dir, dir, &mut out)?;
    }
    Ok(out)
}

pub fn write_initrd(files: &[ImageFile], out: &Path) -> Result {
    let mut w = huldra_cpio::Writer::new();
    let mut dirs = std::collections::BTreeSet::new();
    for f in files {
        let mut parts: Vec<&str> = f.dest.trim_end_matches('/').split('/').collect();
        if !f.dest.ends_with('/') {
            parts.pop();
        }
        let mut cur = String::new();
        for p in parts {
            if !cur.is_empty() {
                cur.push('/');
            }
            cur.push_str(p);
            if dirs.insert(cur.clone()) {
                w.add_dir(&cur, 0o755);
            }
        }
        if !f.dest.ends_with('/') {
            let data = fs::read(&f.source).map_err(|e| format!("{}: {e}", f.source.display()))?;
            w.add_file(&f.dest, f.mode, &data);
        }
    }
    fs::write(out, w.finish()).map_err(|e| format!("{}: {e}", out.display()))
}
