//! Terminal interface (`struct termios`, ioctl numbers).

pub const NCCS: usize = 19;

/// Kernel `struct termios`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; NCCS],
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

// c_iflag
pub const ICRNL: u32 = 0o400;
// c_oflag
pub const OPOST: u32 = 0o1;
pub const ONLCR: u32 = 0o4;
// c_cflag
pub const CS8: u32 = 0o60;
pub const CREAD: u32 = 0o200;
pub const B38400: u32 = 0o17;
// c_lflag
pub const ISIG: u32 = 0o1;
pub const ICANON: u32 = 0o2;
pub const ECHO: u32 = 0o10;
pub const ECHOE: u32 = 0o20;
pub const ECHOK: u32 = 0o40;
pub const ECHOCTL: u32 = 0o1000;
pub const IEXTEN: u32 = 0o100000;

// c_cc indices
pub const VINTR: usize = 0;
pub const VQUIT: usize = 1;
pub const VERASE: usize = 2;
pub const VKILL: usize = 3;
pub const VEOF: usize = 4;
pub const VTIME: usize = 5;
pub const VMIN: usize = 6;
pub const VSUSP: usize = 10;

pub const TCGETS: u32 = 0x5401;
pub const TCSETS: u32 = 0x5402;
pub const TCSETSW: u32 = 0x5403;
pub const TCSETSF: u32 = 0x5404;
pub const TIOCSCTTY: u32 = 0x540E;
pub const TIOCGPGRP: u32 = 0x540F;
pub const TIOCSPGRP: u32 = 0x5410;
pub const TIOCGWINSZ: u32 = 0x5413;
pub const FIONREAD: u32 = 0x541B;

impl Termios {
    /// Settings of a freshly opened terminal ("cooked" mode).
    pub fn sane() -> Termios {
        let mut cc = [0u8; NCCS];
        cc[VINTR] = 0x03; // ^C
        cc[VQUIT] = 0x1C; // ^\
        cc[VERASE] = 0x7F; // DEL
        cc[VKILL] = 0x15; // ^U
        cc[VEOF] = 0x04; // ^D
        cc[VMIN] = 1;
        cc[VSUSP] = 0x1A; // ^Z
        Termios {
            c_iflag: ICRNL,
            c_oflag: OPOST | ONLCR,
            c_cflag: B38400 | CS8 | CREAD,
            c_lflag: ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHOCTL | IEXTEN,
            c_line: 0,
            c_cc: cc,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_match_linux() {
        assert_eq!(core::mem::size_of::<Termios>(), 36);
        assert_eq!(core::mem::size_of::<crate::fs::Stat>(), 144);
        assert_eq!(core::mem::size_of::<crate::signal::SigAction>(), 32);
        assert_eq!(core::mem::size_of::<crate::Utsname>(), 390);
        assert_eq!(crate::fs::dirent64_reclen(1), 24);
    }

    #[test]
    fn dirent_roundtrip() {
        let mut buf = [0u8; 128];
        let a = crate::fs::encode_dirent64(&mut buf, 7, 1, crate::fs::DT_REG, b"hello").unwrap();
        let b = crate::fs::encode_dirent64(&mut buf[a..], 8, 2, crate::fs::DT_DIR, b"dir").unwrap();
        let v: [_; 2] = {
            let mut it = crate::fs::decode_dirents(&buf[..a + b]);
            [it.next().unwrap().name, it.next().unwrap().name]
        };
        assert_eq!(v, [&b"hello"[..], &b"dir"[..]]);
    }
}
