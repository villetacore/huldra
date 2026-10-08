//! x87 FPU / SSE state. The kernel itself never uses these registers (it is
//! built soft-float), so user state stays live in the CPU while in the
//! kernel and only has to be saved and restored on task switches.

use core::arch::asm;

/// FXSAVE area: 512 bytes, 16-byte aligned.
#[derive(Clone, Copy)]
#[repr(C, align(16))]
pub struct FpuState {
    bytes: [u8; 512],
}

impl FpuState {
    /// The state after `fninit` with SSE exceptions masked (MXCSR 0x1F80).
    pub fn initial() -> FpuState {
        let mut s = FpuState { bytes: [0; 512] };
        s.bytes[0..2].copy_from_slice(&0x037Fu16.to_le_bytes()); // FCW
        s.bytes[24..28].copy_from_slice(&0x1F80u32.to_le_bytes()); // MXCSR
        s.bytes[28..32].copy_from_slice(&0xFFFFu32.to_le_bytes()); // MXCSR mask
        s
    }

    /// Saves the CPU's current FPU/SSE registers into `self`.
    pub fn save(&mut self) {
        unsafe { asm!("fxsave64 [{}]", in(reg) self.bytes.as_mut_ptr(), options(nostack, preserves_flags)) }
    }

    /// Loads `self` into the CPU.
    pub fn restore(&self) {
        unsafe { asm!("fxrstor64 [{}]", in(reg) self.bytes.as_ptr(), options(nostack, preserves_flags)) }
    }
}

/// Enables SSE: CR0.MP set, CR0.EM/TS clear, CR4.OSFXSR and OSXMMEXCPT set.
pub fn init() {
    unsafe {
        let mut cr0: u64;
        asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack));
        cr0 &= !(1 << 2); // EM
        cr0 &= !(1 << 3); // TS
        cr0 |= 1 << 1; // MP
        cr0 |= 1 << 5; // NE: native FPU error reporting
        asm!("mov cr0, {}", in(reg) cr0, options(nostack));
        let cr4 = super::cpu::read_cr4() | (1 << 9) | (1 << 10);
        super::cpu::write_cr4(cr4);
        asm!("fninit", options(nomem, nostack));
    }
    FpuState::initial().restore();
}
