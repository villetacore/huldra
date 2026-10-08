//! The console terminal: line discipline on top of the keyboard and serial
//! input and the VGA/serial output.
//!
//! In canonical mode input is collected into lines with editing (erase,
//! kill, EOF) and echo; reads return whole lines. In raw mode every byte is
//! delivered immediately. `^C`/`^\` send SIGINT/SIGQUIT to the foreground
//! process group.

use crate::fs::vfs::*;
use crate::sync::SpinLock;
use crate::task::wait::WaitQueue;
use crate::task::Pid;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::Any;
use huldra_abi::errno::Errno;
use huldra_abi::signal::{SIGINT, SIGQUIT};
use huldra_abi::termios::*;

const MAX_LINE: usize = 4096;
const MAX_INPUT: usize = 16 * 1024;

struct State {
    termios: Termios,
    /// Line being edited (canonical mode).
    line: Vec<u8>,
    /// Bytes ready to be read.
    ready: VecDeque<u8>,
    /// Pending end-of-file marks (^D on an empty line): positions in `ready`.
    eof_marks: VecDeque<usize>,
    foreground: Pid,
    session: Pid,
}

pub struct Tty {
    ino: u64,
    state: SpinLock<State>,
    readers: WaitQueue,
}

static CONSOLE: SpinLock<Option<Arc<Tty>>> = SpinLock::new(None);

pub fn console() -> Arc<Tty> {
    CONSOLE.lock().clone().expect("tty not initialized")
}

pub fn init() {
    let tty = Arc::new(Tty {
        ino: crate::fs::devfs::dev_ino(),
        state: SpinLock::new(State {
            termios: Termios::sane(),
            line: Vec::new(),
            ready: VecDeque::new(),
            eof_marks: VecDeque::new(),
            foreground: 0,
            session: 0,
        }),
        readers: WaitQueue::new(),
    });
    *CONSOLE.lock() = Some(tty.clone());
    crate::fs::devfs::register("console", tty.clone());
    crate::fs::devfs::register("tty", tty.clone());
    crate::fs::devfs::register("tty0", tty);
}

fn echo(bytes: &[u8]) {
    crate::console::write_bytes(bytes);
}

fn echo_char(t: &Termios, c: u8) {
    if t.c_lflag & ECHO == 0 {
        return;
    }
    if c < 0x20 && c != b'\n' && c != b'\t' && t.c_lflag & ECHOCTL != 0 {
        echo(&[b'^', c + 0x40]);
    } else {
        echo(&[c]);
    }
}

/// Feeds one input byte from a keyboard or serial interrupt.
pub fn input(c: u8) {
    let Some(tty) = CONSOLE.lock().clone() else {
        return;
    };
    tty.receive(c);
}

impl Tty {
    fn receive(&self, mut c: u8) {
        let mut signal = None;
        {
            let mut s = self.state.lock();
            let t = s.termios;
            if t.c_iflag & ICRNL != 0 && c == b'\r' {
                c = b'\n';
            }
            if t.c_lflag & ISIG != 0 && (c == t.c_cc[VINTR] || c == t.c_cc[VQUIT]) {
                signal = Some((
                    if c == t.c_cc[VINTR] { SIGINT } else { SIGQUIT },
                    s.foreground,
                ));
                s.line.clear();
                echo_char(&t, c);
                if t.c_lflag & ECHO != 0 {
                    echo(b"\n");
                }
            } else if t.c_lflag & ICANON != 0 {
                if c == t.c_cc[VERASE] || c == 0x08 {
                    if s.line.pop().is_some() && t.c_lflag & ECHOE != 0 {
                        echo(b"\x08 \x08");
                    }
                } else if c == t.c_cc[VKILL] {
                    while s.line.pop().is_some() {
                        if t.c_lflag & ECHOK != 0 {
                            echo(b"\x08 \x08");
                        }
                    }
                } else if c == t.c_cc[VEOF] {
                    let line = core::mem::take(&mut s.line);
                    s.ready.extend(line);
                    let at = s.ready.len();
                    s.eof_marks.push_back(at);
                } else if c == b'\n' {
                    let mut line = core::mem::take(&mut s.line);
                    line.push(b'\n');
                    if s.ready.len() + line.len() <= MAX_INPUT {
                        s.ready.extend(line);
                    }
                    echo_char(&t, b'\n');
                } else if s.line.len() < MAX_LINE {
                    s.line.push(c);
                    echo_char(&t, c);
                }
            } else {
                if s.ready.len() < MAX_INPUT {
                    s.ready.push_back(c);
                }
                echo_char(&t, c);
            }
        }
        if let Some((sig, pgrp)) = signal {
            if pgrp != 0 {
                crate::proc::signal::send_to_group(pgrp, sig);
            }
        }
        self.readers.wake_all();
    }

    pub fn set_foreground(&self, pgrp: Pid) {
        self.state.lock().foreground = pgrp;
    }

    pub fn foreground(&self) -> Pid {
        self.state.lock().foreground
    }

    pub fn set_session(&self, sid: Pid) {
        self.state.lock().session = sid;
    }
}

impl Inode for Tty {
    fn metadata(&self) -> Metadata {
        let mut m = Metadata::new(0, self.ino, FileType::CharDevice, 0o620);
        m.rdev = makedev(5, 1);
        m
    }

    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> KResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.readers.wait_until(|| {
            let mut s = self.state.lock();
            let canonical = s.termios.c_lflag & ICANON != 0;
            // An EOF mark at the front of the queue: return 0 once.
            if s.eof_marks.front() == Some(&0) {
                s.eof_marks.pop_front();
                return Some(0);
            }
            if s.ready.is_empty() {
                return None;
            }
            let limit = s.eof_marks.front().copied().unwrap_or(usize::MAX);
            let mut n = 0;
            while n < buf.len() && n < limit {
                let Some(b) = s.ready.pop_front() else { break };
                buf[n] = b;
                n += 1;
                if canonical && b == b'\n' {
                    break;
                }
            }
            for m in s.eof_marks.iter_mut() {
                *m -= n;
            }
            Some(n)
        })
    }

    fn write_at(&self, _offset: u64, buf: &[u8]) -> KResult<usize> {
        crate::console::write_bytes(buf);
        Ok(buf.len())
    }

    fn ioctl(&self, cmd: u32, arg: u64) -> KResult<u64> {
        use crate::proc::uaccess::{read_user, write_user};
        match cmd {
            TCGETS => {
                let t = self.state.lock().termios;
                write_user(arg, &t)?;
                Ok(0)
            }
            TCSETS | TCSETSW | TCSETSF => {
                let t: Termios = read_user(arg)?;
                let mut s = self.state.lock();
                if cmd == TCSETSF {
                    s.ready.clear();
                    s.line.clear();
                    s.eof_marks.clear();
                }
                // Leaving canonical mode makes the partial line readable.
                if s.termios.c_lflag & ICANON != 0 && t.c_lflag & ICANON == 0 {
                    let line = core::mem::take(&mut s.line);
                    s.ready.extend(line);
                }
                s.termios = t;
                drop(s);
                self.readers.wake_all();
                Ok(0)
            }
            TIOCGPGRP => {
                write_user(arg, &(self.foreground() as i32))?;
                Ok(0)
            }
            TIOCSPGRP => {
                let pgrp: i32 = read_user(arg)?;
                if pgrp <= 0 {
                    return Err(Errno::EINVAL);
                }
                self.set_foreground(pgrp as Pid);
                Ok(0)
            }
            TIOCSCTTY => Ok(0),
            TIOCGWINSZ => {
                write_user(
                    arg,
                    &Winsize {
                        ws_row: 25,
                        ws_col: 80,
                        ws_xpixel: 0,
                        ws_ypixel: 0,
                    },
                )?;
                Ok(0)
            }
            FIONREAD => {
                let n = self.state.lock().ready.len() as i32;
                write_user(arg, &n)?;
                Ok(0)
            }
            _ => Err(Errno::ENOTTY),
        }
    }

    fn bytes_available(&self) -> Option<usize> {
        Some(self.state.lock().ready.len())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
