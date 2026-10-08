//! Heap allocator: the same size-class allocator the kernel uses, with
//! pages obtained from `mmap`. Processes are single-threaded.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use huldra_abi::mm::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_READ, PROT_WRITE};
use huldra_kalloc::{PageSource, SlabAllocator, PAGE_SIZE};

struct MmapPages;

impl PageSource for MmapPages {
    unsafe fn alloc_pages(&mut self, order: usize) -> *mut u8 {
        let len = PAGE_SIZE << order;
        if order == 0 {
            return match crate::sys::mmap(
                0,
                len,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            ) {
                Ok(a) => a as *mut u8,
                Err(_) => core::ptr::null_mut(),
            };
        }
        // Over-allocate so the block can be aligned to its size.
        let raw = match crate::sys::mmap(
            0,
            len * 2,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        ) {
            Ok(a) => a,
            Err(_) => return core::ptr::null_mut(),
        };
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
static HEAP: Heap = Heap(UnsafeCell::new(SlabAllocator::new(MmapPages)));
