//! Raw input for graphical programs.
//!
//! `/dev/kbd`: while a program has it open, key presses and releases go to
//! it instead of the terminal, as 4-byte events `[code lo, code hi,
//! pressed, 0]` where `code` is the set-1 scancode plus 0x100 for E0-prefixed
//! keys. Ctrl+Alt+Backspace sends SIGTERM to the holder (an escape hatch).
//!
//! `/dev/mouse`: 8-byte events `[kind, buttons, wheel, 0, x lo, x hi, y lo,
//! y hi]`. kind 0 is relative motion (x/y signed, y up), kind 1 absolute
//! (x/y 0..65535 across the screen) when the VMware/QEMU absolute pointer
//! (vmmouse) is available, so the host cursor needs no grab.

use crate::arch::port::{inb, outb};
use crate::fs::vfs::*;
use crate::sync::SpinLock;
use crate::task::wait::WaitQueue;
use crate::task::Pid;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::any::Any;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use huldra_abi::errno::Errno;

const QUEUE: usize = 512;

struct Queue {
    events: SpinLock<VecDeque<[u8; 8]>>,
    readers: WaitQueue,
    size: usize,
}

impl Queue {
    const fn new(size: usize) -> Queue {
        Queue { events: SpinLock::new(VecDeque::new()), readers: WaitQueue::new(), size }
    }

    fn push(&self, e: [u8; 8]) {
        {
            let mut q = self.events.lock();
            if q.len() >= QUEUE {
                q.pop_front();
            }
            q.push_back(e);
        }
        self.readers.wake_all();
    }

    fn read(&self, buf: &mut [u8]) -> KResult<usize> {
        if buf.len() < self.size {
            return Err(Errno::EINVAL);
        }
        self.readers.wait_until(|| {
            let mut q = self.events.lock();
            if q.is_empty() {
                return None;
            }
            let mut n = 0;
            while n + self.size <= buf.len() {
                let Some(e) = q.pop_front() else { break };
                buf[n..n + self.size].copy_from_slice(&e[..self.size]);
                n += self.size;
            }
            Some(n)
        })
    }
}

static KBD: Queue = Queue::new(4);
static MOUSE: Queue = Queue::new(8);
static KBD_GRABBED: AtomicBool = AtomicBool::new(false);
static KBD_OWNER: AtomicU32 = AtomicU32::new(0);
static MODS: SpinLock<(bool, bool)> = SpinLock::new((false, false)); // ctrl, alt

/// Called by the keyboard driver for every key; true if the event was
/// taken by a `/dev/kbd` reader (the terminal must not see it).
pub fn keyboard_event(code: u8, extended: bool, pressed: bool) -> bool {
    if !KBD_GRABBED.load(Ordering::Acquire) {
        return false;
    }
    let mut m = MODS.lock();
    match code {
        0x1D => m.0 = pressed,
        0x38 => m.1 = pressed,
        0x0E if pressed && m.0 && m.1 => {
            let pid = KBD_OWNER.load(Ordering::Relaxed) as Pid;
            if pid != 0 {
                if let Some(t) = crate::task::lookup(pid) {
                    crate::proc::signal::send(&t, huldra_abi::signal::SIGTERM);
                }
            }
        }
        _ => {}
    }
    drop(m);
    let full = code as u16 | if extended { 0x100 } else { 0 };
    KBD.push([full as u8, (full >> 8) as u8, pressed as u8, 0, 0, 0, 0, 0]);
    true
}

struct KbdDevice {
    ino: u64,
}

impl Inode for KbdDevice {
    fn metadata(&self) -> Metadata {
        let mut m = Metadata::new(0, self.ino, FileType::CharDevice, 0o600);
        m.rdev = makedev(13, 1);
        m
    }

    fn open(&self, _flags: u32) -> KResult<()> {
        if KBD_GRABBED.swap(true, Ordering::AcqRel) {
            return Err(Errno::EBUSY);
        }
        KBD_OWNER.store(crate::task::sched::current_pid() as u32, Ordering::Relaxed);
        KBD.events.lock().clear();
        *MODS.lock() = (false, false);
        Ok(())
    }

    fn release(&self, _flags: u32) {
        KBD_GRABBED.store(false, Ordering::Release);
        KBD_OWNER.store(0, Ordering::Relaxed);
    }

    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> KResult<usize> {
        KBD.read(buf)
    }

    fn poll(&self) -> PollState {
        PollState { readable: !KBD.events.lock().is_empty(), writable: false, hangup: false }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct MouseDevice {
    ino: u64,
}

impl Inode for MouseDevice {
    fn metadata(&self) -> Metadata {
        let mut m = Metadata::new(0, self.ino, FileType::CharDevice, 0o600);
        m.rdev = makedev(13, 63);
        m
    }

    fn open(&self, _flags: u32) -> KResult<()> {
        MOUSE.events.lock().clear();
        Ok(())
    }

    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> KResult<usize> {
        MOUSE.read(buf)
    }

    fn poll(&self) -> PollState {
        PollState { readable: !MOUSE.events.lock().is_empty(), writable: false, hangup: false }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ---- PS/2 mouse ---------------------------------------------------------------

fn wait_write() {
    for _ in 0..100_000 {
        if unsafe { inb(0x64) } & 2 == 0 {
            return;
        }
    }
}

fn wait_read() -> bool {
    for _ in 0..100_000 {
        if unsafe { inb(0x64) } & 1 != 0 {
            return true;
        }
    }
    false
}

fn read_data() -> Option<u8> {
    wait_read().then(|| unsafe { inb(0x60) })
}

fn mouse_write(b: u8) -> Option<u8> {
    wait_write();
    unsafe { outb(0x64, 0xD4) };
    wait_write();
    unsafe { outb(0x60, b) };
    read_data() // ACK (0xFA)
}

struct Ps2 {
    packet: [u8; 4],
    len: usize,
    size: usize,
}

static PS2: SpinLock<Ps2> = SpinLock::new(Ps2 { packet: [0; 4], len: 0, size: 3 });
static VMMOUSE: AtomicBool = AtomicBool::new(false);

// ---- VMware absolute pointer ("vmmouse", also in QEMU) ---------------------------

const VMWARE_MAGIC: u32 = 0x564D_5868;
const VMWARE_PORT: u16 = 0x5658;
const CMD_GETVERSION: u32 = 10;
const CMD_ABSPOINTER_DATA: u32 = 39;
const CMD_ABSPOINTER_STATUS: u32 = 40;
const CMD_ABSPOINTER_COMMAND: u32 = 41;

/// The VMware backdoor: returns (eax, ebx, ecx, edx).
fn vmware(cmd: u32, arg: u32) -> (u32, u32, u32, u32) {
    let (a, b, c, d): (u32, u32, u32, u32);
    // rbx cannot be an asm operand: save it on the stack around the call.
    // The hypervisor may also write esi and edi (QEMU's vmmouse data
    // command fills six registers), zero-extending them: they must be
    // declared clobbered, or a pointer kept there comes back truncated.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov ebx, {arg:e}",
            "in eax, dx",
            "mov {arg:e}, ebx",
            "pop rbx",
            arg = inout(reg) arg => b,
            inout("eax") VMWARE_MAGIC => a,
            inout("ecx") cmd => c,
            inout("edx") VMWARE_PORT as u32 => d,
            lateout("rsi") _,
            lateout("rdi") _,
        );
    }
    (a, b, c, d)
}

fn vmmouse_init() -> bool {
    let (a, b, _, _) = vmware(CMD_GETVERSION, 0);
    kdebug!("vmmouse: version {:#x} {:#x}", a, b);
    if b != VMWARE_MAGIC || a == 0xFFFF_FFFF {
        return false;
    }
    vmware(CMD_ABSPOINTER_COMMAND, 0x4541_4552); // READ_ID
    let (status, _, _, _) = vmware(CMD_ABSPOINTER_STATUS, 0);
    kdebug!("vmmouse: status after READ_ID {:#x}", status);
    if status & 0xFFFF == 0 {
        return false;
    }
    let (id, _, _, _) = vmware(CMD_ABSPOINTER_DATA, 1);
    kdebug!("vmmouse: status {:#x} id {:#x}", status, id);
    if id != 0x3442_554A {
        return false;
    }
    vmware(CMD_ABSPOINTER_COMMAND, 0x5342_4152); // REQUEST_ABSOLUTE
    true
}

fn vmmouse_poll() {
    loop {
        let (status, _, _, _) = vmware(CMD_ABSPOINTER_STATUS, 0);
        if status == 0xFFFF_0000 {
            // Error: re-enable.
            vmmouse_init();
            return;
        }
        if status & 0xFFFF < 4 {
            return;
        }
        let (flags, x, y, z) = vmware(CMD_ABSPOINTER_DATA, 4);
        // VMware button bits: 0x20 left, 0x10 right, 0x08 middle.
        let mut buttons = 0u8;
        if flags & 0x20 != 0 {
            buttons |= 1;
        }
        if flags & 0x10 != 0 {
            buttons |= 2;
        }
        if flags & 0x08 != 0 {
            buttons |= 4;
        }
        let wheel = (z as i32 as i8).wrapping_neg();
        MOUSE.push([1, buttons, wheel as u8, 0, x as u8, (x >> 8) as u8, y as u8, (y >> 8) as u8]);
    }
}

/// IRQ12.
pub fn handle_mouse_irq() {
    let b = unsafe { inb(0x60) };
    if VMMOUSE.load(Ordering::Relaxed) {
        vmmouse_poll();
        return;
    }
    let mut p = PS2.lock();
    if p.len == 0 && b & 0x08 == 0 {
        return; // out of sync
    }
    let i = p.len;
    p.packet[i] = b;
    p.len += 1;
    if p.len < p.size {
        return;
    }
    p.len = 0;
    let [flags, dx, dy, wz] = p.packet;
    if flags & 0xC0 != 0 {
        return; // overflow
    }
    let dx = dx as i16 - if flags & 0x10 != 0 { 256 } else { 0 };
    let dy = dy as i16 - if flags & 0x20 != 0 { 256 } else { 0 };
    let wheel = if p.size == 4 { (wz as i8).wrapping_neg() } else { 0 };
    drop(p);
    MOUSE.push([0, flags & 7, wheel as u8, 0, dx as u8, (dx >> 8) as u8, dy as u8, (dy >> 8) as u8]);
}

fn ps2_mouse_init() -> bool {
    wait_write();
    unsafe { outb(0x64, 0xA8) }; // enable the aux port
    wait_write();
    unsafe { outb(0x64, 0x20) };
    let Some(config) = read_data() else { return false };
    wait_write();
    unsafe { outb(0x64, 0x60) };
    wait_write();
    unsafe { outb(0x60, (config | 0x02) & !0x20) }; // IRQ12 on, aux clock on
    if mouse_write(0xF6) != Some(0xFA) {
        return false;
    }
    // IntelliMouse: sample rates 200, 100, 80 enable the wheel (id 3).
    for rate in [200, 100, 80] {
        mouse_write(0xF3);
        mouse_write(rate);
    }
    mouse_write(0xF2);
    let id = read_data().unwrap_or(0);
    PS2.lock().size = if id == 3 { 4 } else { 3 };
    mouse_write(0xF4);
    true
}

pub fn init() {
    let ps2 = ps2_mouse_init();
    let vm = ps2 && vmmouse_init();
    VMMOUSE.store(vm, Ordering::Relaxed);
    if ps2 {
        crate::arch::irq::register(12, "mouse", handle_mouse_irq);
        kinfo!("mouse: PS/2{}", if vm { " with absolute pointer (vmmouse)" } else { "" });
    }
    crate::fs::devfs::register("kbd", Arc::new(KbdDevice { ino: crate::fs::devfs::dev_ino() }));
    crate::fs::devfs::register("mouse", Arc::new(MouseDevice { ino: crate::fs::devfs::dev_ino() }));
}
