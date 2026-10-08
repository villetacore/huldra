//! Legacy 8259 PIC, remapped to vectors 32..48.

use super::port::{inb, io_wait, outb};

pub const IRQ_BASE: u8 = 32;

const MASTER_CMD: u16 = 0x20;
const MASTER_DATA: u16 = 0x21;
const SLAVE_CMD: u16 = 0xA0;
const SLAVE_DATA: u16 = 0xA1;
const EOI: u8 = 0x20;

pub fn init() {
    unsafe {
        outb(MASTER_CMD, 0x11); // ICW1: init, expect ICW4
        io_wait();
        outb(SLAVE_CMD, 0x11);
        io_wait();
        outb(MASTER_DATA, IRQ_BASE); // ICW2: vector offsets
        io_wait();
        outb(SLAVE_DATA, IRQ_BASE + 8);
        io_wait();
        outb(MASTER_DATA, 4); // ICW3: slave on IRQ2
        io_wait();
        outb(SLAVE_DATA, 2);
        io_wait();
        outb(MASTER_DATA, 0x01); // ICW4: 8086 mode
        io_wait();
        outb(SLAVE_DATA, 0x01);
        io_wait();

        // Unmask timer (IRQ0) and keyboard (IRQ1) only.
        outb(MASTER_DATA, 0xFC);
        outb(SLAVE_DATA, 0xFF);
    }
}

#[allow(dead_code)]
pub fn unmask(irq: u8) {
    unsafe {
        let (port, bit) = if irq < 8 { (MASTER_DATA, irq) } else { (SLAVE_DATA, irq - 8) };
        outb(port, inb(port) & !(1 << bit));
    }
}

pub fn eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            outb(SLAVE_CMD, EOI);
        }
        outb(MASTER_CMD, EOI);
    }
}
