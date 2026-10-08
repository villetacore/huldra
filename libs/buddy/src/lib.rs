//! Binary buddy allocator for physical page frames.
//!
//! Frames are identified by their page frame number (PFN). Bookkeeping lives
//! in a caller-provided [`PageMeta`] array (one entry per frame), so the
//! allocator never touches the frames themselves. Free blocks of order `k`
//! (2^k frames, aligned to 2^k) sit in per-order doubly linked lists threaded
//! through the metadata array.

#![no_std]

pub const MAX_ORDER: usize = 10;
const NONE: u32 = u32::MAX;

const FLAG_FREE: u8 = 1;
const FLAG_RESERVED: u8 = 2;

/// Per-frame metadata.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct PageMeta {
    next: u32,
    prev: u32,
    order: u8,
    flags: u8,
    _reserved: u16,
}

impl PageMeta {
    /// State of a frame that is not (yet) managed by the allocator.
    pub const RESERVED: PageMeta = PageMeta { next: NONE, prev: NONE, order: 0, flags: FLAG_RESERVED, _reserved: 0 };
}

pub struct BuddyAllocator<'a> {
    meta: &'a mut [PageMeta],
    heads: [u32; MAX_ORDER + 1],
    free_frames: usize,
    managed_frames: usize,
}

impl<'a> BuddyAllocator<'a> {
    /// Creates an allocator for frames `0..meta.len()`; all start reserved.
    pub fn new(meta: &'a mut [PageMeta]) -> Self {
        assert!(meta.len() < NONE as usize);
        meta.fill(PageMeta::RESERVED);
        BuddyAllocator { meta, heads: [NONE; MAX_ORDER + 1], free_frames: 0, managed_frames: 0 }
    }

    pub fn frame_count(&self) -> usize {
        self.meta.len()
    }

    pub fn free_frames(&self) -> usize {
        self.free_frames
    }

    /// Frames ever handed to the allocator via [`add_range`](Self::add_range).
    pub fn managed_frames(&self) -> usize {
        self.managed_frames
    }

    /// Hands frames `start..end` to the allocator.
    pub fn add_range(&mut self, start: usize, end: usize) {
        let end = end.min(self.meta.len());
        let mut pfn = start;
        while pfn < end {
            let mut order = MAX_ORDER;
            while order > 0 && (pfn & ((1 << order) - 1) != 0 || pfn + (1 << order) > end) {
                order -= 1;
            }
            for f in pfn..pfn + (1 << order) {
                self.meta[f].flags = 0;
            }
            self.managed_frames += 1 << order;
            self.free(pfn, order);
            pfn += 1 << order;
        }
    }

    fn push(&mut self, pfn: usize, order: usize) {
        let head = self.heads[order];
        self.meta[pfn] = PageMeta { next: head, prev: NONE, order: order as u8, flags: FLAG_FREE, _reserved: 0 };
        if head != NONE {
            self.meta[head as usize].prev = pfn as u32;
        }
        self.heads[order] = pfn as u32;
    }

    fn unlink(&mut self, pfn: usize, order: usize) {
        let PageMeta { next, prev, .. } = self.meta[pfn];
        if prev == NONE {
            self.heads[order] = next;
        } else {
            self.meta[prev as usize].next = next;
        }
        if next != NONE {
            self.meta[next as usize].prev = prev;
        }
        self.meta[pfn].flags = 0;
        self.meta[pfn].next = NONE;
        self.meta[pfn].prev = NONE;
    }

    /// Allocates 2^order contiguous frames aligned to 2^order; returns the first PFN.
    pub fn alloc(&mut self, order: usize) -> Option<usize> {
        if order > MAX_ORDER {
            return None;
        }
        let mut k = (order..=MAX_ORDER).find(|&k| self.heads[k] != NONE)?;
        let pfn = self.heads[k] as usize;
        self.unlink(pfn, k);
        while k > order {
            k -= 1;
            self.push(pfn + (1 << k), k);
        }
        self.meta[pfn].order = order as u8;
        self.free_frames -= 1 << order;
        Some(pfn)
    }

    /// Returns a block obtained from [`alloc`](Self::alloc) with the same order.
    pub fn free(&mut self, mut pfn: usize, mut order: usize) {
        assert!(pfn & ((1 << order) - 1) == 0, "misaligned free");
        assert!(self.meta[pfn].flags & (FLAG_FREE | FLAG_RESERVED) == 0, "double free of frame {pfn}");
        self.free_frames += 1 << order;
        while order < MAX_ORDER {
            let buddy = pfn ^ (1 << order);
            if buddy >= self.meta.len() {
                break;
            }
            let m = self.meta[buddy];
            if m.flags & FLAG_FREE == 0 || m.order as usize != order {
                break;
            }
            self.unlink(buddy, order);
            pfn = pfn.min(buddy);
            order += 1;
        }
        self.push(pfn, order);
    }

    /// Number of free blocks per order (for diagnostics).
    pub fn free_blocks(&self) -> [usize; MAX_ORDER + 1] {
        let mut counts = [0; MAX_ORDER + 1];
        for (order, count) in counts.iter_mut().enumerate() {
            let mut p = self.heads[order];
            while p != NONE {
                *count += 1;
                p = self.meta[p as usize].next;
            }
        }
        counts
    }
}

/// Smallest order whose block holds `frames` frames.
pub fn order_for(frames: usize) -> usize {
    frames.max(1).next_power_of_two().trailing_zeros() as usize
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec;
    use std::vec::Vec;

    fn meta(n: usize) -> Vec<PageMeta> {
        vec![PageMeta::RESERVED; n]
    }

    #[test]
    fn coalesces_back_to_max_blocks() {
        let mut m = meta(4096);
        let mut b = BuddyAllocator::new(&mut m);
        b.add_range(0, 4096);
        assert_eq!(b.free_frames(), 4096);
        assert_eq!(b.free_blocks()[MAX_ORDER], 4);

        let frames: Vec<usize> = (0..4096).map(|_| b.alloc(0).unwrap()).collect();
        assert_eq!(b.free_frames(), 0);
        assert!(b.alloc(0).is_none());
        for f in frames {
            b.free(f, 0);
        }
        assert_eq!(b.free_blocks()[MAX_ORDER], 4);
    }

    #[test]
    fn alignment_and_unaligned_ranges() {
        let mut m = meta(3000);
        let mut b = BuddyAllocator::new(&mut m);
        b.add_range(3, 2999);
        assert_eq!(b.free_frames(), 2996);
        for order in 0..=6 {
            let p = b.alloc(order).unwrap();
            assert_eq!(p % (1 << order), 0);
            assert!(p >= 3 && p + (1 << order) <= 2999);
            b.free(p, order);
        }
        assert_eq!(b.free_frames(), 2996);
    }

    #[test]
    fn random_workload_has_no_overlaps() {
        let mut m = meta(2048);
        let mut b = BuddyAllocator::new(&mut m);
        b.add_range(0, 2048);
        let mut owner = vec![false; 2048];
        let mut live: Vec<(usize, usize)> = Vec::new();
        let mut seed = 12345u64;
        let mut rand = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        for _ in 0..20_000 {
            if rand() % 3 != 0 || live.is_empty() {
                let order = rand() % 5;
                if let Some(p) = b.alloc(order) {
                    for f in p..p + (1 << order) {
                        assert!(!owner[f], "frame {f} handed out twice");
                        owner[f] = true;
                    }
                    live.push((p, order));
                }
            } else {
                let (p, order) = live.swap_remove(rand() % live.len());
                for f in p..p + (1 << order) {
                    owner[f] = false;
                }
                b.free(p, order);
            }
        }
        for (p, order) in live {
            b.free(p, order);
        }
        assert_eq!(b.free_frames(), 2048);
        assert_eq!(b.free_blocks()[MAX_ORDER], 2);
    }

    #[test]
    #[should_panic(expected = "double free")]
    fn double_free_is_detected() {
        let mut m = meta(16);
        let mut b = BuddyAllocator::new(&mut m);
        b.add_range(0, 16);
        let p = b.alloc(0).unwrap();
        b.free(p, 0);
        b.free(p, 0);
    }

    #[test]
    fn order_for_sizes() {
        assert_eq!(order_for(0), 0);
        assert_eq!(order_for(1), 0);
        assert_eq!(order_for(2), 1);
        assert_eq!(order_for(3), 2);
        assert_eq!(order_for(1024), 10);
    }
}
