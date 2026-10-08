//! Intel 8254x (e1000) Ethernet driver, as emulated by QEMU, VirtualBox
//! and VMware: descriptor rings in DMA memory, one interrupt line.

use crate::drivers::pci;
use crate::mm::{frame, phys_to_virt};
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicU64, Ordering};
use huldra_net::Mac;

const CTRL: u32 = 0x0000;
const STATUS: u32 = 0x0008;
const EERD: u32 = 0x0014;
const ICR: u32 = 0x00C0;
const IMS: u32 = 0x00D0;
const IMC: u32 = 0x00D8;
const RCTL: u32 = 0x0100;
const TCTL: u32 = 0x0400;
const TIPG: u32 = 0x0410;
const RDBAL: u32 = 0x2800;
const RDBAH: u32 = 0x2804;
const RDLEN: u32 = 0x2808;
const RDH: u32 = 0x2810;
const RDT: u32 = 0x2818;
const TDBAL: u32 = 0x3800;
const TDBAH: u32 = 0x3804;
const TDLEN: u32 = 0x3808;
const TDH: u32 = 0x3810;
const TDT: u32 = 0x3818;
const MTA: u32 = 0x5200;
const RAL: u32 = 0x5400;
const RAH: u32 = 0x5404;

const CTRL_SLU: u32 = 1 << 6;
const CTRL_ASDE: u32 = 1 << 5;
const CTRL_RST: u32 = 1 << 26;
const RCTL_EN: u32 = 1 << 1;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;
const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const CMD_EOP: u8 = 1 << 0;
const CMD_IFCS: u8 = 1 << 1;
const CMD_RS: u8 = 1 << 3;
const DD: u8 = 1 << 0;
const INT_RXT0: u32 = 1 << 7;
const INT_RXO: u32 = 1 << 6;
const INT_RXDMT0: u32 = 1 << 4;
const INT_LSC: u32 = 1 << 2;

const RING: usize = 32;
const BUF: usize = 2048;

/// Device IDs of 8254x variants with the classic register layout.
const IDS: &[u16] = &[0x100E, 0x100F, 0x1004, 0x1011, 0x1026, 0x1027, 0x1028, 0x1075, 0x1076, 0x1077, 0x1079, 0x107C, 0x10D3];

#[repr(C)]
#[derive(Clone, Copy)]
struct RxDesc {
    addr: u64,
    len: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TxDesc {
    addr: u64,
    len: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

pub struct E1000 {
    mmio: u64,
    rx_ring: *mut RxDesc,
    tx_ring: *mut TxDesc,
    rx_bufs: u64,
    tx_bufs: u64,
    rx_next: usize,
    tx_next: usize,
    pub mac: Mac,
    pub irq: u8,
}

unsafe impl Send for E1000 {}

/// MMIO base for the interrupt handler (which must not take locks).
static IRQ_MMIO: AtomicU64 = AtomicU64::new(0);

impl E1000 {
    fn read(&self, reg: u32) -> u32 {
        unsafe { read_volatile((self.mmio + reg as u64) as *const u32) }
    }

    fn write(&self, reg: u32, v: u32) {
        unsafe { write_volatile((self.mmio + reg as u64) as *mut u32, v) }
    }

    pub fn probe() -> Option<E1000> {
        let dev = pci::devices().into_iter().find(|d| d.vendor == 0x8086 && IDS.contains(&d.device))?;
        dev.enable();
        let mmio = phys_to_virt(dev.bar_mem(0));
        let mut nic = E1000 {
            mmio,
            rx_ring: core::ptr::null_mut(),
            tx_ring: core::ptr::null_mut(),
            rx_bufs: 0,
            tx_bufs: 0,
            rx_next: 0,
            tx_next: 0,
            mac: Mac::default(),
            irq: dev.irq_line,
        };
        nic.reset();
        nic.mac = nic.read_mac();
        nic.setup_rings()?;
        kinfo!("e1000: {:04x} at {:02x}:{:02x}.{} irq {}, MAC {}", dev.device, dev.bus, dev.slot, dev.function, dev.irq_line, nic.mac);
        Some(nic)
    }

    fn reset(&mut self) {
        self.write(IMC, 0xFFFF_FFFF);
        self.write(CTRL, self.read(CTRL) | CTRL_RST);
        for _ in 0..100_000 {
            if self.read(CTRL) & CTRL_RST == 0 {
                break;
            }
            core::hint::spin_loop();
        }
        self.write(IMC, 0xFFFF_FFFF);
        self.write(CTRL, self.read(CTRL) | CTRL_SLU | CTRL_ASDE);
        for i in 0..128 {
            self.write(MTA + 4 * i, 0);
        }
        let _ = self.read(ICR);
    }

    fn read_mac(&self) -> Mac {
        let lo = self.read(RAL);
        let hi = self.read(RAH);
        if lo != 0 {
            let b = lo.to_le_bytes();
            return Mac([b[0], b[1], b[2], b[3], hi as u8, (hi >> 8) as u8]);
        }
        // Fall back to the EEPROM.
        let mut m = [0u8; 6];
        for word in 0..3u32 {
            self.write(EERD, 1 | (word << 8));
            let mut v = 0;
            for _ in 0..100_000 {
                v = self.read(EERD);
                if v & (1 << 4) != 0 {
                    break;
                }
            }
            let data = (v >> 16) as u16;
            m[word as usize * 2] = data as u8;
            m[word as usize * 2 + 1] = (data >> 8) as u8;
        }
        Mac(m)
    }

    fn setup_rings(&mut self) -> Option<()> {
        // One page per ring, 16 pages (32 x 2 KiB) of buffers per direction.
        let rx_ring = frame::alloc_zeroed()?;
        let tx_ring = frame::alloc_zeroed()?;
        self.rx_bufs = frame::alloc_pages(4)?;
        self.tx_bufs = frame::alloc_pages(4)?;
        self.rx_ring = phys_to_virt(rx_ring) as *mut RxDesc;
        self.tx_ring = phys_to_virt(tx_ring) as *mut TxDesc;
        for i in 0..RING {
            unsafe {
                write_volatile(self.rx_ring.add(i), RxDesc { addr: self.rx_bufs + (i * BUF) as u64, len: 0, checksum: 0, status: 0, errors: 0, special: 0 });
                // Start with every transmit slot "done" so it is free.
                write_volatile(self.tx_ring.add(i), TxDesc { addr: self.tx_bufs + (i * BUF) as u64, len: 0, cso: 0, cmd: 0, status: DD, css: 0, special: 0 });
            }
        }
        self.write(RDBAL, rx_ring as u32);
        self.write(RDBAH, (rx_ring >> 32) as u32);
        self.write(RDLEN, (RING * 16) as u32);
        self.write(RDH, 0);
        self.write(RDT, RING as u32 - 1);
        self.write(RCTL, RCTL_EN | RCTL_BAM | RCTL_SECRC);
        self.write(TDBAL, tx_ring as u32);
        self.write(TDBAH, (tx_ring >> 32) as u32);
        self.write(TDLEN, (RING * 16) as u32);
        self.write(TDH, 0);
        self.write(TDT, 0);
        self.write(TCTL, TCTL_EN | TCTL_PSP | (0x10 << 4) | (0x40 << 12));
        self.write(TIPG, 0x0060_200A);
        Some(())
    }

    /// Unmasks interrupts once the handler is installed.
    pub fn enable_interrupts(&self) {
        IRQ_MMIO.store(self.mmio, Ordering::Release);
        self.write(IMS, INT_RXT0 | INT_RXO | INT_RXDMT0 | INT_LSC);
        let _ = self.read(ICR);
    }

    pub fn link_up(&self) -> bool {
        self.read(STATUS) & 2 != 0
    }

    pub fn transmit(&mut self, frame: &[u8]) -> bool {
        let i = self.tx_next;
        let desc = unsafe { read_volatile(self.tx_ring.add(i)) };
        if desc.status & DD == 0 || frame.len() > BUF {
            return false; // ring full
        }
        let buf = phys_to_virt(self.tx_bufs + (i * BUF) as u64) as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), buf, frame.len());
            write_volatile(self.tx_ring.add(i), TxDesc { addr: self.tx_bufs + (i * BUF) as u64, len: frame.len() as u16, cso: 0, cmd: CMD_EOP | CMD_IFCS | CMD_RS, status: 0, css: 0, special: 0 });
        }
        fence(Ordering::SeqCst);
        self.tx_next = (i + 1) % RING;
        self.write(TDT, self.tx_next as u32);
        true
    }

    /// Frames received since the last call.
    pub fn receive(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let i = self.rx_next;
            let desc = unsafe { read_volatile(self.rx_ring.add(i)) };
            if desc.status & DD == 0 {
                break;
            }
            fence(Ordering::SeqCst);
            // Frames that span buffers (EOP clear) or have errors are dropped.
            if desc.status & 0x2 != 0 && desc.errors == 0 {
                let buf = phys_to_virt(self.rx_bufs + (i * BUF) as u64) as *const u8;
                let len = (desc.len as usize).min(BUF);
                out.push(unsafe { core::slice::from_raw_parts(buf, len) }.to_vec());
            }
            unsafe {
                let mut d = desc;
                d.status = 0;
                write_volatile(self.rx_ring.add(i), d);
            }
            fence(Ordering::SeqCst);
            self.write(RDT, i as u32);
            self.rx_next = (i + 1) % RING;
        }
        out
    }
}

/// Interrupt handler: acknowledges the device and wakes the network thread.
pub fn handle_irq() {
    let mmio = IRQ_MMIO.load(Ordering::Acquire);
    if mmio != 0 {
        let cause = unsafe { read_volatile((mmio + ICR as u64) as *const u32) };
        if cause != 0 {
            super::wake_netd();
        }
    }
}
