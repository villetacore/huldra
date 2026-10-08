//! `execve`: replaces the current process image with a static ELF
//! executable (or a `#!` script).

use super::mm::{MemorySpace, VmaKind, STACK_SIZE, STACK_TOP};
use crate::arch::context::user_entry_frame;
use crate::arch::cpu::{wrmsr, MSR_FS_BASE};
use crate::arch::TrapFrame;
use crate::fs::{self, FileType, KResult};
use crate::mm::{align_down, align_up, PAGE_SIZE};
use crate::task::sched;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use huldra_abi::errno::Errno;
use huldra_abi::mm::{PROT_EXEC, PROT_READ, PROT_WRITE};
use huldra_abi::process::*;
use huldra_elf::{Elf, ET_DYN, ET_EXEC, PT_LOAD};

/// Load address for position-independent (static-pie) executables.
const PIE_BASE: u64 = 0x40_0000;
const MAX_SCRIPT_DEPTH: usize = 4;

pub fn exec(path: &str, argv: Vec<String>, envp: Vec<String>) -> KResult<TrapFrame> {
    exec_depth(path, argv, envp, 0)
}

fn exec_depth(path: &str, argv: Vec<String>, envp: Vec<String>, depth: usize) -> KResult<TrapFrame> {
    let meta = fs::stat(path)?;
    if meta.kind != FileType::Regular || meta.perm & 0o111 == 0 {
        return Err(Errno::EACCES);
    }
    let data = fs::read_file(path)?;

    if let Some(rest) = data.strip_prefix(b"#!") {
        if depth >= MAX_SCRIPT_DEPTH {
            return Err(Errno::ELOOP);
        }
        let line_end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
        let line = core::str::from_utf8(&rest[..line_end]).map_err(|_| Errno::ENOEXEC)?.trim();
        let mut parts = line.splitn(2, char::is_whitespace);
        let interp = parts.next().filter(|s| !s.is_empty()).ok_or(Errno::ENOEXEC)?;
        let mut new_argv = alloc::vec![String::from(interp)];
        if let Some(arg) = parts.next().map(str::trim).filter(|s| !s.is_empty()) {
            new_argv.push(String::from(arg));
        }
        new_argv.push(String::from(path));
        new_argv.extend(argv.into_iter().skip(1));
        let interp = super::resolve(interp)?;
        return exec_depth(&interp, new_argv, envp, depth + 1);
    }

    let elf = Elf::parse(&data).map_err(|_| Errno::ENOEXEC)?;
    if elf.interpreter().is_some() {
        return Err(Errno::ENOEXEC); // dynamically linked programs are not supported
    }
    let bias = match elf.kind {
        ET_EXEC => 0,
        ET_DYN => PIE_BASE,
        _ => return Err(Errno::ENOEXEC),
    };

    let mut mm = MemorySpace::new()?;

    // Page-aligned regions of all loadable segments, merged where they share pages.
    let mut regions: Vec<(u64, u64, u32)> = Vec::new();
    for ph in elf.program_headers().filter(|p| p.kind == PT_LOAD && p.memsz > 0) {
        let mut prot = 0;
        if ph.readable() {
            prot |= PROT_READ;
        }
        if ph.writable() {
            prot |= PROT_WRITE | PROT_READ;
        }
        if ph.executable() {
            prot |= PROT_EXEC | PROT_READ;
        }
        let start = align_down(ph.vaddr + bias, PAGE_SIZE);
        let end = align_up(ph.vaddr + bias + ph.memsz, PAGE_SIZE);
        regions.push((start, end, prot));
    }
    regions.sort_unstable();
    let mut merged: Vec<(u64, u64, u32)> = Vec::new();
    for (s, e, p) in regions {
        match merged.last_mut() {
            Some(last) if s < last.1 => {
                last.1 = last.1.max(e);
                last.2 |= p;
            }
            _ => merged.push((s, e, p)),
        }
    }
    if merged.is_empty() {
        return Err(Errno::ENOEXEC);
    }
    for &(s, e, p) in &merged {
        mm.add_vma(s, e, p, VmaKind::Program).map_err(|_| Errno::ENOEXEC)?;
    }
    for ph in elf.program_headers().filter(|p| p.kind == PT_LOAD) {
        mm.write_bytes(ph.vaddr + bias, elf.segment_data(&ph))?;
    }

    let image_end = merged.iter().map(|r| r.1).max().unwrap();
    mm.brk_start = image_end;
    mm.brk = image_end;
    mm.add_vma(STACK_TOP - STACK_SIZE, STACK_TOP, PROT_READ | PROT_WRITE, VmaKind::Stack)?;

    let entry = elf.entry + bias;
    let phdr = elf.phdr_vaddr().map_or(0, |p| p + bias);
    let sp = build_stack(&mut mm, path, &argv, &envp, &elf, entry, phdr)?;

    // Point of no return: switch to the new image.
    let me = sched::current();
    unsafe { mm.activate() };
    me.cr3.store(mm.root(), Ordering::Release);
    me.set_user();
    let old = me.mm.lock().replace(mm);
    drop(old);
    me.fs_base.store(0, Ordering::Relaxed);
    unsafe { wrmsr(MSR_FS_BASE, 0) };
    me.files.lock().close_on_exec();
    me.signals.lock().exec();
    *me.name.lock() = String::from(path.rsplit('/').next().unwrap_or(path));
    me.proc.lock().cmdline = argv;
    Ok(user_entry_frame(entry, sp))
}

/// Lays out argc/argv/envp/auxv at the top of the new stack (System V ABI).
fn build_stack(
    mm: &mut MemorySpace,
    path: &str,
    argv: &[String],
    envp: &[String],
    elf: &Elf,
    entry: u64,
    phdr: u64,
) -> KResult<u64> {
    struct Stack<'a> {
        mm: &'a mut MemorySpace,
        sp: u64,
    }
    impl Stack<'_> {
        fn push(&mut self, bytes: &[u8]) -> KResult<u64> {
            self.sp -= bytes.len() as u64;
            self.mm.write_bytes(self.sp, bytes)?;
            Ok(self.sp)
        }
        fn push_str(&mut self, s: &str) -> KResult<u64> {
            let mut v = Vec::with_capacity(s.len() + 1);
            v.extend_from_slice(s.as_bytes());
            v.push(0);
            self.push(&v)
        }
    }
    let mut st = Stack { mm, sp: STACK_TOP };

    let execfn = st.push_str(path)?;
    let env_ptrs: Vec<u64> = envp.iter().map(|s| st.push_str(s)).collect::<KResult<_>>()?;
    let arg_ptrs: Vec<u64> = argv.iter().map(|s| st.push_str(s)).collect::<KResult<_>>()?;
    let mut random = [0u8; 16];
    let tsc = crate::arch::cpu::rdtsc();
    for (i, b) in random.iter_mut().enumerate() {
        *b = (tsc.rotate_left(i as u32 * 5) as u8) ^ (i as u8).wrapping_mul(0x9D);
    }
    let random_ptr = st.push(&random)?;
    let sp = st.sp;

    let auxv: [(u64, u64); 12] = [
        (AT_PHDR, phdr),
        (AT_PHENT, huldra_elf::PHDR_SIZE as u64),
        (AT_PHNUM, elf.phnum as u64),
        (AT_PAGESZ, PAGE_SIZE),
        (AT_BASE, 0),
        (AT_ENTRY, entry),
        (AT_UID, 0),
        (AT_EUID, 0),
        (AT_GID, 0),
        (AT_EGID, 0),
        (AT_RANDOM, random_ptr),
        (AT_EXECFN, execfn),
    ];

    let mut words: Vec<u64> = Vec::new();
    words.push(argv.len() as u64);
    words.extend(&arg_ptrs);
    words.push(0);
    words.extend(&env_ptrs);
    words.push(0);
    for (k, v) in auxv {
        words.push(k);
        words.push(v);
    }
    words.push(AT_NULL);
    words.push(0);

    let size = (words.len() * 8) as u64;
    let start = (sp - size) & !15; // rsp must be 16-byte aligned at entry
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    mm.write_bytes(start, &bytes)?;
    Ok(start)
}
