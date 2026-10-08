//! 16550 UART on COM1 (polled).

use crate::arch::port::{inb, outb};

const COM1: u16 = 0x3F8;
const LINE_STATUS: u16 = COM1 + 5;

pub fn init() {
    unsafe {
        outb(COM1 + 1, 0x00); // disable UART interrupts
        outb(COM1 + 3, 0x80); // DLAB on
        outb(COM1, 0x01); // divisor 1 = 115200 baud
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03); // 8N1, DLAB off
        outb(COM1 + 2, 0xC7); // FIFO on, clear, 14-byte threshold
        outb(COM1 + 4, 0x0B); // DTR | RTS | OUT2 (routes the IRQ to the PIC)
        outb(COM1 + 1, 0x01); // interrupt when data arrives
    }
}

pub fn write_byte(b: u8) {
    unsafe {
        // Bounded wait so a missing UART cannot hang the kernel.
        for _ in 0..100_000 {
            if inb(LINE_STATUS) & 0x20 != 0 {
                break;
            }
        }
        outb(COM1, b);
    }
}

/// IRQ4 handler: passes received bytes to the terminal.
pub fn handle_irq() {
    while let Some(b) = poll() {
        super::tty::input(b);
    }
}

fn poll() -> Option<u8> {
    unsafe {
        if inb(LINE_STATUS) & 0x01 != 0 {
            Some(inb(COM1))
        } else {
            None
        }
    }
}
