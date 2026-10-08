//! Kernel address space.
//!
//! Builds the kernel's own page tables: a direct map of physical memory and
//! the kernel image with per-section permissions (W^X). User address spaces
//! (see `crate::proc::mm`) share the upper half by copying its PML4 entries.

use super::*;
use crate::arch::cpu;
use crate::arch::paging::{PageTable, PteFlags, ENTRIES, KERNEL_HALF_START};
use crate::bootinfo::BootInfo;
use crate::sync::Once;

const HUGE_PAGE: u64 = 2 << 20;
const LOW_DIRECT_MAP: u64 = 4 << 30;

static KERNEL_PML4: Once<u64> = Once::new();

pub fn kernel_page_table() -> PageTable {
    unsafe { PageTable::from_root(*KERNEL_PML4.expect("kernel page table")) }
}

fn pml4_index(virt: u64) -> usize {
    ((virt >> 39) & 0x1FF) as usize
}

pub fn init(boot: &BootInfo) {
    let mut pt = PageTable::new().expect("out of memory for page tables");
    let nx = PteFlags::no_execute();
    let data = PteFlags::WRITABLE | PteFlags::GLOBAL | nx;

    // Direct map: all of 0..4 GiB (RAM, MMIO, firmware) plus RAM above it.
    let is_ram = |s: u64, e: u64| {
        boot.memory
            .iter()
            .any(|r| r.is_usable() && r.base < e && s < r.base + r.len)
    };
    let mut phys = 0;
    while phys < LOW_DIRECT_MAP {
        let mut flags = data;
        if !is_ram(phys, phys + HUGE_PAGE) {
            flags |= PteFlags::NO_CACHE | PteFlags::WRITE_THROUGH;
        }
        pt.map_2m(phys_to_virt(phys), phys, flags)
            .expect("direct map");
        phys += HUGE_PAGE;
    }
    for r in boot
        .memory
        .iter()
        .filter(|r| r.is_usable() && r.base + r.len > LOW_DIRECT_MAP)
    {
        let mut p = align_down(r.base.max(LOW_DIRECT_MAP), HUGE_PAGE);
        while p < r.base + r.len {
            let _ = pt.map_2m(phys_to_virt(p), p, data);
            p += HUGE_PAGE;
        }
    }

    // Kernel image with section permissions.
    let k = kernel_layout();
    let map_section = |pt: &mut PageTable, (start, end): (u64, u64), flags: PteFlags| {
        let mut v = start;
        while v < end {
            pt.map(v, v - KERNEL_VMA, flags | PteFlags::GLOBAL)
                .expect("kernel image");
            v += PAGE_SIZE;
        }
    };
    map_section(&mut pt, k.text, PteFlags::EMPTY);
    map_section(&mut pt, k.rodata, nx);
    map_section(
        &mut pt,
        (k.data.0, align_up(k.data.1, PAGE_SIZE)),
        PteFlags::WRITABLE | nx,
    );

    // Pre-create every upper-half PDPT so later kernel mappings (stacks)
    // show up in all address spaces, which copy these PML4 entries.
    for i in KERNEL_HALF_START..ENTRIES {
        if pt.pml4_entry(i) & PteFlags::PRESENT.bits() == 0 {
            let pdpt = frame::alloc_zeroed().expect("out of memory for page tables");
            pt.set_pml4_entry(i, pdpt | (PteFlags::PRESENT | PteFlags::WRITABLE).bits());
        }
    }
    debug_assert!(pml4_index(KSTACK_REGION) >= KERNEL_HALF_START);

    cpu::enable_global_pages();
    unsafe { pt.activate() };
    KERNEL_PML4.call_once(|| pt.root());
}

/// Copies the kernel half into a new top-level table.
pub fn share_kernel_half(pt: &mut PageTable) {
    let k = kernel_page_table();
    for i in KERNEL_HALF_START..ENTRIES {
        pt.set_pml4_entry(i, k.pml4_entry(i));
    }
}
