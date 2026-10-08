//! Just enough ACPI to find interrupt controllers: walks the RSDT/XSDT to
//! the MADT ("APIC" table).

use crate::mm::phys_to_virt;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug)]
pub struct IoApic {
    pub id: u8,
    pub address: u64,
    pub gsi_base: u32,
}

/// ISA IRQ remapping from the MADT.
#[derive(Clone, Copy, Debug)]
pub struct Override {
    pub irq: u8,
    pub gsi: u32,
    pub active_low: bool,
    pub level_triggered: bool,
}

#[derive(Debug, Default)]
pub struct Madt {
    pub local_apic: u64,
    pub cpus: usize,
    pub io_apics: Vec<IoApic>,
    pub overrides: Vec<Override>,
}

unsafe fn read<T: Copy>(phys: u64) -> T {
    (phys_to_virt(phys) as *const T).read_unaligned()
}

fn checksum_ok(phys: u64, len: u32) -> bool {
    let bytes = unsafe { core::slice::from_raw_parts(phys_to_virt(phys) as *const u8, len as usize) };
    bytes.iter().fold(0u8, |a, &b| a.wrapping_add(b)) == 0
}

/// Finds the table with `signature` among the root table's entries.
fn find_table(root: u64, xsdt: bool, signature: &[u8; 4]) -> Option<u64> {
    unsafe {
        let len = read::<u32>(root + 4);
        if !checksum_ok(root, len) {
            return None;
        }
        let entry_size = if xsdt { 8 } else { 4 };
        let count = (len as u64 - 36) / entry_size;
        (0..count)
            .map(|i| {
                let at = root + 36 + i * entry_size;
                if xsdt {
                    read::<u64>(at)
                } else {
                    read::<u32>(at) as u64
                }
            })
            .find(|&t| read::<[u8; 4]>(t) == *signature)
    }
}

/// Looks for the RSDP in the EBDA and the BIOS area (when the boot loader
/// did not pass it) and returns the root table address.
fn scan_for_root() -> Option<(u64, bool)> {
    let ebda = unsafe { read::<u16>(0x40E) } as u64 * 16;
    let ranges = [(ebda, ebda + 1024), (0xE0000, 0x100000)];
    for (start, end) in ranges {
        if start == 0 {
            continue;
        }
        let mut p = start & !15;
        while p + 36 <= end {
            if unsafe { read::<[u8; 8]>(p) } == *b"RSD PTR " && checksum_ok(p, 20) {
                let revision = unsafe { read::<u8>(p + 15) };
                let xsdt = unsafe { read::<u64>(p + 24) };
                return Some(if revision >= 2 && xsdt != 0 { (xsdt, true) } else { (unsafe { read::<u32>(p + 16) } as u64, false) });
            }
            p += 16;
        }
    }
    None
}

pub fn parse_madt() -> Option<Madt> {
    let (root, xsdt) = crate::bootinfo::get().acpi_root.or_else(scan_for_root)?;
    let madt = find_table(root, xsdt, b"APIC")?;
    unsafe {
        let len = read::<u32>(madt + 4);
        if !checksum_ok(madt, len) {
            return None;
        }
        let mut info = Madt { local_apic: read::<u32>(madt + 36) as u64, ..Madt::default() };
        let mut p = madt + 44;
        while p + 2 <= madt + len as u64 {
            let (kind, entry_len) = (read::<u8>(p), read::<u8>(p + 1) as u64);
            if entry_len < 2 {
                break;
            }
            match kind {
                0 if read::<u32>(p + 4) & 1 != 0 => info.cpus += 1,
                1 => info.io_apics.push(IoApic {
                    id: read(p + 2),
                    address: read::<u32>(p + 4) as u64,
                    gsi_base: read(p + 8),
                }),
                2 => {
                    let flags = read::<u16>(p + 8);
                    info.overrides.push(Override {
                        irq: read(p + 3),
                        gsi: read(p + 4),
                        active_low: flags & 3 == 3,
                        level_triggered: (flags >> 2) & 3 == 3,
                    });
                }
                5 => info.local_apic = read(p + 4),
                _ => {}
            }
            p += entry_len;
        }
        Some(info)
    }
}
