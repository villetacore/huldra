//! Physical page frame allocator (buddy system, see `huldra-buddy`).
//!
//! The per-frame metadata array is carved out of the first usable region
//! large enough to hold it. Reserved: everything below the end of the kernel
//! image, boot modules (initrd) and the metadata array itself.

use super::{align_down, align_up, kernel_phys_end, phys_to_virt, PAGE_SIZE};
use crate::bootinfo::BootInfo;
use crate::sync::SpinLock;
use core::mem::size_of;
use huldra_buddy::{BuddyAllocator, PageMeta};

/// Memory the boot page tables can reach (direct map of 0..4 GiB).
const EARLY_LIMIT: u64 = 4 << 30;

static BUDDY: SpinLock<Option<BuddyAllocator<'static>>> = SpinLock::new(None);

fn with<R>(f: impl FnOnce(&mut BuddyAllocator<'static>) -> R) -> R {
    f(BUDDY
        .lock()
        .as_mut()
        .expect("frame allocator not initialized"))
}

/// Calls `f(start, end)` for the parts of `[start, end)` not covered by `holes`.
fn for_each_gap(start: u64, end: u64, holes: &[(u64, u64)], mut f: impl FnMut(u64, u64)) {
    let mut sorted = [(0u64, 0u64); 16];
    let n = holes.len().min(16);
    sorted[..n].copy_from_slice(&holes[..n]);
    sorted[..n].sort_unstable();
    let mut cur = start;
    for &(hs, he) in &sorted[..n] {
        if he <= cur || hs >= end {
            continue;
        }
        if hs > cur {
            f(cur, hs);
        }
        cur = cur.max(he);
    }
    if cur < end {
        f(cur, end);
    }
}

pub fn init(boot: &BootInfo) {
    let max_pfn = boot
        .memory
        .iter()
        .filter(|r| r.is_usable())
        .map(|r| (r.base + r.len) / PAGE_SIZE)
        .max()
        .unwrap_or(0) as usize;
    let meta_bytes = align_up((max_pfn * size_of::<PageMeta>()) as u64, PAGE_SIZE);

    let mut holes = [(0u64, 0u64); 16];
    let mut nholes = 0;
    holes[nholes] = (0, kernel_phys_end());
    nholes += 1;
    for m in boot.modules.iter() {
        holes[nholes] = (align_down(m.start, PAGE_SIZE), align_up(m.end, PAGE_SIZE));
        nholes += 1;
    }

    // Place the metadata array in the first gap that fits (below 4 GiB).
    let mut meta_at = None;
    for r in boot.memory.iter().filter(|r| r.is_usable()) {
        let end = (r.base + r.len).min(EARLY_LIMIT);
        for_each_gap(
            align_up(r.base, PAGE_SIZE),
            align_down(end, PAGE_SIZE),
            &holes[..nholes],
            |s, e| {
                if meta_at.is_none() && e - s >= meta_bytes {
                    meta_at = Some(s);
                }
            },
        );
    }
    let meta_at = meta_at.expect("no room for the page frame metadata");
    holes[nholes] = (meta_at, meta_at + meta_bytes);
    nholes += 1;

    let meta =
        unsafe { core::slice::from_raw_parts_mut(phys_to_virt(meta_at) as *mut PageMeta, max_pfn) };
    let mut buddy = BuddyAllocator::new(meta);
    for r in boot.memory.iter().filter(|r| r.is_usable()) {
        let end = (r.base + r.len).min(EARLY_LIMIT);
        if r.base >= end {
            continue;
        }
        for_each_gap(
            align_up(r.base, PAGE_SIZE),
            align_down(end, PAGE_SIZE),
            &holes[..nholes],
            |s, e| buddy.add_range((s / PAGE_SIZE) as usize, (e / PAGE_SIZE) as usize),
        );
    }
    *BUDDY.lock() = Some(buddy);
}

/// Adds usable memory above 4 GiB, once the full direct map is active.
pub fn add_high_memory(boot: &BootInfo) {
    with(|b| {
        for r in boot.memory.iter().filter(|r| r.is_usable()) {
            let start = align_up(r.base.max(EARLY_LIMIT), PAGE_SIZE);
            let end = align_down(r.base + r.len, PAGE_SIZE);
            if end > start {
                b.add_range((start / PAGE_SIZE) as usize, (end / PAGE_SIZE) as usize);
            }
        }
    });
}

/// Returns a no-longer-needed reserved range (e.g. the initrd) to the allocator.
pub fn release_range(start: u64, end: u64) {
    let (s, e) = (align_up(start, PAGE_SIZE), align_down(end, PAGE_SIZE));
    if e > s {
        with(|b| b.add_range((s / PAGE_SIZE) as usize, (e / PAGE_SIZE) as usize));
    }
}

/// Allocates 2^order contiguous frames; returns the physical address.
pub fn alloc_pages(order: usize) -> Option<u64> {
    with(|b| b.alloc(order)).map(|pfn| pfn as u64 * PAGE_SIZE)
}

pub fn free_pages(phys: u64, order: usize) {
    with(|b| b.free((phys / PAGE_SIZE) as usize, order))
}

pub fn alloc() -> Option<u64> {
    alloc_pages(0)
}

pub fn alloc_zeroed() -> Option<u64> {
    let f = alloc()?;
    unsafe { core::ptr::write_bytes(phys_to_virt(f) as *mut u8, 0, PAGE_SIZE as usize) };
    Some(f)
}

pub fn free(phys: u64) {
    free_pages(phys, 0)
}

/// Returns (free, managed) frame counts.
pub fn stats() -> (usize, usize) {
    with(|b| (b.free_frames(), b.managed_frames()))
}
