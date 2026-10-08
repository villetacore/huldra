//! Frame buffer: the Bochs/QEMU "dispi" graphics adapter (QEMU's standard
//! VGA, VirtualBox's VBoxVGA) as `/dev/fb0`, with the Linux fbdev ioctls
//! and `mmap` of the linear frame buffer.
//!
//! Opening `/dev/fb0` switches from VGA text mode to a 32-bit linear mode;
//! when the last user closes it, the saved VGA registers, font and screen
//! text are put back, so the console reappears.
//!
//! `/dev/font` is the 8x16 VGA font read at boot (256 glyphs, 16 bytes
//! each), for graphical programs.

use crate::arch::port::{inb, inw, outb, outw};
use crate::drivers::pci;
use crate::fs::vfs::*;
use crate::mm::phys_to_virt;
use crate::proc::uaccess::{read_user, write_user};
use crate::sync::SpinLock;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::Any;
use huldra_abi::errno::Errno;

const DISPI_INDEX: u16 = 0x1CE;
const DISPI_DATA: u16 = 0x1CF;
const REG_ID: u16 = 0;
const REG_XRES: u16 = 1;
const REG_YRES: u16 = 2;
const REG_BPP: u16 = 3;
const REG_ENABLE: u16 = 4;
const REG_VIRT_WIDTH: u16 = 6;
const REG_VIRT_HEIGHT: u16 = 7;
const REG_X_OFFSET: u16 = 8;
const REG_Y_OFFSET: u16 = 9;
const REG_VIDEO_MEMORY_64K: u16 = 10;
const ENABLED: u16 = 0x01;
const LFB_ENABLED: u16 = 0x40;

const FBIOGET_VSCREENINFO: u32 = 0x4600;
const FBIOPUT_VSCREENINFO: u32 = 0x4601;
const FBIOGET_FSCREENINFO: u32 = 0x4602;

const TEXT_PHYS: u64 = 0xB8000;
const TEXT_BYTES: usize = 80 * 25 * 2;

fn dispi_read(reg: u16) -> u16 {
    unsafe {
        outw(DISPI_INDEX, reg);
        inw(DISPI_DATA)
    }
}

fn dispi_write(reg: u16, v: u16) {
    unsafe {
        outw(DISPI_INDEX, reg);
        outw(DISPI_DATA, v);
    }
}

/// VGA register file of the text mode, saved at boot.
#[derive(Clone)]
struct VgaState {
    misc: u8,
    seq: [u8; 5],
    crtc: [u8; 25],
    gc: [u8; 9],
    ac: [u8; 21],
    dac: Vec<u8>,
}

fn save_vga() -> VgaState {
    let mut s = VgaState { misc: 0, seq: [0; 5], crtc: [0; 25], gc: [0; 9], ac: [0; 21], dac: alloc::vec![0; 768] };
    unsafe {
        s.misc = inb(0x3CC);
        for (i, v) in s.seq.iter_mut().enumerate() {
            outb(0x3C4, i as u8);
            *v = inb(0x3C5);
        }
        for (i, v) in s.crtc.iter_mut().enumerate() {
            outb(0x3D4, i as u8);
            *v = inb(0x3D5);
        }
        for (i, v) in s.gc.iter_mut().enumerate() {
            outb(0x3CE, i as u8);
            *v = inb(0x3CF);
        }
        for (i, v) in s.ac.iter_mut().enumerate() {
            inb(0x3DA);
            outb(0x3C0, i as u8);
            *v = inb(0x3C1);
        }
        inb(0x3DA);
        outb(0x3C0, 0x20);
        outb(0x3C7, 0);
        for v in s.dac.iter_mut() {
            *v = inb(0x3C9);
        }
    }
    s
}

fn restore_vga(s: &VgaState) {
    unsafe {
        outb(0x3C2, s.misc);
        for (i, &v) in s.seq.iter().enumerate() {
            outb(0x3C4, i as u8);
            outb(0x3C5, v);
        }
        // Unlock CRTC registers 0-7 first.
        outb(0x3D4, 0x11);
        outb(0x3D5, s.crtc[0x11] & 0x7F);
        for (i, &v) in s.crtc.iter().enumerate() {
            outb(0x3D4, i as u8);
            outb(0x3D5, if i == 0x11 { v & 0x7F } else { v });
        }
        outb(0x3D4, 0x11);
        outb(0x3D5, s.crtc[0x11]);
        for (i, &v) in s.gc.iter().enumerate() {
            outb(0x3CE, i as u8);
            outb(0x3CF, v);
        }
        for (i, &v) in s.ac.iter().enumerate() {
            inb(0x3DA);
            outb(0x3C0, i as u8);
            outb(0x3C0, v);
        }
        inb(0x3DA);
        outb(0x3C0, 0x20);
        outb(0x3C8, 0);
        for &v in &s.dac {
            outb(0x3C9, v);
        }
    }
}

/// Maps VGA plane 2 (the font) at 0xA0000, runs `f`, restores text mode.
fn with_font_plane(s: &VgaState, f: impl FnOnce(*mut u8)) {
    unsafe {
        outb(0x3C4, 2);
        outb(0x3C5, 4); // write plane 2
        outb(0x3C4, 4);
        outb(0x3C5, 7); // sequential addressing
        outb(0x3CE, 4);
        outb(0x3CF, 2); // read plane 2
        outb(0x3CE, 5);
        outb(0x3CF, 0);
        outb(0x3CE, 6);
        outb(0x3CF, 0x04); // 64 KiB at 0xA0000
        f(phys_to_virt(0xA0000) as *mut u8);
        outb(0x3C4, 2);
        outb(0x3C5, s.seq[2]);
        outb(0x3C4, 4);
        outb(0x3C5, s.seq[4]);
        outb(0x3CE, 4);
        outb(0x3CF, s.gc[4]);
        outb(0x3CE, 5);
        outb(0x3CF, s.gc[5]);
        outb(0x3CE, 6);
        outb(0x3CF, s.gc[6]);
    }
}

struct Fb {
    lfb: u64,
    vram: u64,
    width: u32,
    height: u32,
    users: usize,
    vga: Option<VgaState>,
    font: Vec<u8>,
    text: Vec<u8>,
}

static FB: SpinLock<Option<Fb>> = SpinLock::new(None);

impl Fb {
    fn set_mode(&mut self, w: u32, h: u32) -> KResult<()> {
        if w < 320 || h < 200 || w > 4096 || h > 4096 || (w as u64 * h as u64 * 4) > self.vram {
            return Err(Errno::EINVAL);
        }
        dispi_write(REG_ENABLE, 0);
        dispi_write(REG_XRES, w as u16);
        dispi_write(REG_YRES, h as u16);
        dispi_write(REG_BPP, 32);
        dispi_write(REG_VIRT_WIDTH, w as u16);
        dispi_write(REG_VIRT_HEIGHT, h as u16);
        dispi_write(REG_X_OFFSET, 0);
        dispi_write(REG_Y_OFFSET, 0);
        dispi_write(REG_ENABLE, ENABLED | LFB_ENABLED);
        self.width = w;
        self.height = h;
        Ok(())
    }

    fn enter_graphics(&mut self) -> KResult<()> {
        let text = unsafe { core::slice::from_raw_parts(phys_to_virt(TEXT_PHYS) as *const u8, TEXT_BYTES) };
        self.text = text.to_vec();
        let (w, h) = (self.width, self.height);
        self.set_mode(w, h)?;
        // Start from black.
        unsafe { core::ptr::write_bytes(phys_to_virt(self.lfb) as *mut u8, 0, (w * h * 4) as usize) };
        Ok(())
    }

    fn leave_graphics(&mut self) {
        dispi_write(REG_ENABLE, 0);
        if let Some(vga) = &self.vga {
            restore_vga(vga);
            let font = &self.font;
            with_font_plane(vga, |plane| {
                for c in 0..256 {
                    for row in 0..32 {
                        let v = if row < 16 { font[c * 16 + row] } else { 0 };
                        unsafe { core::ptr::write_volatile(plane.add(c * 32 + row), v) };
                    }
                }
            });
            let dst = phys_to_virt(TEXT_PHYS) as *mut u8;
            for (i, &b) in self.text.iter().enumerate() {
                unsafe { core::ptr::write_volatile(dst.add(i), b) };
            }
        }
    }
}

struct FbDevice {
    ino: u64,
}

fn var_info(fb: &Fb) -> [u32; 40] {
    let mut v = [0u32; 40];
    v[0] = fb.width;
    v[1] = fb.height;
    v[2] = fb.width;
    v[3] = fb.height;
    v[6] = 32;
    // red, green, blue, transp: offset, length, msb_right
    v[8..20].copy_from_slice(&[16, 8, 0, 8, 8, 0, 0, 8, 0, 24, 8, 0]);
    v
}

fn fix_info(fb: &Fb) -> [u8; 80] {
    let mut f = [0u8; 80];
    f[..12].copy_from_slice(b"bochs-dispi\0");
    f[16..24].copy_from_slice(&fb.lfb.to_le_bytes());
    f[24..28].copy_from_slice(&(fb.vram as u32).to_le_bytes());
    f[36..40].copy_from_slice(&2u32.to_le_bytes()); // FB_VISUAL_TRUECOLOR
    f[48..52].copy_from_slice(&(fb.width * 4).to_le_bytes());
    f
}

impl Inode for FbDevice {
    fn metadata(&self) -> Metadata {
        let mut m = Metadata::new(0, self.ino, FileType::CharDevice, 0o660);
        m.rdev = makedev(29, 0);
        if let Some(fb) = FB.lock().as_ref() {
            m.size = fb.vram;
        }
        m
    }

    fn open(&self, _flags: u32) -> KResult<()> {
        let mut g = FB.lock();
        let fb = g.as_mut().ok_or(Errno::ENODEV)?;
        if fb.users == 0 {
            fb.enter_graphics()?;
        }
        fb.users += 1;
        Ok(())
    }

    fn release(&self, _flags: u32) {
        let mut g = FB.lock();
        if let Some(fb) = g.as_mut() {
            fb.users -= 1;
            if fb.users == 0 {
                fb.leave_graphics();
            }
        }
    }

    fn ioctl(&self, cmd: u32, arg: u64) -> KResult<u64> {
        let mut g = FB.lock();
        let fb = g.as_mut().ok_or(Errno::ENODEV)?;
        match cmd {
            FBIOGET_VSCREENINFO => {
                write_user(arg, &var_info(fb))?;
                Ok(0)
            }
            FBIOGET_FSCREENINFO => {
                write_user(arg, &fix_info(fb))?;
                Ok(0)
            }
            FBIOPUT_VSCREENINFO => {
                let v: [u32; 40] = read_user(arg)?;
                if v[6] != 32 && v[6] != 0 {
                    return Err(Errno::EINVAL);
                }
                fb.set_mode(v[0], v[1])?;
                write_user(arg, &var_info(fb))?;
                Ok(0)
            }
            _ => Err(Errno::ENOTTY),
        }
    }

    fn mmap_phys(&self) -> Option<(u64, u64)> {
        FB.lock().as_ref().map(|fb| (fb.lfb, fb.vram))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct FontDevice {
    ino: u64,
}

impl Inode for FontDevice {
    fn metadata(&self) -> Metadata {
        // A regular file, so reads advance through it.
        let mut m = Metadata::new(0, self.ino, FileType::Regular, 0o444);
        m.size = 4096;
        m
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> KResult<usize> {
        let g = FB.lock();
        let font = g.as_ref().map(|f| f.font.as_slice()).unwrap_or(&[]);
        let start = (offset as usize).min(font.len());
        let n = buf.len().min(font.len() - start);
        buf[..n].copy_from_slice(&font[start..start + n]);
        Ok(n)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Detects the adapter and saves the text mode (call while it is shown).
pub fn init() {
    let id = dispi_read(REG_ID);
    if !(0xB0C0..=0xB0C5).contains(&id) {
        return;
    }
    let lfb = pci::devices()
        .into_iter()
        .find(|d| (d.vendor == 0x1234 && d.device == 0x1111) || (d.vendor == 0x80EE && d.device == 0xBEEF))
        .map(|d| d.bar_mem(0))
        .unwrap_or(0xE000_0000);
    let vram = (dispi_read(REG_VIDEO_MEMORY_64K) as u64).max(64) * 65536;
    let vga = save_vga();
    let mut font = alloc::vec![0u8; 4096];
    with_font_plane(&vga, |plane| {
        for c in 0..256 {
            for row in 0..16 {
                font[c * 16 + row] = unsafe { core::ptr::read_volatile(plane.add(c * 32 + row)) };
            }
        }
    });
    *FB.lock() = Some(Fb { lfb, vram, width: 1024, height: 768, users: 0, vga: Some(vga), font, text: Vec::new() });
    crate::fs::devfs::register("fb0", Arc::new(FbDevice { ino: crate::fs::devfs::dev_ino() }));
    crate::fs::devfs::register("font", Arc::new(FontDevice { ino: crate::fs::devfs::dev_ino() }));
    kinfo!("fb: Bochs dispi {:#x} at {:#x}, {} MiB video memory", id, lfb, vram >> 20);
}
