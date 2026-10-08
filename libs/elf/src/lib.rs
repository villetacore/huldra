//! Minimal ELF64 (little-endian, x86_64) parser: just enough to load
//! statically linked executables.

#![no_std]

pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;
pub const EM_X86_64: u16 = 62;

pub const PT_LOAD: u32 = 1;
pub const PT_INTERP: u32 = 3;
pub const PT_PHDR: u32 = 6;
pub const PT_TLS: u32 = 7;

pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

pub const EHDR_SIZE: usize = 64;
pub const PHDR_SIZE: usize = 56;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    TooShort,
    BadMagic,
    NotElf64,
    NotLittleEndian,
    WrongMachine,
    BadProgramHeaders,
    SegmentOutOfFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramHeader {
    pub kind: u32,
    pub flags: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

impl ProgramHeader {
    pub fn readable(&self) -> bool {
        self.flags & PF_R != 0
    }
    pub fn writable(&self) -> bool {
        self.flags & PF_W != 0
    }
    pub fn executable(&self) -> bool {
        self.flags & PF_X != 0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Elf<'a> {
    data: &'a [u8],
    pub kind: u16,
    pub entry: u64,
    pub phoff: u64,
    pub phnum: u16,
}

fn u16_at(d: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([d[off], d[off + 1]])
}

fn u32_at(d: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(d[off..off + 4].try_into().unwrap())
}

fn u64_at(d: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(d[off..off + 8].try_into().unwrap())
}

impl<'a> Elf<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Elf<'a>, Error> {
        if data.len() < EHDR_SIZE {
            return Err(Error::TooShort);
        }
        if &data[..4] != b"\x7fELF" {
            return Err(Error::BadMagic);
        }
        if data[4] != 2 {
            return Err(Error::NotElf64);
        }
        if data[5] != 1 {
            return Err(Error::NotLittleEndian);
        }
        if u16_at(data, 18) != EM_X86_64 {
            return Err(Error::WrongMachine);
        }
        let phentsize = u16_at(data, 54) as usize;
        let elf = Elf {
            data,
            kind: u16_at(data, 16),
            entry: u64_at(data, 24),
            phoff: u64_at(data, 32),
            phnum: u16_at(data, 56),
        };
        let table_end = (elf.phoff as usize).checked_add(elf.phnum as usize * PHDR_SIZE);
        if (elf.phnum > 0 && phentsize != PHDR_SIZE) || table_end.is_none_or(|e| e > data.len()) {
            return Err(Error::BadProgramHeaders);
        }
        for ph in elf.program_headers() {
            if ph.kind == PT_LOAD
                && (ph.filesz > ph.memsz || ph.offset.checked_add(ph.filesz).is_none_or(|e| e > data.len() as u64))
            {
                return Err(Error::SegmentOutOfFile);
            }
        }
        Ok(elf)
    }

    pub fn program_headers(&self) -> impl Iterator<Item = ProgramHeader> + '_ {
        (0..self.phnum as usize).map(move |i| {
            let o = self.phoff as usize + i * PHDR_SIZE;
            let d = self.data;
            ProgramHeader {
                kind: u32_at(d, o),
                flags: u32_at(d, o + 4),
                offset: u64_at(d, o + 8),
                vaddr: u64_at(d, o + 16),
                filesz: u64_at(d, o + 32),
                memsz: u64_at(d, o + 40),
                align: u64_at(d, o + 48),
            }
        })
    }

    /// File contents of a segment.
    pub fn segment_data(&self, ph: &ProgramHeader) -> &'a [u8] {
        &self.data[ph.offset as usize..(ph.offset + ph.filesz) as usize]
    }

    /// True if the program needs a dynamic loader.
    pub fn interpreter(&self) -> Option<&'a [u8]> {
        self.program_headers().find(|p| p.kind == PT_INTERP).map(|p| self.segment_data(&p))
    }

    /// Virtual address of the program header table once loaded, if it is
    /// part of a loaded segment.
    pub fn phdr_vaddr(&self) -> Option<u64> {
        if let Some(p) = self.program_headers().find(|p| p.kind == PT_PHDR) {
            return Some(p.vaddr);
        }
        self.program_headers()
            .filter(|p| p.kind == PT_LOAD)
            .find(|p| self.phoff >= p.offset && self.phoff < p.offset + p.filesz)
            .map(|p| p.vaddr + (self.phoff - p.offset))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// Builds a small ELF with the given (flags, vaddr, data, memsz) segments.
    fn build(segments: &[(u32, u64, &[u8], u64)]) -> Vec<u8> {
        let mut out = std::vec![0u8; EHDR_SIZE];
        out[..4].copy_from_slice(b"\x7fELF");
        out[4] = 2;
        out[5] = 1;
        out[6] = 1;
        out[16..18].copy_from_slice(&ET_EXEC.to_le_bytes());
        out[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
        out[24..32].copy_from_slice(&0x40_1000u64.to_le_bytes());
        out[32..40].copy_from_slice(&(EHDR_SIZE as u64).to_le_bytes());
        out[54..56].copy_from_slice(&(PHDR_SIZE as u16).to_le_bytes());
        out[56..58].copy_from_slice(&(segments.len() as u16).to_le_bytes());
        let data_start = EHDR_SIZE + segments.len() * PHDR_SIZE;
        let mut offset = data_start;
        let mut phdrs = Vec::new();
        for &(flags, vaddr, data, memsz) in segments {
            let mut ph = std::vec![0u8; PHDR_SIZE];
            ph[0..4].copy_from_slice(&PT_LOAD.to_le_bytes());
            ph[4..8].copy_from_slice(&flags.to_le_bytes());
            ph[8..16].copy_from_slice(&(offset as u64).to_le_bytes());
            ph[16..24].copy_from_slice(&vaddr.to_le_bytes());
            ph[32..40].copy_from_slice(&(data.len() as u64).to_le_bytes());
            ph[40..48].copy_from_slice(&memsz.to_le_bytes());
            phdrs.extend_from_slice(&ph);
            offset += data.len();
        }
        out.extend_from_slice(&phdrs);
        for &(_, _, data, _) in segments {
            out.extend_from_slice(data);
        }
        out
    }

    #[test]
    fn parses_segments() {
        let image = build(&[(PF_R | PF_X, 0x40_1000, b"code", 4), (PF_R | PF_W, 0x40_2000, b"data", 0x100)]);
        let elf = Elf::parse(&image).unwrap();
        assert_eq!(elf.kind, ET_EXEC);
        assert_eq!(elf.entry, 0x40_1000);
        let phs: Vec<_> = elf.program_headers().collect();
        assert_eq!(phs.len(), 2);
        assert!(phs[0].executable() && !phs[0].writable());
        assert_eq!(elf.segment_data(&phs[1]), b"data");
        assert_eq!(phs[1].memsz, 0x100);
        assert!(elf.interpreter().is_none());
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(Elf::parse(b"short").unwrap_err(), Error::TooShort);
        let mut image = build(&[(PF_R, 0x40_0000, b"x", 1)]);
        image[0] = 0;
        assert_eq!(Elf::parse(&image).unwrap_err(), Error::BadMagic);
        let mut image = build(&[(PF_R, 0x40_0000, b"x", 1)]);
        image[18] = 3; // EM_386
        assert_eq!(Elf::parse(&image).unwrap_err(), Error::WrongMachine);
    }

    #[test]
    fn rejects_segment_past_end() {
        let mut image = build(&[(PF_R, 0x40_0000, b"abcd", 4)]);
        let ph = EHDR_SIZE;
        image[ph + 32..ph + 40].copy_from_slice(&1000u64.to_le_bytes()); // filesz
        image[ph + 40..ph + 48].copy_from_slice(&1000u64.to_le_bytes()); // memsz
        assert_eq!(Elf::parse(&image).unwrap_err(), Error::SegmentOutOfFile);
    }
}
