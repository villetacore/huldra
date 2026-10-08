//! x86_64 4-level page tables.
//!
//! Tables are accessed through the kernel's direct map, so any page table
//! (not just the active one) can be inspected and modified.

use super::cpu;
use crate::mm::{frame, phys_to_virt, PAGE_SIZE};
use core::ops::{BitAnd, BitOr, BitOrAssign, Not};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PteFlags(pub u64);

impl PteFlags {
    pub const EMPTY: PteFlags = PteFlags(0);
    pub const PRESENT: PteFlags = PteFlags(1 << 0);
    pub const WRITABLE: PteFlags = PteFlags(1 << 1);
    pub const USER: PteFlags = PteFlags(1 << 2);
    pub const WRITE_THROUGH: PteFlags = PteFlags(1 << 3);
    pub const NO_CACHE: PteFlags = PteFlags(1 << 4);
    pub const HUGE: PteFlags = PteFlags(1 << 7);
    pub const GLOBAL: PteFlags = PteFlags(1 << 8);
    const NX_BIT: PteFlags = PteFlags(1 << 63);

    /// NX if the CPU supports it, otherwise nothing (the bit is reserved).
    pub fn no_execute() -> PteFlags {
        static NX: crate::sync::Once<bool> = crate::sync::Once::new();
        if *NX.call_once(cpu::has_nx) {
            Self::NX_BIT
        } else {
            Self::EMPTY
        }
    }

    pub const fn contains(self, other: PteFlags) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn bits(self) -> u64 {
        self.0
    }
}

impl BitOr for PteFlags {
    type Output = PteFlags;
    fn bitor(self, rhs: PteFlags) -> PteFlags {
        PteFlags(self.0 | rhs.0)
    }
}

impl BitOrAssign for PteFlags {
    fn bitor_assign(&mut self, rhs: PteFlags) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for PteFlags {
    type Output = PteFlags;
    fn bitand(self, rhs: PteFlags) -> PteFlags {
        PteFlags(self.0 & rhs.0)
    }
}

impl Not for PteFlags {
    type Output = PteFlags;
    fn not(self) -> PteFlags {
        PteFlags(!self.0)
    }
}

const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
const FLAGS_MASK: u64 = !ADDR_MASK;
pub const ENTRIES: usize = 512;
/// PML4 slots 256..512 hold kernel mappings shared by every address space.
pub const KERNEL_HALF_START: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    AlreadyMapped,
    OutOfMemory,
    HugePageInTheWay,
}

type Table = [u64; ENTRIES];

fn table(phys: u64) -> &'static mut Table {
    unsafe { &mut *(phys_to_virt(phys) as *mut Table) }
}

fn index(virt: u64, level: usize) -> usize {
    ((virt >> (12 + 9 * (level - 1))) & 0x1FF) as usize
}

/// A page table hierarchy, identified by the physical address of its PML4.
pub struct PageTable {
    root: u64,
}

impl PageTable {
    /// Allocates an empty PML4.
    pub fn new() -> Option<PageTable> {
        Some(PageTable { root: frame::alloc_zeroed()? })
    }

    /// # Safety
    /// `root` must be a valid PML4 that outlives the returned value's use.
    pub const unsafe fn from_root(root: u64) -> PageTable {
        PageTable { root }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn is_active(&self) -> bool {
        cpu::read_cr3() & ADDR_MASK == self.root
    }

    /// # Safety
    /// The table must map the running kernel.
    pub unsafe fn activate(&self) {
        cpu::write_cr3(self.root);
    }

    /// Raw PML4 entry (used to share the kernel half between address spaces).
    pub fn pml4_entry(&self, i: usize) -> u64 {
        table(self.root)[i]
    }

    pub fn set_pml4_entry(&mut self, i: usize, value: u64) {
        table(self.root)[i] = value;
    }

    /// Returns the next-level table for `entry`, creating it if requested.
    fn descend(entry: &mut u64, create: bool, user: bool) -> Result<u64, MapError> {
        if *entry & PteFlags::PRESENT.0 == 0 {
            if !create {
                return Err(MapError::OutOfMemory);
            }
            let t = frame::alloc_zeroed().ok_or(MapError::OutOfMemory)?;
            *entry = t | PteFlags::PRESENT.0 | PteFlags::WRITABLE.0;
        }
        if *entry & PteFlags::HUGE.0 != 0 {
            return Err(MapError::HugePageInTheWay);
        }
        if user {
            *entry |= PteFlags::USER.0;
        }
        Ok(*entry & ADDR_MASK)
    }

    fn walk_create(&mut self, virt: u64, leaf_level: usize, user: bool) -> Result<&'static mut u64, MapError> {
        let mut t = self.root;
        for level in (leaf_level + 1..=4).rev() {
            t = Self::descend(&mut table(t)[index(virt, level)], true, user)?;
        }
        Ok(&mut table(t)[index(virt, leaf_level)])
    }

    /// Finds the leaf entry for `virt` and its level (1 = 4 KiB, 2 = 2 MiB).
    fn walk(&self, virt: u64) -> Option<(&'static mut u64, usize)> {
        let mut t = self.root;
        for level in (1..=4).rev() {
            let e = &mut table(t)[index(virt, level)];
            if *e & PteFlags::PRESENT.0 == 0 {
                return None;
            }
            if level == 1 || *e & PteFlags::HUGE.0 != 0 {
                return Some((e, level));
            }
            t = *e & ADDR_MASK;
        }
        None
    }

    /// Maps the 4 KiB page at `virt` to `phys`.
    pub fn map(&mut self, virt: u64, phys: u64, flags: PteFlags) -> Result<(), MapError> {
        let user = flags.contains(PteFlags::USER);
        let e = self.walk_create(virt, 1, user)?;
        if *e & PteFlags::PRESENT.0 != 0 {
            return Err(MapError::AlreadyMapped);
        }
        *e = (phys & ADDR_MASK) | (flags | PteFlags::PRESENT).0;
        Ok(())
    }

    /// Maps a 2 MiB page.
    pub fn map_2m(&mut self, virt: u64, phys: u64, flags: PteFlags) -> Result<(), MapError> {
        let user = flags.contains(PteFlags::USER);
        let e = self.walk_create(virt, 2, user)?;
        if *e & PteFlags::PRESENT.0 != 0 {
            return Err(MapError::AlreadyMapped);
        }
        *e = (phys & ADDR_MASK) | (flags | PteFlags::PRESENT | PteFlags::HUGE).0;
        Ok(())
    }

    /// Removes a 4 KiB mapping and returns the physical page it pointed to.
    pub fn unmap(&mut self, virt: u64) -> Option<u64> {
        let (e, level) = self.walk(virt)?;
        if level != 1 {
            return None;
        }
        let phys = *e & ADDR_MASK;
        *e = 0;
        self.flush(virt);
        Some(phys)
    }

    pub fn translate(&self, virt: u64) -> Option<(u64, PteFlags)> {
        let (e, level) = self.walk(virt)?;
        let page_size = PAGE_SIZE << (9 * (level - 1));
        let phys = (*e & ADDR_MASK & !(page_size - 1)) | (virt & (page_size - 1));
        Some((phys, PteFlags(*e & FLAGS_MASK)))
    }

    /// Replaces the flags of an existing 4 KiB mapping.
    pub fn set_flags(&mut self, virt: u64, flags: PteFlags) -> bool {
        match self.walk(virt) {
            Some((e, 1)) => {
                *e = (*e & ADDR_MASK) | (flags | PteFlags::PRESENT).0;
                self.flush(virt);
                true
            }
            _ => false,
        }
    }

    fn flush(&self, virt: u64) {
        // Kernel-half mappings are shared by all address spaces.
        if self.is_active() || virt >= crate::mm::PHYS_OFFSET {
            cpu::invlpg(virt);
        }
    }

    /// Frees all user-half page tables and the pages they map.
    ///
    /// # Safety
    /// No user mapping of this table may be used afterwards.
    pub unsafe fn free_user_half(&mut self) {
        unsafe fn free_level(t: u64, level: usize) {
            for &e in table(t).iter() {
                if e & PteFlags::PRESENT.0 == 0 {
                    continue;
                }
                let next = e & ADDR_MASK;
                if level == 1 {
                    frame::free(next);
                } else if e & PteFlags::HUGE.0 == 0 {
                    free_level(next, level - 1);
                }
            }
            frame::free(t);
        }
        let pml4 = table(self.root);
        for e in pml4.iter_mut().take(KERNEL_HALF_START) {
            if *e & PteFlags::PRESENT.0 != 0 {
                free_level(*e & ADDR_MASK, 3);
                *e = 0;
            }
        }
    }
}
