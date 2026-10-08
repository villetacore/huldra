//! Memory management.
//!
//! Virtual address space layout:
//!
//! ```text
//! 0x0000_0000_0040_0000 .. 0x0000_7FFF_FFFF_F000   user space (per process)
//! 0xFFFF_8000_0000_0000 ..                          direct map of physical memory
//! 0xFFFF_FF00_0000_0000 .. 0xFFFF_FF80_0000_0000   kernel stacks (with guard pages)
//! 0xFFFF_FFFF_8000_0000 ..                          kernel image
//! ```

pub mod frame;
pub mod heap;
pub mod kstack;
pub mod vmm;

pub const PAGE_SIZE: u64 = 4096;

/// Start of the direct map: `virt = PHYS_OFFSET + phys`.
pub const PHYS_OFFSET: u64 = 0xFFFF_8000_0000_0000;
/// Kernel image base: `virt = KERNEL_VMA + phys` for the kernel's sections.
pub const KERNEL_VMA: u64 = 0xFFFF_FFFF_8000_0000;
/// Region for kernel stacks (PML4 slot 510).
pub const KSTACK_REGION: u64 = 0xFFFF_FF00_0000_0000;
/// First address that is not user space.
pub const USER_END: u64 = 0x0000_8000_0000_0000;

pub const fn phys_to_virt(phys: u64) -> u64 {
    phys + PHYS_OFFSET
}

pub const fn virt_to_phys_direct(virt: u64) -> u64 {
    virt - PHYS_OFFSET
}

pub const fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}

pub const fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

pub const fn is_user_address(addr: u64) -> bool {
    addr < USER_END
}

extern "C" {
    static __kernel_start: u8;
    static __kernel_end: u8;
    static __text_start: u8;
    static __text_end: u8;
    static __rodata_start: u8;
    static __rodata_end: u8;
    static __data_start: u8;
}

/// Kernel image sections as (start, end) virtual addresses.
pub struct KernelLayout {
    pub start: u64,
    pub end: u64,
    pub text: (u64, u64),
    pub rodata: (u64, u64),
    /// .data and .bss
    pub data: (u64, u64),
}

pub fn kernel_layout() -> KernelLayout {
    use core::ptr::addr_of;
    KernelLayout {
        start: addr_of!(__kernel_start) as u64,
        end: addr_of!(__kernel_end) as u64,
        text: (addr_of!(__text_start) as u64, addr_of!(__text_end) as u64),
        rodata: (
            addr_of!(__rodata_start) as u64,
            addr_of!(__rodata_end) as u64,
        ),
        data: (addr_of!(__data_start) as u64, addr_of!(__kernel_end) as u64),
    }
}

/// Physical end of the kernel image (everything below is reserved).
pub fn kernel_phys_end() -> u64 {
    align_up(kernel_layout().end - KERNEL_VMA, PAGE_SIZE)
}

pub const TESTS: &[crate::ktest::Test] = ktests![
    tests::heap_vec,
    tests::heap_alignment,
    tests::large_allocation_is_contiguous,
    tests::frames,
    tests::user_mappings,
    tests::kernel_stacks,
];

mod tests {
    use super::*;
    use crate::arch::paging::{PageTable, PteFlags};
    use alloc::vec::Vec;

    pub fn heap_vec() {
        let v: Vec<u64> = (0..100_000).collect();
        assert_eq!(v.iter().sum::<u64>(), 4_999_950_000);
    }

    pub fn heap_alignment() {
        use core::alloc::Layout;
        for (size, align) in [(1, 1), (100, 16), (100, 64), (3000, 4096), (100, 8192)] {
            let layout = Layout::from_size_align(size, align).unwrap();
            let p = unsafe { alloc::alloc::alloc(layout) };
            assert!(!p.is_null() && p as usize % align == 0);
            unsafe { alloc::alloc::dealloc(p, layout) };
        }
    }

    pub fn large_allocation_is_contiguous() {
        let v: Vec<u8> = alloc::vec![7u8; 1 << 20];
        let start = v.as_ptr() as u64;
        assert!(start >= PHYS_OFFSET);
        let first = virt_to_phys_direct(start);
        let last = virt_to_phys_direct(start + (1 << 20) - 1);
        assert_eq!(last - first, (1 << 20) - 1);
    }

    pub fn frames() {
        let (free_before, _) = frame::stats();
        let a = frame::alloc().expect("out of frames");
        let b = frame::alloc_pages(3).expect("out of frames");
        assert_ne!(a, b);
        assert_eq!(b % (8 * PAGE_SIZE), 0);
        frame::free(a);
        frame::free_pages(b, 3);
        assert_eq!(frame::stats().0, free_before);
    }

    pub fn user_mappings() {
        let (free_before, _) = frame::stats();
        let mut pt = PageTable::new().unwrap();
        let page = frame::alloc_zeroed().unwrap();
        let flags = PteFlags::USER | PteFlags::WRITABLE | PteFlags::no_execute();
        pt.map(0x40_0000, page, flags).unwrap();
        assert_eq!(
            pt.map(0x40_0000, page, flags),
            Err(crate::arch::paging::MapError::AlreadyMapped)
        );
        let (phys, f) = pt.translate(0x40_0123).unwrap();
        assert_eq!(phys, page + 0x123);
        assert!(f.contains(PteFlags::USER | PteFlags::WRITABLE));
        unsafe {
            pt.free_user_half();
            frame::free(pt.root());
        }
        assert_eq!(frame::stats().0, free_before);
    }

    pub fn kernel_stacks() {
        let s = kstack::KernelStack::new().expect("no kernel stack");
        let top = s.top();
        unsafe {
            let p = (top - 8) as *mut u64;
            p.write_volatile(0xDEAD_BEEF);
            assert_eq!(p.read_volatile(), 0xDEAD_BEEF);
        }
        let guard = top - kstack::STACK_SIZE - 1;
        assert!(
            vmm::kernel_page_table().translate(guard).is_none(),
            "guard page must be unmapped"
        );
    }
}
