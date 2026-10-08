//! 16550 UART on COM1 (polled).

use crate::arch::port::{inb, outb};
use crate::sync::SpinLock;

const RX_SIZE: usize = 256;

struct RxBuffer {
    data: [u8; RX_SIZE],
    head: usize,
    tail: usize,
}

static RX: SpinLock<RxBuffer> = SpinLock::new(RxBuffer { data: [0; RX_SIZE], head: 0, tail: 0 });

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

pub fn write_str(s: &str) {
    for b in s.bytes() {
        write_byte(b);
    }
}

/// IRQ4 handler: drains the UART into the receive buffer.
pub fn handle_irq() {
    let mut rx = RX.lock();
    while let Some(b) = poll() {
        let next = (rx.head + 1) % RX_SIZE;
        if next != rx.tail {
            let h = rx.head;
            rx.data[h] = b;
            rx.head = next;
        }
    }
    drop(rx);
    super::input_ready();
}

pub fn read_char() -> Option<u8> {
    let mut rx = RX.lock();
    if rx.head == rx.tail {
        drop(rx);
        return poll();
    }
    let b = rx.data[rx.tail];
    rx.tail = (rx.tail + 1) % RX_SIZE;
    Some(b)
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
