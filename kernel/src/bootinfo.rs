//! Boot protocol parsing: Multiboot2 (GRUB) and PVH (QEMU -kernel).
//!
//! Everything is copied into owned data, so loader memory may be reused.

use crate::sync::SpinLock;
use alloc::string::String;
use alloc::vec::Vec;

const MULTIBOOT2_MAGIC: u32 = 0x36D7_6289;
const PVH_MAGIC: u32 = 0x336E_C578;

#[derive(Clone, Copy)]
pub struct MemRegion {
    pub base: u64,
    pub len: u64,
    /// E820 type: 1 usable, 2 reserved, 3 ACPI reclaimable, 4 ACPI NVS, 5 bad.
    pub kind: u32,
}

impl MemRegion {
    pub fn is_usable(&self) -> bool {
        self.kind == 1
    }

    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            1 => "usable",
            2 => "reserved",
            3 => "ACPI reclaimable",
            4 => "ACPI NVS",
            5 => "bad memory",
            _ => "unknown",
        }
    }
}

pub struct BootInfo {
    pub protocol: &'static str,
    pub bootloader: Option<String>,
    pub cmdline: Option<String>,
    pub memory: Vec<MemRegion>,
}

static BOOT_INFO: SpinLock<Option<BootInfo>> = SpinLock::new(None);

pub fn store(info: BootInfo) {
    *BOOT_INFO.lock() = Some(info);
}

pub fn with<R>(f: impl FnOnce(&BootInfo) -> R) -> Option<R> {
    BOOT_INFO.lock().as_ref().map(f)
}

unsafe fn read<T: Copy>(addr: usize) -> T {
    (addr as *const T).read_unaligned()
}

unsafe fn c_string(addr: usize) -> Option<String> {
    if addr == 0 {
        return None;
    }
    let mut len = 0;
    while len < 4096 && read::<u8>(addr + len) != 0 {
        len += 1;
    }
    let bytes = core::slice::from_raw_parts(addr as *const u8, len);
    let s = String::from_utf8_lossy(bytes).into_owned();
    (!s.is_empty()).then_some(s)
}

/// # Safety
/// `info` must be the pointer handed over by the boot loader.
pub unsafe fn parse(magic: u32, info: usize) -> BootInfo {
    if magic == MULTIBOOT2_MAGIC {
        parse_multiboot2(info)
    } else if info != 0 && read::<u32>(info) == PVH_MAGIC {
        parse_pvh(info)
    } else {
        BootInfo { protocol: "unknown", bootloader: None, cmdline: None, memory: Vec::new() }
    }
}

unsafe fn parse_multiboot2(info: usize) -> BootInfo {
    let mut boot = BootInfo {
        protocol: "Multiboot2",
        bootloader: None,
        cmdline: None,
        memory: Vec::new(),
    };
    let end = info + read::<u32>(info) as usize;
    let mut tag = info + 8;
    while tag + 8 <= end {
        let kind = read::<u32>(tag);
        let size = read::<u32>(tag + 4) as usize;
        if kind == 0 || size < 8 {
            break;
        }
        match kind {
            1 => boot.cmdline = c_string(tag + 8),
            2 => boot.bootloader = c_string(tag + 8),
            6 => {
                let entry_size = read::<u32>(tag + 8) as usize;
                let mut entry = tag + 16;
                while entry_size >= 24 && entry + entry_size <= tag + size {
                    boot.memory.push(MemRegion {
                        base: read(entry),
                        len: read(entry + 8),
                        kind: read(entry + 16),
                    });
                    entry += entry_size;
                }
            }
            _ => {}
        }
        tag += (size + 7) & !7;
    }
    boot
}

unsafe fn parse_pvh(info: usize) -> BootInfo {
    // struct hvm_start_info, see xen/include/public/arch-x86/hvm/start_info.h
    let version = read::<u32>(info + 4);
    let mut boot = BootInfo {
        protocol: "PVH",
        bootloader: None,
        cmdline: c_string(read::<u64>(info + 24) as usize),
        memory: Vec::new(),
    };
    if version >= 1 {
        let map = read::<u64>(info + 40) as usize;
        let entries = read::<u32>(info + 48) as usize;
        for i in 0..entries {
            let e = map + i * 24;
            boot.memory.push(MemRegion { base: read(e), len: read(e + 8), kind: read(e + 16) });
        }
    }
    boot
}
