//! Kernel heap: size-class allocator (`huldra-kalloc`) on top of the buddy
//! allocator. Objects live in the direct map; large allocations are
//! physically contiguous.

use super::{frame, phys_to_virt, virt_to_phys_direct};
use crate::sync::SpinLock;
use core::alloc::{GlobalAlloc, Layout};
use huldra_kalloc::{PageSource, SlabAllocator, Stats};

pub struct BuddyPages;

impl PageSource for BuddyPages {
    unsafe fn alloc_pages(&mut self, order: usize) -> *mut u8 {
        match frame::alloc_pages(order) {
            Some(p) => phys_to_virt(p) as *mut u8,
            None => core::ptr::null_mut(),
        }
    }

    unsafe fn free_pages(&mut self, ptr: *mut u8, order: usize) {
        frame::free_pages(virt_to_phys_direct(ptr as u64), order)
    }
}

pub struct KernelAllocator(SpinLock<SlabAllocator<BuddyPages>>);

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0.lock().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.0.lock().dealloc(ptr, layout)
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator(SpinLock::new(SlabAllocator::new(BuddyPages)));

pub fn stats() -> Stats {
    ALLOCATOR.0.lock().stats()
}
