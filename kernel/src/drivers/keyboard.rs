//! PS/2 keyboard (scancode set 1, US layout).

use crate::arch::port::inb;
use crate::sync::SpinLock;

const BUFFER_SIZE: usize = 128;

#[rustfmt::skip]
const NORMAL: [u8; 58] = *b"\0\x1b1234567890-=\x7f\tqwertyuiop[]\r\0asdfghjkl;'`\0\\zxcvbnm,./\0*\0 ";
#[rustfmt::skip]
const SHIFTED: [u8; 58] = *b"\0\x1b!@#$%^&*()_+\x7f\tQWERTYUIOP{}\r\0ASDFGHJKL:\"~\0|ZXCVBNM<>?\0*\0 ";

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

    fn push_str(&mut self, s: &[u8]) {
        for &c in s {
            self.push(c);
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
        _ if extended => {
            // Cursor block: the same escape sequences as a VT100/xterm.
            let seq: &[u8] = match code {
                0x48 => b"\x1b[A",
                0x50 => b"\x1b[B",
                0x4D => b"\x1b[C",
                0x4B => b"\x1b[D",
                0x47 => b"\x1b[H",
                0x4F => b"\x1b[F",
                0x49 => b"\x1b[5~",
                0x51 => b"\x1b[6~",
                0x52 => b"\x1b[2~",
                0x53 => b"\x1b[3~",
                0x35 => b"/", // keypad divide
                _ => b"",
            };
            kbd.push_str(seq);
        }
        0x3B..=0x44 | 0x57 | 0x58 => {
            const FKEYS: [&[u8]; 12] = [
                b"\x1bOP", b"\x1bOQ", b"\x1bOR", b"\x1bOS", b"\x1b[15~", b"\x1b[17~", b"\x1b[18~",
                b"\x1b[19~", b"\x1b[20~", b"\x1b[21~", b"\x1b[23~", b"\x1b[24~",
            ];
            let i = match code {
                0x57 => 10,
                0x58 => 11,
                c => (c - 0x3B) as usize,
            };
            kbd.push_str(FKEYS[i]);
        }
        _ => {
            let Some(&base) = (if kbd.shift { &SHIFTED } else { &NORMAL }).get(code as usize)
            else {
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
