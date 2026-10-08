//! Memory management.

pub mod frame;
pub mod heap;

pub const PAGE_SIZE: u64 = 4096;

pub const fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}

pub const fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

pub const TESTS: &[crate::ktest::Test] = ktests![tests::heap_vec, tests::heap_alignment, tests::frames];

mod tests {
    use alloc::vec::Vec;

    pub fn heap_vec() {
        let v: Vec<u64> = (0..10_000).collect();
        assert_eq!(v.iter().sum::<u64>(), 49_995_000);
    }

    pub fn heap_alignment() {
        use core::alloc::Layout;
        for align in [16, 64, 4096] {
            let layout = Layout::from_size_align(100, align).unwrap();
            let p = unsafe { alloc::alloc::alloc(layout) };
            assert!(!p.is_null() && p as usize % align == 0);
            unsafe { alloc::alloc::dealloc(p, layout) };
        }
    }

    pub fn frames() {
        let a = super::frame::alloc_frame().expect("out of frames");
        let b = super::frame::alloc_frame().expect("out of frames");
        assert_ne!(a, b);
        assert_eq!(a % super::PAGE_SIZE, 0);
        unsafe {
            super::frame::free_frame(a);
            super::frame::free_frame(b);
        }
    }
}
