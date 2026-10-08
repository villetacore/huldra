//! Unpacks the initrd (a cpio newc archive loaded as a boot module) into
//! the root tmpfs, like Linux initramfs.

use crate::bootinfo;
use crate::mm::{frame, phys_to_virt};
use alloc::format;
use huldra_abi::fs::*;
use huldra_cpio::Reader;

/// Returns true if at least one archive was unpacked.
pub fn unpack_all() -> bool {
    let mut any = false;
    for m in bootinfo::get().modules.iter() {
        let data = unsafe { core::slice::from_raw_parts(phys_to_virt(m.start) as *const u8, (m.end - m.start) as usize) };
        let (mut files, mut bytes) = (0, 0);
        for entry in Reader::new(data) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    kerror!("initrd: corrupt archive ({:?})", e);
                    break;
                }
            };
            if entry.name.is_empty() || entry.name == "." {
                continue;
            }
            let path = format!("/{}", entry.name);
            if entry.is_dir() {
                let _ = super::mkdir_all(&path);
            } else if entry.is_file() {
                if let Some(parent) = path.rfind('/').map(|i| &path[..i]) {
                    let _ = super::mkdir_all(parent);
                }
                let result = super::open(&path, O_WRONLY | O_CREAT | O_TRUNC, entry.permissions())
                    .and_then(|f| f.write(entry.data));
                match result {
                    Ok(_) => {
                        files += 1;
                        bytes += entry.data.len();
                    }
                    Err(e) => kerror!("initrd: {}: {}", path, e),
                }
            }
        }
        kinfo!("initrd: unpacked {} files ({} KiB) from '{}'", files, bytes / 1024, m.cmdline.as_str());
        frame::release_range(m.start, m.end);
        any = true;
    }
    any
}
