//! Memory management system calls.

use super::{value, Args, Ret};
use crate::fs::KResult;
use crate::mm::{align_up, PAGE_SIZE};
use crate::task::sched;
use huldra_abi::errno::Errno;
use huldra_abi::mm::*;

pub fn brk(a: &mut Args) -> KResult<Ret> {
    let me = sched::current();
    let mut mm = me.mm.lock();
    let mm = mm.as_mut().ok_or(Errno::ENOMEM)?;
    value(if a.a0() == 0 {
        mm.brk
    } else {
        mm.set_brk(a.a0())
    })
}

pub fn mmap(a: &mut Args) -> KResult<Ret> {
    let (hint, len, prot, flags, fd, offset) = (
        a.a0(),
        a.a1(),
        a.a2() as u32,
        a.a3() as u32,
        a.a4() as i32,
        a.a5(),
    );
    if len == 0 || offset % PAGE_SIZE != 0 {
        return Err(Errno::EINVAL);
    }
    let file = if flags & MAP_ANONYMOUS == 0 {
        if flags & MAP_SHARED != 0 {
            return Err(Errno::ENODEV); // shared file mappings need a page cache
        }
        Some(sched::current().files.lock().get(fd)?)
    } else {
        None
    };

    let me = sched::current();
    let addr = {
        let mut mm = me.mm.lock();
        let mm = mm.as_mut().ok_or(Errno::ENOMEM)?;
        // File contents are copied in below, so map writable at first.
        let initial_prot = if file.is_some() {
            prot | PROT_WRITE
        } else {
            prot
        };
        mm.mmap_anonymous(hint, len, initial_prot, flags & MAP_FIXED != 0)?
    };

    if let Some(f) = file {
        // Private file mapping: a copy of the file contents.
        let mut buf = alloc::vec![0u8; len as usize];
        let mut done = 0;
        while done < buf.len() {
            let n = f.pread(&mut buf[done..], offset + done as u64)?;
            if n == 0 {
                break;
            }
            done += n;
        }
        let mut mm = me.mm.lock();
        let mm = mm.as_mut().ok_or(Errno::ENOMEM)?;
        mm.write_bytes(addr, &buf[..done])?;
        if prot & PROT_WRITE == 0 {
            mm.protect(addr, addr + align_up(len, PAGE_SIZE), prot)?;
        }
    }
    value(addr)
}

pub fn munmap(a: &mut Args) -> KResult<Ret> {
    let (addr, len) = (a.a0(), align_up(a.a1(), PAGE_SIZE));
    if addr % PAGE_SIZE != 0 || len == 0 {
        return Err(Errno::EINVAL);
    }
    let me = sched::current();
    let mut mm = me.mm.lock();
    mm.as_mut()
        .ok_or(Errno::EINVAL)?
        .unmap_range(addr, addr + len);
    value(0)
}

pub fn mprotect(a: &mut Args) -> KResult<Ret> {
    let (addr, len) = (a.a0(), align_up(a.a1(), PAGE_SIZE));
    if addr % PAGE_SIZE != 0 {
        return Err(Errno::EINVAL);
    }
    let me = sched::current();
    let mut mm = me.mm.lock();
    mm.as_mut()
        .ok_or(Errno::EINVAL)?
        .protect(addr, addr + len, a.a2() as u32)?;
    value(0)
}
