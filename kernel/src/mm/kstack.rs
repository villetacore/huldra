//! Kernel stacks, mapped in a dedicated region with an unmapped guard page
//! below each stack so overflows fault instead of corrupting memory.

use super::{frame, vmm, KSTACK_REGION, PAGE_SIZE};
use crate::arch::paging::PteFlags;
use crate::sync::SpinLock;
use alloc::vec::Vec;

/// Usable stack size.
pub const STACK_SIZE: u64 = 32 * 1024;
/// Virtual size of one slot: guard page(s) + stack.
const SLOT_SIZE: u64 = 64 * 1024;
const MAX_SLOTS: u64 = (1 << 39) / SLOT_SIZE;

struct Slots {
    next: u64,
    free: Vec<u64>,
}

static SLOTS: SpinLock<Slots> = SpinLock::new(Slots {
    next: 0,
    free: Vec::new(),
});

pub struct KernelStack {
    slot: u64,
}

impl KernelStack {
    pub fn new() -> Option<KernelStack> {
        let slot = {
            let mut s = SLOTS.lock();
            match s.free.pop() {
                Some(slot) => slot,
                None if s.next < MAX_SLOTS => {
                    s.next += 1;
                    s.next - 1
                }
                None => return None,
            }
        };
        let stack = KernelStack { slot };
        let mut pt = vmm::kernel_page_table();
        let flags = PteFlags::WRITABLE | PteFlags::GLOBAL | PteFlags::no_execute();
        let mut v = stack.bottom();
        while v < stack.top() {
            let f = frame::alloc_zeroed()?;
            pt.map(v, f, flags).ok()?;
            v += PAGE_SIZE;
        }
        Some(stack)
    }

    pub fn bottom(&self) -> u64 {
        KSTACK_REGION + self.slot * SLOT_SIZE + (SLOT_SIZE - STACK_SIZE)
    }

    pub fn top(&self) -> u64 {
        KSTACK_REGION + (self.slot + 1) * SLOT_SIZE
    }

    /// True if `addr` is in the guard area below some kernel stack.
    pub fn is_guard_address(addr: u64) -> bool {
        addr >= KSTACK_REGION
            && addr < KSTACK_REGION + MAX_SLOTS * SLOT_SIZE
            && (addr - KSTACK_REGION) % SLOT_SIZE < SLOT_SIZE - STACK_SIZE
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        let mut pt = vmm::kernel_page_table();
        let mut v = self.bottom();
        while v < self.top() {
            if let Some(f) = pt.unmap(v) {
                frame::free(f);
            }
            v += PAGE_SIZE;
        }
        SLOTS.lock().free.push(self.slot);
    }
}
