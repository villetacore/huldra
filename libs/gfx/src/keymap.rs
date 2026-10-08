//! Key codes (PS/2 set-1 scancodes, +0x100 for E0-prefixed keys), the US
//! layout, modifier tracking and terminal key sequences.

use crate::proto::{MOD_ALT, MOD_CTRL, MOD_SHIFT, MOD_SUPER};
use alloc::vec::Vec;

pub const ESC: u16 = 0x01;
pub const BACKSPACE: u16 = 0x0E;
pub const TAB: u16 = 0x0F;
pub const ENTER: u16 = 0x1C;
pub const LCTRL: u16 = 0x1D;
pub const LSHIFT: u16 = 0x2A;
pub const RSHIFT: u16 = 0x36;
pub const LALT: u16 = 0x38;
pub const SPACE: u16 = 0x39;
pub const CAPS: u16 = 0x3A;
pub const F1: u16 = 0x3B;
pub const F10: u16 = 0x44;
pub const F11: u16 = 0x57;
pub const F12: u16 = 0x58;
pub const KP_ENTER: u16 = 0x11C;
pub const RCTRL: u16 = 0x11D;
pub const RALT: u16 = 0x138;
pub const HOME: u16 = 0x147;
pub const UP: u16 = 0x148;
pub const PGUP: u16 = 0x149;
pub const LEFT: u16 = 0x14B;
pub const RIGHT: u16 = 0x14D;
pub const END: u16 = 0x14F;
pub const DOWN: u16 = 0x150;
pub const PGDN: u16 = 0x151;
pub const INSERT: u16 = 0x152;
pub const DELETE: u16 = 0x153;
pub const LSUPER: u16 = 0x15B;
pub const RSUPER: u16 = 0x15C;

/// Scancode of a letter or digit key (for key bindings).
pub fn code_of(c: char) -> u16 {
    NORMAL.iter().position(|&b| b == c.to_ascii_lowercase() as u8 && b != 0).map_or(0, |i| i as u16)
}

#[rustfmt::skip]
const NORMAL: [u8; 58] = *b"\0\x1b1234567890-=\x08\tqwertyuiop[]\r\0asdfghjkl;'`\0\\zxcvbnm,./\0*\0 ";
#[rustfmt::skip]
const SHIFTED: [u8; 58] = *b"\0\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\r\0ASDFGHJKL:\"~\0|ZXCVBNM<>?\0*\0 ";

/// The character a key types (0 if none). Ctrl does not change it.
pub fn character(code: u16, mods: u8, caps: bool) -> u32 {
    if code == KP_ENTER {
        return '\r' as u32;
    }
    if code == 0x135 {
        return '/' as u32;
    }
    if code >= 0x100 {
        return 0;
    }
    let shift = mods & MOD_SHIFT != 0;
    let table = if shift { &SHIFTED } else { &NORMAL };
    let Some(&c) = table.get(code as usize) else {
        // Keypad digits and operators.
        return match code {
            0x47..=0x53 => b"789-456+1230."[(code - 0x47) as usize] as u32,
            _ => 0,
        };
    };
    if c == 0 {
        return 0;
    }
    let mut c = c;
    if caps && c.is_ascii_alphabetic() {
        c ^= 0x20;
    }
    c as u32
}

/// Tracks modifier keys from press/release events.
#[derive(Default, Clone, Copy)]
pub struct Modifiers {
    pub mods: u8,
    pub caps: bool,
    shift: u8,
    ctrl: u8,
    alt: u8,
    sup: u8,
}

impl Modifiers {
    /// Updates the state; returns true if `code` was a modifier key.
    pub fn update(&mut self, code: u16, pressed: bool) -> bool {
        let bit = |held: &mut u8, mask: u8| {
            if pressed {
                *held |= mask;
            } else {
                *held &= !mask;
            }
        };
        match code {
            LSHIFT => bit(&mut self.shift, 1),
            RSHIFT => bit(&mut self.shift, 2),
            LCTRL => bit(&mut self.ctrl, 1),
            RCTRL => bit(&mut self.ctrl, 2),
            LALT => bit(&mut self.alt, 1),
            RALT => bit(&mut self.alt, 2),
            LSUPER => bit(&mut self.sup, 1),
            RSUPER => bit(&mut self.sup, 2),
            CAPS => {
                if pressed {
                    self.caps = !self.caps;
                }
            }
            _ => return false,
        }
        self.mods = (if self.shift != 0 { MOD_SHIFT } else { 0 }) | (if self.ctrl != 0 { MOD_CTRL } else { 0 }) | (if self.alt != 0 { MOD_ALT } else { 0 }) | (if self.sup != 0 { MOD_SUPER } else { 0 });
        true
    }
}

/// Bytes a terminal sends for a key press (xterm conventions).
pub fn terminal_bytes(code: u16, mods: u8, ch: u32, app_cursor: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let cursor = |c: u8, out: &mut Vec<u8>| {
        if mods & (MOD_SHIFT | MOD_CTRL | MOD_ALT) != 0 {
            let m = 1 + (mods & MOD_SHIFT != 0) as u8 + 2 * (mods & MOD_ALT != 0) as u8 + 4 * (mods & MOD_CTRL != 0) as u8;
            out.extend_from_slice(b"\x1b[1;");
            out.push(b'0' + m);
            out.push(c);
        } else if app_cursor {
            out.extend_from_slice(&[0x1b, b'O', c]);
        } else {
            out.extend_from_slice(&[0x1b, b'[', c]);
        }
    };
    match code {
        UP => cursor(b'A', &mut out),
        DOWN => cursor(b'B', &mut out),
        RIGHT => cursor(b'C', &mut out),
        LEFT => cursor(b'D', &mut out),
        HOME => cursor(b'H', &mut out),
        END => cursor(b'F', &mut out),
        PGUP => out.extend_from_slice(b"\x1b[5~"),
        PGDN => out.extend_from_slice(b"\x1b[6~"),
        INSERT => out.extend_from_slice(b"\x1b[2~"),
        DELETE => out.extend_from_slice(b"\x1b[3~"),
        BACKSPACE => out.push(0x7F),
        F1..=F10 | F11 | F12 => {
            const SEQ: [&[u8]; 12] = [b"\x1bOP", b"\x1bOQ", b"\x1bOR", b"\x1bOS", b"\x1b[15~", b"\x1b[17~", b"\x1b[18~", b"\x1b[19~", b"\x1b[20~", b"\x1b[21~", b"\x1b[23~", b"\x1b[24~"];
            let i = match code {
                F11 => 10,
                F12 => 11,
                c => (c - F1) as usize,
            };
            out.extend_from_slice(SEQ[i]);
        }
        _ => {
            if ch == 0 {
                return out;
            }
            let mut c = ch;
            if mods & MOD_CTRL != 0 {
                let lower = (c as u8).to_ascii_lowercase();
                c = match lower {
                    b'a'..=b'z' => (lower - b'a' + 1) as u32,
                    b'@' | b' ' | b'2' => 0,
                    b'[' | b'3' => 27,
                    b'\\' | b'4' => 28,
                    b']' | b'5' => 29,
                    b'/' | b'7' => 31,
                    _ => c,
                };
            }
            if mods & MOD_ALT != 0 {
                out.push(0x1b);
            }
            let mut b = [0u8; 4];
            out.extend_from_slice(char::from_u32(c).unwrap_or('?').encode_utf8(&mut b).as_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        assert_eq!(character(0x1E, 0, false), 'a' as u32);
        assert_eq!(character(0x1E, MOD_SHIFT, false), 'A' as u32);
        assert_eq!(character(0x1E, 0, true), 'A' as u32);
        assert_eq!(character(0x02, MOD_SHIFT, false), '!' as u32);
        assert_eq!(character(UP, 0, false), 0);
        assert_eq!(code_of('q'), 0x10);
        assert_eq!(code_of('1'), 0x02);
        let mut m = Modifiers::default();
        m.update(LCTRL, true);
        m.update(RALT, true);
        assert_eq!(m.mods, MOD_CTRL | MOD_ALT);
        m.update(LCTRL, false);
        assert_eq!(m.mods, MOD_ALT);
    }

    #[test]
    fn terminal() {
        assert_eq!(terminal_bytes(UP, 0, 0, false), b"\x1b[A");
        assert_eq!(terminal_bytes(UP, 0, 0, true), b"\x1bOA");
        assert_eq!(terminal_bytes(0x2E, MOD_CTRL, 'c' as u32, false), [3]);
        assert_eq!(terminal_bytes(0x1E, MOD_ALT, 'a' as u32, false), b"\x1ba");
        assert_eq!(terminal_bytes(BACKSPACE, 0, 8, false), [0x7F]);
        assert_eq!(terminal_bytes(F1, 0, 0, false), b"\x1bOP");
    }
}
