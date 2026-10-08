//! Physical page frame allocator.
//!
//! Hands out 4 KiB frames from usable RAM above the kernel image (and
//! below 4 GiB, which is identity-mapped). Freed frames go to an
//! intrusive free list stored inside the frames themselves.

use super::{align_down, align_up, PAGE_SIZE};
use crate::bootinfo::MemRegion;
use crate::sync::SpinLock;
use core::ptr::addr_of;

const MAX_REGIONS: usize = 32;
const IDENTITY_MAPPED_LIMIT: u64 = 4 << 30;

extern "C" {
    static __kernel_start: u8;
    static __kernel_end: u8;
}

pub fn kernel_range() -> (u64, u64) {
    (addr_of!(__kernel_start) as u64, addr_of!(__kernel_end) as u64)
}

pub struct FrameAllocator {
    regions: [(u64, u64); MAX_REGIONS],
    count: usize,
    current: usize,
    next: u64,
    free_list: u64,
    total: u64,
    used: u64,
}

impl FrameAllocator {
    const fn new() -> Self {
        FrameAllocator {
            regions: [(0, 0); MAX_REGIONS],
            count: 0,
            current: 0,
            next: 0,
            free_list: 0,
            total: 0,
            used: 0,
        }
    }

    fn alloc(&mut self) -> Option<u64> {
        if self.free_list != 0 {
            let frame = self.free_list;
            self.free_list = unsafe { (frame as *const u64).read() };
            self.used += 1;
            return Some(frame);
        }
        while self.current < self.count {
            let (start, end) = self.regions[self.current];
            self.next = self.next.max(start);
            if self.next + PAGE_SIZE <= end {
                let frame = self.next;
                self.next += PAGE_SIZE;
                self.used += 1;
                return Some(frame);
            }
            self.current += 1;
        }
        None
    }

    unsafe fn free(&mut self, frame: u64) {
        (frame as *mut u64).write(self.free_list);
        self.free_list = frame;
        self.used -= 1;
    }
}

static FRAMES: SpinLock<FrameAllocator> = SpinLock::new(FrameAllocator::new());

pub fn init(memory: &[MemRegion]) {
    let floor = align_up(kernel_range().1, PAGE_SIZE);
    let mut fa = FRAMES.lock();
    for r in memory.iter().filter(|r| r.is_usable()) {
        let start = align_up(r.base.max(floor), PAGE_SIZE);
        let end = align_down((r.base + r.len).min(IDENTITY_MAPPED_LIMIT), PAGE_SIZE);
        if end > start && fa.count < MAX_REGIONS {
            let i = fa.count;
            fa.regions[i] = (start, end);
            fa.count += 1;
            fa.total += (end - start) / PAGE_SIZE;
        }
    }
}

#[allow(dead_code)]
pub fn alloc_frame() -> Option<u64> {
    FRAMES.lock().alloc()
}

/// # Safety
/// `frame` must come from [`alloc_frame`] and must not be used afterwards.
#[allow(dead_code)]
pub unsafe fn free_frame(frame: u64) {
    FRAMES.lock().free(frame)
}

/// Returns (used, total) frames.
pub fn stats() -> (u64, u64) {
    let fa = FRAMES.lock();
    (fa.used, fa.total)
}
