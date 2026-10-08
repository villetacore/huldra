//! A tiny x86-64 assembler: just the instructions the code generator
//! uses, with labels and rip-relative fixups.

use alloc::vec::Vec;

pub const RAX: u8 = 0;
pub const RCX: u8 = 1;
pub const RDX: u8 = 2;
pub const RSP: u8 = 4;
pub const RBP: u8 = 5;
pub const RSI: u8 = 6;
pub const RDI: u8 = 7;
pub const R8: u8 = 8;
pub const R9: u8 = 9;
pub const R10: u8 = 10;

/// Condition codes for `jcc`/`setcc`.
#[derive(Clone, Copy)]
pub enum Cond {
    B = 2,
    Ae = 3,
    E = 4,
    Ne = 5,
    Be = 6,
    A = 7,
    P = 0xA,
    Np = 0xB,
    L = 0xC,
    Ge = 0xD,
    Le = 0xE,
    G = 0xF,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Text,
    Data,
    Bss,
}

/// A 32-bit pc-relative reference from code to a label.
pub struct Fixup {
    pub at: usize,
    pub label: usize,
    pub addend: i64,
}

#[derive(Default)]
pub struct Asm {
    pub code: Vec<u8>,
    pub labels: Vec<Option<(Section, usize)>>,
    pub fixups: Vec<Fixup>,
}

impl Asm {
    pub fn new(labels: usize) -> Self {
        Asm {
            code: Vec::new(),
            labels: alloc::vec![None; labels],
            fixups: Vec::new(),
        }
    }

    pub fn new_label(&mut self) -> usize {
        self.labels.push(None);
        self.labels.len() - 1
    }

    pub fn bind(&mut self, label: usize) {
        self.labels[label] = Some((Section::Text, self.code.len()));
    }

    pub fn pos(&self) -> usize {
        self.code.len()
    }

    fn emit(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }

    fn imm32(&mut self, v: i32) {
        self.emit(&v.to_le_bytes());
    }

    fn rel32(&mut self, label: usize) {
        self.fixups.push(Fixup {
            at: self.code.len(),
            label,
            addend: 0,
        });
        self.imm32(0);
    }

    fn rex(&mut self, w: bool, reg: u8, base: u8, force: bool) {
        let r = 0x40 | (w as u8) << 3 | (reg >> 3) << 2 | (base >> 3);
        if r != 0x40 || force {
            self.emit(&[r]);
        }
    }

    /// ModRM for `[base + disp32]`.
    fn mem(&mut self, reg: u8, base: u8, disp: i32) {
        self.emit(&[0x80 | (reg & 7) << 3 | (base & 7)]);
        if base & 7 == RSP {
            self.emit(&[0x24]);
        }
        self.imm32(disp);
    }

    fn rr(&mut self, w: bool, op: &[u8], reg: u8, rm: u8) {
        self.rex(w, reg, rm, false);
        self.emit(op);
        self.emit(&[0xC0 | (reg & 7) << 3 | (rm & 7)]);
    }

    pub fn push(&mut self, r: u8) {
        if r >= 8 {
            self.emit(&[0x41]);
        }
        self.emit(&[0x50 + (r & 7)]);
    }

    pub fn pop(&mut self, r: u8) {
        if r >= 8 {
            self.emit(&[0x41]);
        }
        self.emit(&[0x58 + (r & 7)]);
    }

    /// `mov dst, src` (64-bit)
    pub fn mov(&mut self, dst: u8, src: u8) {
        self.rr(true, &[0x89], src, dst);
    }

    pub fn mov_imm(&mut self, r: u8, v: i64) {
        if v == 0 {
            self.rr(false, &[0x31], r, r); // xor r32, r32
        } else if v >= 0 && v <= u32::MAX as i64 {
            if r >= 8 {
                self.emit(&[0x41]);
            }
            self.emit(&[0xB8 + (r & 7)]);
            self.imm32(v as u32 as i32);
        } else if v >= i32::MIN as i64 && v <= i32::MAX as i64 {
            self.rex(true, 0, r, false);
            self.emit(&[0xC7, 0xC0 | (r & 7)]);
            self.imm32(v as i32);
        } else {
            self.rex(true, 0, r, false);
            self.emit(&[0xB8 + (r & 7)]);
            self.emit(&v.to_le_bytes());
        }
    }

    /// `lea dst, [base + disp]`
    pub fn lea(&mut self, dst: u8, base: u8, disp: i32) {
        self.rex(true, dst, base, false);
        self.emit(&[0x8D]);
        self.mem(dst, base, disp);
    }

    /// `lea rax, [rip + label + addend]`
    pub fn lea_label(&mut self, dst: u8, label: usize, addend: i64) {
        self.rex(true, dst, 0, false);
        self.emit(&[0x8D, 0x05 | (dst & 7) << 3]);
        self.fixups.push(Fixup {
            at: self.code.len(),
            label,
            addend,
        });
        self.imm32(0);
    }

    /// Loads `size` bytes from `[base + disp]` into `dst`, sign- or
    /// zero-extending to 64 bits.
    pub fn load(&mut self, dst: u8, base: u8, disp: i32, size: usize, signed: bool) {
        match (size, signed) {
            (1, true) => {
                self.rex(true, dst, base, false);
                self.emit(&[0x0F, 0xBE]);
            }
            (1, false) => {
                self.rex(false, dst, base, false);
                self.emit(&[0x0F, 0xB6]);
            }
            (2, true) => {
                self.rex(true, dst, base, false);
                self.emit(&[0x0F, 0xBF]);
            }
            (2, false) => {
                self.rex(false, dst, base, false);
                self.emit(&[0x0F, 0xB7]);
            }
            (4, true) => {
                self.rex(true, dst, base, false);
                self.emit(&[0x63]);
            }
            (4, false) => {
                self.rex(false, dst, base, false);
                self.emit(&[0x8B]);
            }
            _ => {
                self.rex(true, dst, base, false);
                self.emit(&[0x8B]);
            }
        }
        self.mem(dst, base, disp);
    }

    /// Stores the low `size` bytes of `src` to `[base + disp]`.
    pub fn store(&mut self, src: u8, base: u8, disp: i32, size: usize) {
        match size {
            1 => {
                self.rex(false, src, base, src >= 4);
                self.emit(&[0x88]);
            }
            2 => {
                self.emit(&[0x66]);
                self.rex(false, src, base, false);
                self.emit(&[0x89]);
            }
            4 => {
                self.rex(false, src, base, false);
                self.emit(&[0x89]);
            }
            _ => {
                self.rex(true, src, base, false);
                self.emit(&[0x89]);
            }
        }
        self.mem(src, base, disp);
    }

    pub fn add(&mut self, dst: u8, src: u8) {
        self.rr(true, &[0x01], src, dst);
    }

    pub fn sub(&mut self, dst: u8, src: u8) {
        self.rr(true, &[0x29], src, dst);
    }

    pub fn and(&mut self, dst: u8, src: u8) {
        self.rr(true, &[0x21], src, dst);
    }

    pub fn or(&mut self, dst: u8, src: u8) {
        self.rr(true, &[0x09], src, dst);
    }

    pub fn xor(&mut self, dst: u8, src: u8) {
        self.rr(true, &[0x31], src, dst);
    }

    pub fn cmp(&mut self, a: u8, b: u8) {
        self.rr(true, &[0x39], b, a);
    }

    pub fn test(&mut self, a: u8, b: u8) {
        self.rr(true, &[0x85], b, a);
    }

    pub fn imul(&mut self, dst: u8, src: u8) {
        self.rr(true, &[0x0F, 0xAF], dst, src);
    }

    fn group_imm(&mut self, digit: u8, r: u8, v: i32) {
        self.rex(true, 0, r, false);
        self.emit(&[0x81, 0xC0 | digit << 3 | (r & 7)]);
        self.imm32(v);
    }

    pub fn add_imm(&mut self, r: u8, v: i32) {
        if v != 0 {
            self.group_imm(0, r, v);
        }
    }

    pub fn sub_imm(&mut self, r: u8, v: i32) {
        if v != 0 {
            self.group_imm(5, r, v);
        }
    }

    pub fn cmp_imm(&mut self, r: u8, v: i32) {
        self.group_imm(7, r, v);
    }

    /// `sub rsp, imm32` with a patchable immediate; returns its position.
    pub fn sub_rsp_patchable(&mut self) -> usize {
        self.group_imm(5, RSP, 0);
        self.code.len() - 4
    }

    pub fn patch32(&mut self, at: usize, v: i32) {
        self.code[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    pub fn cqo(&mut self) {
        self.emit(&[0x48, 0x99]);
    }

    pub fn idiv(&mut self, r: u8) {
        self.rr(true, &[0xF7], 7, r);
    }

    pub fn div(&mut self, r: u8) {
        self.rr(true, &[0xF7], 6, r);
    }

    pub fn neg(&mut self, r: u8) {
        self.rr(true, &[0xF7], 3, r);
    }

    pub fn not(&mut self, r: u8) {
        self.rr(true, &[0xF7], 2, r);
    }

    pub fn shl_cl(&mut self, r: u8) {
        self.rr(true, &[0xD3], 4, r);
    }

    pub fn shr_cl(&mut self, r: u8) {
        self.rr(true, &[0xD3], 5, r);
    }

    pub fn sar_cl(&mut self, r: u8) {
        self.rr(true, &[0xD3], 7, r);
    }

    /// `setcc r8` (low byte of rax/rcx/rdx only).
    pub fn setcc(&mut self, c: Cond, r: u8) {
        self.emit(&[0x0F, 0x90 + c as u8, 0xC0 | r]);
    }

    /// Sign/zero-extends the low `size` bytes of `r` in place.
    pub fn extend(&mut self, r: u8, size: usize, signed: bool) {
        match (size, signed) {
            (1, true) => self.rr(true, &[0x0F, 0xBE], r, r),
            (1, false) => self.rr(false, &[0x0F, 0xB6], r, r),
            (2, true) => self.rr(true, &[0x0F, 0xBF], r, r),
            (2, false) => self.rr(false, &[0x0F, 0xB7], r, r),
            (4, true) => self.rr(true, &[0x63], r, r),
            (4, false) => self.rr(false, &[0x89], r, r),
            _ => {}
        }
    }

    /// `and al, dl` / `or al, dl`
    pub fn and8(&mut self, dst: u8, src: u8) {
        self.emit(&[0x20, 0xC0 | src << 3 | dst]);
    }

    pub fn or8(&mut self, dst: u8, src: u8) {
        self.emit(&[0x08, 0xC0 | src << 3 | dst]);
    }

    pub fn jmp(&mut self, label: usize) {
        self.emit(&[0xE9]);
        self.rel32(label);
    }

    pub fn jcc(&mut self, c: Cond, label: usize) {
        self.emit(&[0x0F, 0x80 + c as u8]);
        self.rel32(label);
    }

    pub fn call(&mut self, label: usize) {
        self.emit(&[0xE8]);
        self.rel32(label);
    }

    pub fn call_reg(&mut self, r: u8) {
        self.rr(false, &[0xFF], 2, r);
    }

    pub fn jmp_reg(&mut self, r: u8) {
        self.rr(false, &[0xFF], 4, r);
    }

    pub fn ret(&mut self) {
        self.emit(&[0xC3]);
    }

    pub fn leave(&mut self) {
        self.emit(&[0xC9]);
    }

    pub fn syscall(&mut self) {
        self.emit(&[0x0F, 0x05]);
    }

    pub fn hlt(&mut self) {
        self.emit(&[0xF4]);
    }

    pub fn rep_movsb(&mut self) {
        self.emit(&[0xF3, 0xA4]);
    }

    pub fn rep_stosb(&mut self) {
        self.emit(&[0xF3, 0xAA]);
    }

    // ---- SSE (scalar double in xmm0/xmm1) ------------------------------

    /// `movq xmm, r64`
    pub fn movq_to_xmm(&mut self, x: u8, r: u8) {
        self.emit(&[0x66]);
        self.rr(true, &[0x0F, 0x6E], x, r);
    }

    /// `movq r64, xmm`
    pub fn movq_from_xmm(&mut self, r: u8, x: u8) {
        self.emit(&[0x66]);
        self.rr(true, &[0x0F, 0x7E], x, r);
    }

    /// `movd xmm, r32`
    pub fn movd_to_xmm(&mut self, x: u8, r: u8) {
        self.emit(&[0x66]);
        self.rr(false, &[0x0F, 0x6E], x, r);
    }

    /// `movd r32, xmm`
    pub fn movd_from_xmm(&mut self, r: u8, x: u8) {
        self.emit(&[0x66]);
        self.rr(false, &[0x0F, 0x7E], x, r);
    }

    fn sse(&mut self, prefix: u8, op: u8, dst: u8, src: u8) {
        self.emit(&[prefix, 0x0F, op, 0xC0 | dst << 3 | src]);
    }

    pub fn addsd(&mut self, d: u8, s: u8) {
        self.sse(0xF2, 0x58, d, s);
    }

    pub fn subsd(&mut self, d: u8, s: u8) {
        self.sse(0xF2, 0x5C, d, s);
    }

    pub fn mulsd(&mut self, d: u8, s: u8) {
        self.sse(0xF2, 0x59, d, s);
    }

    pub fn divsd(&mut self, d: u8, s: u8) {
        self.sse(0xF2, 0x5E, d, s);
    }

    pub fn sqrtsd(&mut self, d: u8, s: u8) {
        self.sse(0xF2, 0x51, d, s);
    }

    pub fn ucomisd(&mut self, a: u8, b: u8) {
        self.sse(0x66, 0x2E, a, b);
    }

    pub fn xorpd(&mut self, d: u8, s: u8) {
        self.sse(0x66, 0x57, d, s);
    }

    pub fn cvtsd2ss(&mut self, d: u8, s: u8) {
        self.sse(0xF2, 0x5A, d, s);
    }

    pub fn cvtss2sd(&mut self, d: u8, s: u8) {
        self.sse(0xF3, 0x5A, d, s);
    }

    /// `cvtsi2sd xmm, r64`
    pub fn cvtsi2sd(&mut self, x: u8, r: u8) {
        self.emit(&[0xF2]);
        self.rr(true, &[0x0F, 0x2A], x, r);
    }

    /// `cvttsd2si r64, xmm`
    pub fn cvttsd2si(&mut self, r: u8, x: u8) {
        self.emit(&[0xF2]);
        self.rr(true, &[0x0F, 0x2C], r, x);
    }

    /// `btc rax, 63` (flip the sign bit of a double held in rax).
    pub fn flip_sign(&mut self, r: u8) {
        self.rex(true, 0, r, false);
        self.emit(&[0x0F, 0xBA, 0xF8 | (r & 7), 63]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(f: impl FnOnce(&mut Asm)) -> Vec<u8> {
        let mut a = Asm::new(0);
        f(&mut a);
        a.code
    }

    #[test]
    fn encodings() {
        assert_eq!(enc(|a| a.mov(RBP, RSP)), [0x48, 0x89, 0xE5]);
        assert_eq!(enc(|a| a.push(R8)), [0x41, 0x50]);
        assert_eq!(enc(|a| a.add(RAX, RDI)), [0x48, 0x01, 0xF8]);
        assert_eq!(enc(|a| a.cmp(RAX, RDI)), [0x48, 0x39, 0xF8]);
        assert_eq!(enc(|a| a.imul(RAX, RDI)), [0x48, 0x0F, 0xAF, 0xC7]);
        assert_eq!(enc(|a| a.idiv(RDI)), [0x48, 0xF7, 0xFF]);
        assert_eq!(
            enc(|a| a.load(RAX, RBP, -8, 8, true)),
            [0x48, 0x8B, 0x85, 0xF8, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(
            enc(|a| a.load(RAX, RAX, 0, 4, true)),
            [0x48, 0x63, 0x80, 0, 0, 0, 0]
        );
        assert_eq!(enc(|a| a.store(RAX, RDI, 0, 1)), [0x88, 0x87, 0, 0, 0, 0]);
        assert_eq!(
            enc(|a| a.store(RSI, RDI, 0, 1)),
            [0x40, 0x88, 0xB7, 0, 0, 0, 0]
        );
        assert_eq!(
            enc(|a| a.lea(RAX, RSP, 8)),
            [0x48, 0x8D, 0x84, 0x24, 8, 0, 0, 0]
        );
        assert_eq!(
            enc(|a| a.mov_imm(RAX, -1)),
            [0x48, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(enc(|a| a.mov_imm(R10, 5)), [0x41, 0xBA, 5, 0, 0, 0]);
        assert_eq!(enc(|a| a.extend(RAX, 4, true)), [0x48, 0x63, 0xC0]);
        assert_eq!(enc(|a| a.extend(RAX, 1, false)), [0x0F, 0xB6, 0xC0]);
        assert_eq!(
            enc(|a| a.movq_to_xmm(1, RDI)),
            [0x66, 0x48, 0x0F, 0x6E, 0xCF]
        );
        assert_eq!(
            enc(|a| a.movq_from_xmm(RAX, 0)),
            [0x66, 0x48, 0x0F, 0x7E, 0xC0]
        );
        assert_eq!(enc(|a| a.cvttsd2si(RAX, 0)), [0xF2, 0x48, 0x0F, 0x2C, 0xC0]);
        assert_eq!(enc(|a| a.sar_cl(RAX)), [0x48, 0xD3, 0xF8]);
        assert_eq!(enc(|a| a.call_reg(RAX)), [0xFF, 0xD0]);
        assert_eq!(enc(|a| a.jmp_reg(RCX)), [0xFF, 0xE1]);
        assert_eq!(
            enc(|a| a.load(RDI, RSP, 8, 8, false)),
            [0x48, 0x8B, 0xBC, 0x24, 8, 0, 0, 0]
        );
        assert_eq!(enc(|a| a.flip_sign(RAX)), [0x48, 0x0F, 0xBA, 0xF8, 63]);
        assert_eq!(enc(|a| a.sub_imm(RSP, 16)), [0x48, 0x81, 0xEC, 16, 0, 0, 0]);
    }
}
