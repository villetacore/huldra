//! Writes the generated image as a static ELF64 executable: one
//! read/execute segment with the headers and code, one read/write
//! segment with data and bss.

use crate::gen::{Image, BASE_ADDR, TEXT_ADDR};
use alloc::vec::Vec;

const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

fn phdr(out: &mut Vec<u8>, flags: u32, offset: u64, vaddr: u64, filesz: u64, memsz: u64) {
    out.extend_from_slice(&PT_LOAD.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&vaddr.to_le_bytes());
    out.extend_from_slice(&vaddr.to_le_bytes());
    out.extend_from_slice(&filesz.to_le_bytes());
    out.extend_from_slice(&memsz.to_le_bytes());
    out.extend_from_slice(&0x1000u64.to_le_bytes());
}

pub fn write(img: &Image) -> Vec<u8> {
    let has_data = img.data_mem > 0;
    let phnum: u16 = if has_data { 2 } else { 1 };
    let mut out = Vec::new();
    out.extend_from_slice(&[0x7F, b'E', b'L', b'F', 2, 1, 1, 0]);
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    out.extend_from_slice(&0x3Eu16.to_le_bytes()); // x86-64
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&img.entry.to_le_bytes());
    out.extend_from_slice(&64u64.to_le_bytes()); // phoff
    out.extend_from_slice(&0u64.to_le_bytes()); // shoff
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&64u16.to_le_bytes()); // ehsize
    out.extend_from_slice(&56u16.to_le_bytes()); // phentsize
    out.extend_from_slice(&phnum.to_le_bytes());
    out.extend_from_slice(&64u16.to_le_bytes()); // shentsize
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    let text_off = TEXT_ADDR - BASE_ADDR;
    let text_end = text_off + img.text.len() as u64;
    phdr(&mut out, PF_R | PF_X, 0, BASE_ADDR, text_end, text_end);
    let data_off = img.data_addr - BASE_ADDR;
    if has_data {
        phdr(
            &mut out,
            PF_R | PF_W,
            data_off,
            img.data_addr,
            img.data.len() as u64,
            img.data_mem as u64,
        );
    }
    out.resize(text_off as usize, 0);
    out.extend_from_slice(&img.text);
    if !img.data.is_empty() {
        out.resize(data_off as usize, 0);
        out.extend_from_slice(&img.data);
    }
    out
}
