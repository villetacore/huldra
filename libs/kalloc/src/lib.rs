//! Size-class ("slab") allocator.
//!
//! Small objects (up to [`MAX_SMALL`] bytes) come from per-size-class free
//! lists; each class is refilled by carving a fresh page into equal objects.
//! Larger requests go straight to the page source as 2^order pages. Used by
//! the kernel (pages from the buddy allocator) and by user space (pages from
//! `mmap`).

#![no_std]

use core::alloc::Layout;
use core::ptr::null_mut;

pub const PAGE_SIZE: usize = 4096;
pub const MAX_SMALL: usize = 2048;
const MIN_SMALL: usize = 16;
const CLASSES: usize = 8; // 16, 32, ..., 2048

/// Provides page-aligned blocks of 2^order pages.
pub trait PageSource {
    /// Returns null on failure.
    unsafe fn alloc_pages(&mut self, order: usize) -> *mut u8;
    unsafe fn free_pages(&mut self, ptr: *mut u8, order: usize);
}

struct FreeObject {
    next: *mut FreeObject,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Stats {
    /// Bytes handed out to callers (rounded to class / page sizes).
    pub allocated: usize,
    /// Bytes obtained from the page source for small objects.
    pub slab_pages_bytes: usize,
    /// Bytes currently held by large allocations.
    pub large_bytes: usize,
}

pub struct SlabAllocator<P: PageSource> {
    heads: [*mut FreeObject; CLASSES],
    source: P,
    stats: Stats,
}

unsafe impl<P: PageSource + Send> Send for SlabAllocator<P> {}

enum Kind {
    Small(usize),
    Large(usize),
}

fn classify(layout: &Layout) -> Kind {
    let size = layout.size().max(layout.align()).max(MIN_SMALL);
    if size <= MAX_SMALL {
        let class_size = size.next_power_of_two();
        Kind::Small(class_size.trailing_zeros() as usize - MIN_SMALL.trailing_zeros() as usize)
    } else {
        let pages = size.div_ceil(PAGE_SIZE);
        Kind::Large(pages.next_power_of_two().trailing_zeros() as usize)
    }
}

const fn class_size(class: usize) -> usize {
    MIN_SMALL << class
}

impl<P: PageSource> SlabAllocator<P> {
    pub const fn new(source: P) -> Self {
        SlabAllocator { heads: [null_mut(); CLASSES], source, stats: Stats { allocated: 0, slab_pages_bytes: 0, large_bytes: 0 } }
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    pub fn source(&mut self) -> &mut P {
        &mut self.source
    }

    unsafe fn refill(&mut self, class: usize) -> bool {
        let page = self.source.alloc_pages(0);
        if page.is_null() {
            return false;
        }
        self.stats.slab_pages_bytes += PAGE_SIZE;
        let size = class_size(class);
        for i in (0..PAGE_SIZE / size).rev() {
            let obj = page.add(i * size) as *mut FreeObject;
            obj.write(FreeObject { next: self.heads[class] });
            self.heads[class] = obj;
        }
        true
    }

    /// # Safety
    /// Standard `GlobalAlloc::alloc` contract.
    pub unsafe fn alloc(&mut self, layout: Layout) -> *mut u8 {
        match classify(&layout) {
            Kind::Small(class) => {
                if self.heads[class].is_null() && !self.refill(class) {
                    return null_mut();
                }
                let obj = self.heads[class];
                self.heads[class] = (*obj).next;
                self.stats.allocated += class_size(class);
                obj as *mut u8
            }
            Kind::Large(order) => {
                let p = self.source.alloc_pages(order);
                if !p.is_null() {
                    self.stats.allocated += PAGE_SIZE << order;
                    self.stats.large_bytes += PAGE_SIZE << order;
                }
                p
            }
        }
    }

    /// # Safety
    /// `ptr` must come from [`alloc`](Self::alloc) with the same `layout`.
    pub unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        match classify(&layout) {
            Kind::Small(class) => {
                let obj = ptr as *mut FreeObject;
                obj.write(FreeObject { next: self.heads[class] });
                self.heads[class] = obj;
                self.stats.allocated -= class_size(class);
            }
            Kind::Large(order) => {
                self.source.free_pages(ptr, order);
                self.stats.allocated -= PAGE_SIZE << order;
                self.stats.large_bytes -= PAGE_SIZE << order;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::alloc::{alloc, dealloc};
    use std::vec::Vec;

    struct HostPages {
        live: usize,
    }

    impl PageSource for HostPages {
        unsafe fn alloc_pages(&mut self, order: usize) -> *mut u8 {
            self.live += 1;
            let size = PAGE_SIZE << order;
            alloc(Layout::from_size_align(size, size).unwrap())
        }
        unsafe fn free_pages(&mut self, ptr: *mut u8, order: usize) {
            self.live -= 1;
            let size = PAGE_SIZE << order;
            dealloc(ptr, Layout::from_size_align(size, size).unwrap())
        }
    }

    #[test]
    fn small_objects_are_aligned_and_distinct() {
        let mut a = SlabAllocator::new(HostPages { live: 0 });
        let mut ptrs = Vec::new();
        for size in [1usize, 8, 16, 17, 100, 1000, 2048] {
            for align in [1usize, 8, 64] {
                let l = Layout::from_size_align(size, align).unwrap();
                for _ in 0..50 {
                    let p = unsafe { a.alloc(l) };
                    assert!(!p.is_null());
                    assert_eq!(p as usize % align, 0);
                    unsafe { p.write_bytes(0xAB, size) };
                    ptrs.push((p, l));
                }
            }
        }
        let mut addrs: Vec<usize> = ptrs.iter().map(|(p, _)| *p as usize).collect();
        addrs.sort();
        addrs.dedup();
        assert_eq!(addrs.len(), ptrs.len());
        for (p, l) in ptrs {
            unsafe { a.dealloc(p, l) };
        }
        assert_eq!(a.stats().allocated, 0);
    }

    #[test]
    fn freed_objects_are_reused() {
        let mut a = SlabAllocator::new(HostPages { live: 0 });
        let l = Layout::from_size_align(64, 8).unwrap();
        let p = unsafe { a.alloc(l) };
        unsafe { a.dealloc(p, l) };
        let q = unsafe { a.alloc(l) };
        assert_eq!(p, q);
        assert_eq!(a.source().live, 1);
    }

    #[test]
    fn large_allocations_use_pages() {
        let mut a = SlabAllocator::new(HostPages { live: 0 });
        let l = Layout::from_size_align(3 * PAGE_SIZE, 4096).unwrap();
        let p = unsafe { a.alloc(l) };
        assert_eq!(p as usize % PAGE_SIZE, 0);
        assert_eq!(a.stats().large_bytes, 4 * PAGE_SIZE);
        unsafe { a.dealloc(p, l) };
        assert_eq!(a.source().live, 0);
        assert_eq!(a.stats().allocated, 0);
    }

    #[test]
    fn big_alignment_goes_large() {
        let mut a = SlabAllocator::new(HostPages { live: 0 });
        let l = Layout::from_size_align(16, 8192).unwrap();
        let p = unsafe { a.alloc(l) };
        assert_eq!(p as usize % 8192, 0);
        unsafe { a.dealloc(p, l) };
    }
}
