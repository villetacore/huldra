//! ATA (IDE) disks in PIO mode, LBA28/LBA48, polled.

use super::block::{self, BlockDevice, SECTOR_SIZE};
use crate::arch::port::{inb, outb};
use crate::fs::KResult;
use crate::sync::Mutex;
use alloc::string::String;
use alloc::sync::Arc;
use core::arch::asm;
use huldra_abi::errno::Errno;

const STATUS_ERR: u8 = 0x01;
const STATUS_DRQ: u8 = 0x08;
const STATUS_DF: u8 = 0x20;
const STATUS_BSY: u8 = 0x80;

const CMD_READ_PIO: u8 = 0x20;
const CMD_READ_PIO_EXT: u8 = 0x24;
const CMD_WRITE_PIO: u8 = 0x30;
const CMD_WRITE_PIO_EXT: u8 = 0x34;
const CMD_CACHE_FLUSH: u8 = 0xE7;
const CMD_IDENTIFY: u8 = 0xEC;

struct Channel {
    io: u16,
    control: u16,
}

pub struct AtaDisk {
    name: String,
    channel: Arc<Mutex<Channel>>,
    slave: bool,
    sectors: u64,
    lba48: bool,
}

unsafe fn insw(port: u16, buf: &mut [u8]) {
    for chunk in buf.chunks_exact_mut(2) {
        let w: u16;
        asm!("in ax, dx", out("ax") w, in("dx") port, options(nomem, nostack, preserves_flags));
        chunk.copy_from_slice(&w.to_le_bytes());
    }
}

unsafe fn outsw(port: u16, buf: &[u8]) {
    for chunk in buf.chunks_exact(2) {
        let w = u16::from_le_bytes([chunk[0], chunk[1]]);
        asm!("out dx, ax", in("dx") port, in("ax") w, options(nomem, nostack, preserves_flags));
    }
}

impl Channel {
    fn status(&self) -> u8 {
        unsafe { inb(self.io + 7) }
    }

    /// 400 ns delay: read the alternate status register four times.
    fn delay(&self) {
        for _ in 0..4 {
            unsafe { inb(self.control) };
        }
    }

    fn wait_not_busy(&self) -> KResult<u8> {
        for _ in 0..10_000_000 {
            let s = self.status();
            if s & STATUS_BSY == 0 {
                return Ok(s);
            }
            core::hint::spin_loop();
        }
        Err(Errno::EIO)
    }

    fn wait_data(&self) -> KResult<()> {
        let s = self.wait_not_busy()?;
        if s & (STATUS_ERR | STATUS_DF) != 0 {
            return Err(Errno::EIO);
        }
        for _ in 0..1_000_000 {
            let s = self.status();
            if s & (STATUS_ERR | STATUS_DF) != 0 {
                return Err(Errno::EIO);
            }
            if s & STATUS_DRQ != 0 {
                return Ok(());
            }
        }
        Err(Errno::EIO)
    }

    fn select(&self, slave: bool, lba_bits: u8) {
        unsafe { outb(self.io + 6, 0xE0 | (slave as u8) << 4 | lba_bits) };
        self.delay();
    }

    fn setup(&self, slave: bool, lba: u64, count: u16, lba48: bool) {
        unsafe {
            if lba48 {
                self.select(slave, 0);
                outb(self.io + 2, (count >> 8) as u8);
                outb(self.io + 3, (lba >> 24) as u8);
                outb(self.io + 4, (lba >> 32) as u8);
                outb(self.io + 5, (lba >> 40) as u8);
            } else {
                self.select(slave, ((lba >> 24) & 0x0F) as u8);
            }
            outb(self.io + 2, count as u8);
            outb(self.io + 3, lba as u8);
            outb(self.io + 4, (lba >> 8) as u8);
            outb(self.io + 5, (lba >> 16) as u8);
        }
    }

    /// IDENTIFY DEVICE; returns (sectors, lba48) for ATA disks.
    fn identify(&self, slave: bool) -> Option<(u64, bool)> {
        unsafe {
            self.select(slave, 0);
            outb(self.io + 2, 0);
            outb(self.io + 3, 0);
            outb(self.io + 4, 0);
            outb(self.io + 5, 0);
            outb(self.io + 7, CMD_IDENTIFY);
            if self.status() == 0 || self.status() == 0xFF {
                return None; // no device
            }
            self.wait_not_busy().ok()?;
            if inb(self.io + 4) != 0 || inb(self.io + 5) != 0 {
                return None; // ATAPI or SATA: not handled here
            }
            self.wait_data().ok()?;
            let mut id = [0u8; 512];
            insw(self.io, &mut id);
            let word = |i: usize| u16::from_le_bytes([id[i * 2], id[i * 2 + 1]]) as u64;
            let lba48 = word(83) & (1 << 10) != 0;
            let sectors = if lba48 { word(100) | word(101) << 16 | word(102) << 32 | word(103) << 48 } else { word(60) | word(61) << 16 };
            (sectors > 0).then_some((sectors, lba48))
        }
    }
}

impl AtaDisk {
    fn transfer(&self, lba: u64, buf: &mut [u8], write: Option<&[u8]>) -> KResult<()> {
        let count = buf.len() / SECTOR_SIZE;
        if lba + count as u64 > self.sectors {
            return Err(Errno::EIO);
        }
        let ch = self.channel.lock();
        let mut done = 0;
        while done < count {
            let n = (count - done).min(if self.lba48 { 65535 } else { 255 });
            let cur = lba + done as u64;
            ch.wait_not_busy()?;
            ch.setup(self.slave, cur, n as u16, self.lba48);
            let cmd = match (write.is_some(), self.lba48) {
                (false, false) => CMD_READ_PIO,
                (false, true) => CMD_READ_PIO_EXT,
                (true, false) => CMD_WRITE_PIO,
                (true, true) => CMD_WRITE_PIO_EXT,
            };
            unsafe { outb(ch.io + 7, cmd) };
            for s in done..done + n {
                ch.wait_data()?;
                let range = s * SECTOR_SIZE..(s + 1) * SECTOR_SIZE;
                unsafe {
                    match write {
                        Some(data) => outsw(ch.io, &data[range]),
                        None => insw(ch.io, &mut buf[range]),
                    }
                }
            }
            if write.is_some() {
                unsafe { outb(ch.io + 7, CMD_CACHE_FLUSH) };
                ch.wait_not_busy()?;
            }
            done += n;
        }
        Ok(())
    }
}

impl BlockDevice for AtaDisk {
    fn name(&self) -> &str {
        &self.name
    }

    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read_sectors(&self, lba: u64, buf: &mut [u8]) -> KResult<()> {
        self.transfer(lba, buf, None)
    }

    fn write_sectors(&self, lba: u64, buf: &[u8]) -> KResult<()> {
        let mut scratch = alloc::vec![0u8; buf.len()];
        self.transfer(lba, &mut scratch, Some(buf))
    }
}

/// Probes the two legacy IDE channels and registers the disks found.
pub fn init() {
    let channels = [(0x1F0u16, 0x3F6u16), (0x170, 0x376)];
    let names = ["hda", "hdb", "hdc", "hdd"];
    for (c, &(io, control)) in channels.iter().enumerate() {
        // Floating bus: no controller.
        if unsafe { inb(io + 7) } == 0xFF {
            continue;
        }
        let channel = Arc::new(Mutex::new(Channel { io, control }));
        unsafe { outb(control, 0x02) }; // polling: disable interrupts
        for slave in [false, true] {
            let found = channel.lock().identify(slave);
            if let Some((sectors, lba48)) = found {
                let idx = c * 2 + slave as usize;
                let disk = AtaDisk { name: String::from(names[idx]), channel: channel.clone(), slave, sectors, lba48 };
                block::register(Arc::new(disk), 3, (idx * 64) as u32);
            }
        }
    }
}
