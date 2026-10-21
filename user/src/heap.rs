//! Heap allocator: the same size-class allocator the kernel uses, with
//! pages obtained from `mmap`. Processes are single-threaded.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use huldra_abi::mm::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_READ, PROT_WRITE};
use huldra_kalloc::{PageSource, SlabAllocator, PAGE_SIZE};

/// Pages from `mmap`, with a cache: single pages are carved out of larger
/// mappings, and freed blocks are kept for reuse instead of being unmapped
/// (allocation-heavy programs such as the compilers would otherwise spend
/// most of their time in `mmap`/`munmap` and page faults).
struct MmapPages {
    /// Free blocks of each order (linked through their first word).
    free: [usize; CACHED_ORDERS],
    /// The unused part of the current chunk of single pages.
    chunk: usize,
    chunk_end: usize,
}

/// Orders up to 1 MiB blocks are cached; larger ones are unmapped.
const CACHED_ORDERS: usize = 9;
/// Single pages are carved from 256 KiB mappings.
const CHUNK: usize = 64 * PAGE_SIZE;

fn map(len: usize) -> usize {
    crate::sys::mmap(0, len, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0).unwrap_or(0)
}

impl PageSource for MmapPages {
    unsafe fn alloc_pages(&mut self, order: usize) -> *mut u8 {
        if order < CACHED_ORDERS && self.free[order] != 0 {
            let p = self.free[order];
            self.free[order] = *(p as *const usize);
            return p as *mut u8;
        }
        let len = PAGE_SIZE << order;
        if order == 0 {
            if self.chunk == self.chunk_end {
                let c = map(CHUNK);
                if c == 0 {
                    return core::ptr::null_mut();
                }
                self.chunk = c;
                self.chunk_end = c + CHUNK;
            }
            let p = self.chunk;
            self.chunk += PAGE_SIZE;
            return p as *mut u8;
        }
        // Over-allocate so the block can be aligned to its size.
        let raw = map(len * 2);
        if raw == 0 {
            return core::ptr::null_mut();
        }
        let aligned = (raw + len - 1) & !(len - 1);
        if aligned > raw {
            let _ = crate::sys::munmap(raw, aligned - raw);
        }
        let tail = raw + len * 2 - (aligned + len);
        if tail > 0 {
            let _ = crate::sys::munmap(aligned + len, tail);
        }
        aligned as *mut u8
    }

    unsafe fn free_pages(&mut self, ptr: *mut u8, order: usize) {
        if order < CACHED_ORDERS {
            *(ptr as *mut usize) = self.free[order];
            self.free[order] = ptr as usize;
            return;
        }
        let _ = crate::sys::munmap(ptr as usize, PAGE_SIZE << order);
    }
}

struct Heap(UnsafeCell<SlabAllocator<MmapPages>>);

// Single-threaded processes: no concurrent access.
unsafe impl Sync for Heap {}

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        (*self.0.get()).alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        (*self.0.get()).dealloc(ptr, layout)
    }
}

#[global_allocator]
static HEAP: Heap = Heap(UnsafeCell::new(SlabAllocator::new(MmapPages {
    free: [0; CACHED_ORDERS],
    chunk: 0,
    chunk_end: 0,
})));
