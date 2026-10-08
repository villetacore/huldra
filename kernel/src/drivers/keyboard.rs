//! PS/2 keyboard (scancode set 1, US layout).

use crate::arch::port::inb;
use crate::sync::SpinLock;

const BUFFER_SIZE: usize = 128;

#[rustfmt::skip]
const NORMAL: [u8; 58] = *b"\0\x1b1234567890-=\x08\tqwertyuiop[]\n\0asdfghjkl;'`\0\\zxcvbnm,./\0*\0 ";
#[rustfmt::skip]
const SHIFTED: [u8; 58] = *b"\0\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\n\0ASDFGHJKL:\"~\0|ZXCVBNM<>?\0*\0 ";

struct Keyboard {
    shift: bool,
    ctrl: bool,
    caps: bool,
    extended: bool,
    buffer: [u8; BUFFER_SIZE],
    head: usize,
    tail: usize,
}

impl Keyboard {
    fn push(&mut self, c: u8) {
        let next = (self.head + 1) % BUFFER_SIZE;
        if next != self.tail {
            self.buffer[self.head] = c;
            self.head = next;
        }
    }

    fn pop(&mut self) -> Option<u8> {
        if self.head == self.tail {
            return None;
        }
        let c = self.buffer[self.tail];
        self.tail = (self.tail + 1) % BUFFER_SIZE;
        Some(c)
    }
}

static KEYBOARD: SpinLock<Keyboard> = SpinLock::new(Keyboard {
    shift: false,
    ctrl: false,
    caps: false,
    extended: false,
    buffer: [0; BUFFER_SIZE],
    head: 0,
    tail: 0,
});

/// IRQ1 handler: decodes one scancode into the input buffer.
pub fn handle_irq() {
    let scancode = unsafe { inb(0x60) };
    let mut kbd = KEYBOARD.lock();

    if scancode == 0xE0 {
        kbd.extended = true;
        return;
    }
    let extended = core::mem::replace(&mut kbd.extended, false);
    let released = scancode & 0x80 != 0;
    let code = scancode & 0x7F;

    match code {
        0x2A | 0x36 => kbd.shift = !released,
        0x1D => kbd.ctrl = !released,
        0x3A if !released => kbd.caps = !kbd.caps,
        _ if released => {}
        0x1C if extended => kbd.push(b'\r'), // keypad Enter
        _ if extended => {}                  // arrows etc. are not supported yet
        _ => {
            let Some(&base) = (if kbd.shift { &SHIFTED } else { &NORMAL }).get(code as usize) else {
                return;
            };
            if base == 0 {
                return;
            }
            let mut c = base;
            if kbd.caps && c.is_ascii_alphabetic() {
                c ^= 0x20;
            }
            if kbd.ctrl && c.is_ascii_alphabetic() {
                c = c.to_ascii_lowercase() & 0x1F;
            }
            kbd.push(c);
        }
    }
    let pending: alloc::vec::Vec<u8> = core::iter::from_fn(|| kbd.pop()).collect();
    drop(kbd);
    for c in pending {
        super::tty::input(c);
    }
}

