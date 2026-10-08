//! Boot protocol parsing: Multiboot2 (GRUB) and PVH (QEMU -kernel).
//!
//! Runs before the heap exists, so everything is copied into fixed-size
//! storage; loader memory may be reused afterwards (except modules, which
//! the frame allocator keeps reserved until they are released).

use crate::mm::phys_to_virt;
use crate::sync::Once;
use crate::util::{ArrayString, ArrayVec};

const MULTIBOOT2_MAGIC: u32 = 0x36D7_6289;
const PVH_MAGIC: u32 = 0x336E_C578;

#[derive(Clone, Copy, Default)]
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

/// A file loaded by the boot loader next to the kernel (physical range).
#[derive(Clone, Copy, Default)]
pub struct Module {
    pub start: u64,
    pub end: u64,
    pub cmdline: ArrayString<64>,
}

#[derive(Clone, Copy)]
pub struct BootInfo {
    pub protocol: &'static str,
    pub bootloader: ArrayString<64>,
    pub cmdline: ArrayString<256>,
    pub memory: ArrayVec<MemRegion, 64>,
    pub modules: ArrayVec<Module, 8>,
    /// ACPI root table (physical address, true if it is an XSDT).
    pub acpi_root: Option<(u64, bool)>,
}

impl BootInfo {
    fn empty(protocol: &'static str) -> Self {
        BootInfo {
            protocol,
            bootloader: ArrayString::new(),
            cmdline: ArrayString::new(),
            memory: ArrayVec::new(),
            modules: ArrayVec::new(),
            acpi_root: None,
        }
    }

    /// True if the kernel command line contains `word`.
    pub fn has_flag(&self, word: &str) -> bool {
        self.cmdline.split_whitespace().any(|w| w == word)
    }

    /// Value of `key=value` on the command line.
    pub fn option(&self, key: &str) -> Option<&str> {
        self.cmdline.split_whitespace().find_map(|w| w.strip_prefix(key)?.strip_prefix('='))
    }
}

static BOOT_INFO: Once<BootInfo> = Once::new();

pub fn store(info: BootInfo) -> &'static BootInfo {
    BOOT_INFO.call_once(|| info)
}

pub fn get() -> &'static BootInfo {
    BOOT_INFO.expect("boot info")
}

unsafe fn read<T: Copy>(phys: u64) -> T {
    (phys_to_virt(phys) as *const T).read_unaligned()
}

unsafe fn c_string<const N: usize>(phys: u64) -> ArrayString<N> {
    if phys == 0 {
        return ArrayString::new();
    }
    let mut len = 0;
    while len < N && read::<u8>(phys + len as u64) != 0 {
        len += 1;
    }
    ArrayString::from_bytes(core::slice::from_raw_parts(phys_to_virt(phys) as *const u8, len))
}

/// Extracts the RSDT/XSDT address from an RSDP structure.
unsafe fn acpi_root(rsdp: u64) -> Option<(u64, bool)> {
    if read::<[u8; 8]>(rsdp) != *b"RSD PTR " {
        return None;
    }
    let revision = read::<u8>(rsdp + 15);
    let xsdt = read::<u64>(rsdp + 24);
    if revision >= 2 && xsdt != 0 {
        Some((xsdt, true))
    } else {
        Some((read::<u32>(rsdp + 16) as u64, false))
    }
}

/// # Safety
/// `info` must be the physical pointer handed over by the boot loader.
pub unsafe fn parse(magic: u32, info: u64) -> BootInfo {
    if magic == MULTIBOOT2_MAGIC {
        parse_multiboot2(info)
    } else if info != 0 && read::<u32>(info) == PVH_MAGIC {
        parse_pvh(info)
    } else {
        BootInfo::empty("unknown")
    }
}

unsafe fn parse_multiboot2(info: u64) -> BootInfo {
    let mut boot = BootInfo::empty("Multiboot2");
    let end = info + read::<u32>(info) as u64;
    let mut tag = info + 8;
    while tag + 8 <= end {
        let kind = read::<u32>(tag);
        let size = read::<u32>(tag + 4) as u64;
        if kind == 0 || size < 8 {
            break;
        }
        match kind {
            1 => boot.cmdline = c_string(tag + 8),
            2 => boot.bootloader = c_string(tag + 8),
            3 => {
                boot.modules.push(Module {
                    start: read::<u32>(tag + 8) as u64,
                    end: read::<u32>(tag + 12) as u64,
                    cmdline: c_string(tag + 16),
                });
            }
            6 => {
                let entry_size = read::<u32>(tag + 8) as u64;
                let mut entry = tag + 16;
                while entry_size >= 24 && entry + entry_size <= tag + size {
                    boot.memory.push(MemRegion { base: read(entry), len: read(entry + 8), kind: read(entry + 16) });
                    entry += entry_size;
                }
            }
            // ACPI old/new RSDP: the tag holds a copy of the structure.
            14 | 15 if boot.acpi_root.is_none() || kind == 15 => boot.acpi_root = acpi_root(tag + 8),
            _ => {}
        }
        tag += (size + 7) & !7;
    }
    boot
}

unsafe fn parse_pvh(info: u64) -> BootInfo {
    // struct hvm_start_info, see xen/include/public/arch-x86/hvm/start_info.h
    let mut boot = BootInfo::empty("PVH");
    let version = read::<u32>(info + 4);
    let nr_modules = read::<u32>(info + 12) as u64;
    let modlist = read::<u64>(info + 16);
    boot.cmdline = c_string(read::<u64>(info + 24));
    let rsdp = read::<u64>(info + 32);
    if rsdp != 0 {
        boot.acpi_root = acpi_root(rsdp);
    }
    for i in 0..nr_modules {
        let m = modlist + i * 32;
        let start = read::<u64>(m);
        boot.modules.push(Module { start, end: start + read::<u64>(m + 8), cmdline: c_string(read::<u64>(m + 16)) });
    }
    if version >= 1 {
        let map = read::<u64>(info + 40);
        let entries = read::<u32>(info + 48) as u64;
        for i in 0..entries {
            let e = map + i * 24;
            boot.memory.push(MemRegion { base: read(e), len: read(e + 8), kind: read(e + 16) });
        }
    }
    boot
}
