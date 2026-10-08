//! User address spaces.
//!
//! A `MemorySpace` is a page table plus a list of virtual memory areas
//! (VMAs). Pages of anonymous areas (heap, stack, `mmap`) are allocated on
//! first access by the page fault handler; program segments are filled in
//! by `exec`. `fork` copies every mapped page (no copy-on-write yet).

use crate::arch::paging::{PageTable, PteFlags};
use crate::fs::KResult;
use crate::mm::{align_down, align_up, frame, phys_to_virt, vmm, PAGE_SIZE, USER_END};
use alloc::collections::BTreeMap;
use huldra_abi::errno::Errno;
use huldra_abi::mm::{PROT_EXEC, PROT_READ, PROT_WRITE};

/// Lowest address user mappings may use (catches NULL dereferences).
pub const USER_MIN: u64 = 0x10000;
pub const STACK_TOP: u64 = 0x0000_7FFF_FFFF_F000;
pub const STACK_SIZE: u64 = 8 << 20;
/// `mmap` allocations are placed below this address, growing down.
pub const MMAP_TOP: u64 = 0x0000_7F00_0000_0000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VmaKind {
    Program,
    Heap,
    Stack,
    Anonymous,
}

#[derive(Clone, Copy, Debug)]
pub struct Vma {
    pub start: u64,
    pub end: u64,
    pub prot: u32,
    pub kind: VmaKind,
}

pub struct MemorySpace {
    pt: PageTable,
    vmas: BTreeMap<u64, Vma>,
    pub brk_start: u64,
    pub brk: u64,
}

fn pte_flags(prot: u32) -> PteFlags {
    let mut f = PteFlags::USER;
    if prot & PROT_WRITE != 0 {
        f |= PteFlags::WRITABLE;
    }
    if prot & PROT_EXEC == 0 {
        f |= PteFlags::no_execute();
    }
    f
}

impl MemorySpace {
    pub fn new() -> KResult<MemorySpace> {
        let mut pt = PageTable::new().ok_or(Errno::ENOMEM)?;
        vmm::share_kernel_half(&mut pt);
        Ok(MemorySpace {
            pt,
            vmas: BTreeMap::new(),
            brk_start: 0,
            brk: 0,
        })
    }

    pub fn root(&self) -> u64 {
        self.pt.root()
    }

    pub fn find(&self, addr: u64) -> Option<&Vma> {
        self.vmas
            .range(..=addr)
            .next_back()
            .map(|(_, v)| v)
            .filter(|v| addr < v.end)
    }

    fn overlaps(&self, start: u64, end: u64) -> bool {
        self.vmas
            .range(..end)
            .next_back()
            .is_some_and(|(_, v)| v.end > start)
    }

    pub fn add_vma(&mut self, start: u64, end: u64, prot: u32, kind: VmaKind) -> KResult<()> {
        if start >= end
            || start < USER_MIN
            || end > USER_END
            || start % PAGE_SIZE != 0
            || end % PAGE_SIZE != 0
        {
            return Err(Errno::EINVAL);
        }
        if self.overlaps(start, end) {
            return Err(Errno::EEXIST);
        }
        self.vmas.insert(
            start,
            Vma {
                start,
                end,
                prot,
                kind,
            },
        );
        Ok(())
    }

    /// Total size of all areas.
    pub fn vm_bytes(&self) -> u64 {
        self.vmas.values().map(|v| v.end - v.start).sum()
    }

    fn map_new_page(&mut self, page: u64, prot: u32) -> KResult<u64> {
        let f = frame::alloc_zeroed().ok_or(Errno::ENOMEM)?;
        if self.pt.map(page, f, pte_flags(prot)).is_err() {
            frame::free(f);
            return Err(Errno::ENOMEM);
        }
        Ok(f)
    }

    /// Resolves a fault at `addr`; false if the access is not allowed.
    pub fn handle_fault(&mut self, addr: u64, write: bool, exec: bool) -> bool {
        let Some(vma) = self.find(addr).copied() else {
            return false;
        };
        if (write && vma.prot & PROT_WRITE == 0)
            || (exec && vma.prot & PROT_EXEC == 0)
            || vma.prot == 0
        {
            return false;
        }
        let page = align_down(addr, PAGE_SIZE);
        if self.pt.translate(page).is_some() {
            return false; // present: a real protection violation
        }
        self.map_new_page(page, vma.prot).is_ok()
    }

    /// Makes sure every page of `[start, end)` is backed by memory.
    pub fn populate(&mut self, start: u64, end: u64) -> KResult<()> {
        let mut page = align_down(start, PAGE_SIZE);
        while page < end {
            if self.pt.translate(page).is_none() {
                let prot = self.find(page).ok_or(Errno::EFAULT)?.prot;
                self.map_new_page(page, prot)?;
            }
            page += PAGE_SIZE;
        }
        Ok(())
    }

    /// Writes into user memory through the direct map (works for read-only
    /// pages too, which is what `exec` needs). Pages are populated on demand.
    pub fn write_bytes(&mut self, addr: u64, data: &[u8]) -> KResult<()> {
        self.populate(addr, addr + data.len() as u64)?;
        let mut done = 0;
        while done < data.len() {
            let va = addr + done as u64;
            let (pa, _) = self.pt.translate(va).ok_or(Errno::EFAULT)?;
            let n = (PAGE_SIZE - va % PAGE_SIZE).min((data.len() - done) as u64) as usize;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    data[done..].as_ptr(),
                    phys_to_virt(pa) as *mut u8,
                    n,
                )
            };
            done += n;
        }
        Ok(())
    }

    /// Checks that `[addr, addr + len)` is mapped with the needed access.
    pub fn check_access(&self, addr: u64, len: u64, write: bool) -> KResult<()> {
        if len == 0 {
            return Ok(());
        }
        let end = addr.checked_add(len).ok_or(Errno::EFAULT)?;
        if end > USER_END {
            return Err(Errno::EFAULT);
        }
        let mut cur = addr;
        while cur < end {
            let vma = self.find(cur).ok_or(Errno::EFAULT)?;
            let needed = if write { PROT_WRITE } else { PROT_READ };
            if vma.prot & needed == 0 {
                return Err(Errno::EFAULT);
            }
            cur = vma.end;
        }
        Ok(())
    }

    fn unmap_pages(&mut self, start: u64, end: u64) {
        let mut page = start;
        while page < end {
            if let Some(f) = self.pt.unmap(page) {
                frame::free(f);
            }
            page += PAGE_SIZE;
        }
    }

    /// Removes `[start, end)` from the address space, splitting areas.
    pub fn unmap_range(&mut self, start: u64, end: u64) {
        let affected: alloc::vec::Vec<Vma> = self
            .vmas
            .range(..end)
            .map(|(_, v)| *v)
            .filter(|v| v.end > start)
            .collect();
        for v in affected {
            self.vmas.remove(&v.start);
            if v.start < start {
                self.vmas.insert(v.start, Vma { end: start, ..v });
            }
            if v.end > end {
                self.vmas.insert(end, Vma { start: end, ..v });
            }
            self.unmap_pages(v.start.max(start), v.end.min(end));
        }
    }

    /// Changes protection of `[start, end)` (must be fully mapped).
    pub fn protect(&mut self, start: u64, end: u64, prot: u32) -> KResult<()> {
        self.check_range_mapped(start, end)?;
        let affected: alloc::vec::Vec<Vma> = self
            .vmas
            .range(..end)
            .map(|(_, v)| *v)
            .filter(|v| v.end > start)
            .collect();
        for v in affected {
            self.vmas.remove(&v.start);
            if v.start < start {
                self.vmas.insert(v.start, Vma { end: start, ..v });
            }
            if v.end > end {
                self.vmas.insert(end, Vma { start: end, ..v });
            }
            let (s, e) = (v.start.max(start), v.end.min(end));
            self.vmas.insert(
                s,
                Vma {
                    start: s,
                    end: e,
                    prot,
                    kind: v.kind,
                },
            );
            let mut page = s;
            while page < e {
                self.pt.set_flags(page, pte_flags(prot));
                page += PAGE_SIZE;
            }
        }
        Ok(())
    }

    fn check_range_mapped(&self, start: u64, end: u64) -> KResult<()> {
        let mut cur = start;
        while cur < end {
            cur = self.find(cur).ok_or(Errno::ENOMEM)?.end;
        }
        Ok(())
    }

    /// Finds a free gap of `len` bytes below MMAP_TOP.
    fn find_gap(&self, len: u64) -> KResult<u64> {
        let mut top = MMAP_TOP;
        for (_, v) in self.vmas.range(..MMAP_TOP).rev() {
            if v.end <= top && top - v.end >= len {
                return Ok(top - len);
            }
            top = top.min(v.start);
        }
        if top >= len + USER_MIN.max(self.brk) {
            Ok(top - len)
        } else {
            Err(Errno::ENOMEM)
        }
    }

    /// Creates an anonymous mapping; returns its address.
    pub fn mmap_anonymous(&mut self, hint: u64, len: u64, prot: u32, fixed: bool) -> KResult<u64> {
        let len = align_up(len, PAGE_SIZE);
        if len == 0 {
            return Err(Errno::EINVAL);
        }
        let addr = if fixed {
            if hint % PAGE_SIZE != 0 {
                return Err(Errno::EINVAL);
            }
            self.unmap_range(hint, hint + len);
            hint
        } else if hint != 0
            && hint % PAGE_SIZE == 0
            && !self.overlaps(hint, hint + len)
            && hint >= USER_MIN
            && hint + len <= MMAP_TOP
        {
            hint
        } else {
            self.find_gap(len)?
        };
        self.add_vma(addr, addr + len, prot, VmaKind::Anonymous)?;
        Ok(addr)
    }

    /// `brk(2)`: grows or shrinks the heap; returns the new break.
    pub fn set_brk(&mut self, new: u64) -> u64 {
        if new < self.brk_start {
            return self.brk;
        }
        let old_end = align_up(self.brk, PAGE_SIZE);
        let new_end = align_up(new, PAGE_SIZE);
        if new_end > old_end {
            if self.overlaps(old_end, new_end) {
                return self.brk;
            }
            match self.vmas.get_mut(&self.brk_start) {
                Some(v) if v.kind == VmaKind::Heap => v.end = new_end,
                _ => {
                    if self
                        .add_vma(old_end, new_end, PROT_READ | PROT_WRITE, VmaKind::Heap)
                        .is_err()
                    {
                        return self.brk;
                    }
                }
            }
        } else if new_end < old_end {
            self.unmap_range(new_end, old_end);
        }
        self.brk = new;
        self.brk
    }

    /// Copies the address space for `fork`.
    pub fn duplicate(&self) -> KResult<MemorySpace> {
        let mut child = MemorySpace::new()?;
        child.brk_start = self.brk_start;
        child.brk = self.brk;
        for v in self.vmas.values() {
            child.vmas.insert(v.start, *v);
            let mut page = v.start;
            while page < v.end {
                if let Some((pa, flags)) = self.pt.translate(page) {
                    let f = frame::alloc().ok_or(Errno::ENOMEM)?;
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            phys_to_virt(pa) as *const u8,
                            phys_to_virt(f) as *mut u8,
                            PAGE_SIZE as usize,
                        )
                    };
                    let flags = flags & !(PteFlags::PRESENT | PteFlags(0x60)); // drop accessed/dirty
                    child.pt.map(page, f, flags).map_err(|_| Errno::ENOMEM)?;
                }
                page += PAGE_SIZE;
            }
        }
        Ok(child)
    }

    /// # Safety
    /// The address space must not be active on any CPU.
    pub unsafe fn activate(&self) {
        self.pt.activate();
    }
}

impl Drop for MemorySpace {
    fn drop(&mut self) {
        debug_assert!(!self.pt.is_active(), "dropping the active address space");
        unsafe {
            self.pt.free_user_half();
            frame::free(self.pt.root());
        }
    }
}
