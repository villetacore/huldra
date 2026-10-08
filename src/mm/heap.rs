//! Kernel heap: first-fit free-list allocator with coalescing.
//!
//! Every block is a multiple of 16 bytes and 16-byte aligned, so splitting
//! never produces fragments too small to hold a free-list node.

use crate::sync::SpinLock;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{addr_of_mut, null_mut};

pub const HEAP_SIZE: usize = 8 * 1024 * 1024;
const MIN_BLOCK: usize = 16;

#[repr(C, align(4096))]
struct HeapSpace([u8; HEAP_SIZE]);

static mut HEAP_SPACE: HeapSpace = HeapSpace([0; HEAP_SIZE]);

struct FreeBlock {
    size: usize,
    next: *mut FreeBlock,
}

struct Heap {
    head: *mut FreeBlock,
    used: usize,
}

unsafe impl Send for Heap {}

impl Heap {
    /// Inserts a free region keeping the list sorted, merging neighbours.
    unsafe fn add_free(&mut self, addr: usize, size: usize) {
        let mut prev: *mut FreeBlock = null_mut();
        let mut cur = self.head;
        while !cur.is_null() && (cur as usize) < addr {
            prev = cur;
            cur = (*cur).next;
        }

        let node = addr as *mut FreeBlock;
        node.write(FreeBlock { size, next: cur });
        if !cur.is_null() && addr + size == cur as usize {
            (*node).size += (*cur).size;
            (*node).next = (*cur).next;
        }

        if prev.is_null() {
            self.head = node;
        } else if prev as usize + (*prev).size == addr {
            (*prev).size += (*node).size;
            (*prev).next = (*node).next;
        } else {
            (*prev).next = node;
        }
    }

    unsafe fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        let mut prev: *mut FreeBlock = null_mut();
        let mut cur = self.head;
        while !cur.is_null() {
            let start = cur as usize;
            let end = start + (*cur).size;
            let aligned = (start + align - 1) & !(align - 1);
            if aligned + size <= end {
                let next = (*cur).next;
                if prev.is_null() {
                    self.head = next;
                } else {
                    (*prev).next = next;
                }
                if aligned > start {
                    self.add_free(start, aligned - start);
                }
                if aligned + size < end {
                    self.add_free(aligned + size, end - aligned - size);
                }
                self.used += size;
                return aligned as *mut u8;
            }
            prev = cur;
            cur = (*cur).next;
        }
        null_mut()
    }
}

fn block_size(layout: &Layout) -> (usize, usize) {
    let size = (layout.size().max(MIN_BLOCK) + MIN_BLOCK - 1) & !(MIN_BLOCK - 1);
    (size, layout.align().max(MIN_BLOCK))
}

pub struct KernelAllocator(SpinLock<Heap>);

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let (size, align) = block_size(&layout);
        self.0.lock().alloc(size, align)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let (size, _) = block_size(&layout);
        let mut heap = self.0.lock();
        heap.add_free(ptr as usize, size);
        heap.used -= size;
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator(SpinLock::new(Heap { head: null_mut(), used: 0 }));

pub fn init() {
    unsafe {
        let start = addr_of_mut!(HEAP_SPACE) as usize;
        ALLOCATOR.0.lock().add_free(start, HEAP_SIZE);
    }
}

/// Returns (used, total) bytes.
pub fn stats() -> (usize, usize) {
    (ALLOCATOR.0.lock().used, HEAP_SIZE)
}
