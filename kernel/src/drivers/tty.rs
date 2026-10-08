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
use alloc::sync::{Arc, Weak};
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
    winsize: Winsize,
    /// The other side of a pseudo-terminal went away: reads return 0.
    hung_up: bool,
}

/// Where a terminal's output goes.
pub enum Output {
    Console,
    Pty(Weak<super::pty::PtyMaster>),
}

pub struct Tty {
    ino: u64,
    rdev: u64,
    state: SpinLock<State>,
    readers: WaitQueue,
    output: Output,
}

static CONSOLE: SpinLock<Option<Arc<Tty>>> = SpinLock::new(None);

pub fn console() -> Arc<Tty> {
    CONSOLE.lock().clone().expect("tty not initialized")
}

impl Tty {
    pub fn new(rdev: u64, output: Output, winsize: Winsize) -> Tty {
        Tty {
            ino: crate::fs::devfs::dev_ino(),
            rdev,
            state: SpinLock::new(State {
                termios: Termios::sane(),
                line: Vec::new(),
                ready: VecDeque::new(),
                eof_marks: VecDeque::new(),
                foreground: 0,
                session: 0,
                winsize,
                hung_up: false,
            }),
            readers: WaitQueue::new(),
            output,
        }
    }

    /// Output processing (ONLCR) and delivery to the console or pty master.
    fn emit(&self, t: &Termios, bytes: &[u8]) {
        match &self.output {
            Output::Console => crate::console::write_bytes(bytes),
            Output::Pty(m) => {
                if let Some(m) = m.upgrade() {
                    if t.c_oflag & OPOST != 0 && t.c_oflag & ONLCR != 0 {
                        let mut v = Vec::with_capacity(bytes.len() + 8);
                        for &b in bytes {
                            if b == b'\n' {
                                v.push(b'\r');
                            }
                            v.push(b);
                        }
                        m.push(&v);
                    } else {
                        m.push(bytes);
                    }
                }
            }
        }
    }

    fn echo_char(&self, t: &Termios, c: u8) {
        if t.c_lflag & ECHO == 0 {
            return;
        }
        if c < 0x20 && c != b'\n' && c != b'\t' && t.c_lflag & ECHOCTL != 0 {
            self.emit(t, &[b'^', c + 0x40]);
        } else {
            self.emit(t, &[c]);
        }
    }

    /// The pty master closed.
    pub fn hang_up(&self) {
        let fg = {
            let mut s = self.state.lock();
            s.hung_up = true;
            s.foreground
        };
        self.readers.wake_all();
        if fg != 0 {
            crate::proc::signal::send_to_group(fg, huldra_abi::signal::SIGHUP);
        }
    }

    pub fn set_winsize(&self, w: Winsize) {
        let fg = {
            let mut s = self.state.lock();
            s.winsize = w;
            s.foreground
        };
        if fg != 0 {
            crate::proc::signal::send_to_group(fg, huldra_abi::signal::SIGWINCH);
        }
    }

    pub fn winsize(&self) -> Winsize {
        self.state.lock().winsize
    }
}

pub fn init() {
    let tty = Arc::new(Tty::new(makedev(5, 1), Output::Console, Winsize { ws_row: 25, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 }));
    *CONSOLE.lock() = Some(tty.clone());
    crate::fs::devfs::register("console", tty.clone());
    crate::fs::devfs::register("tty", tty.clone());
    crate::fs::devfs::register("tty0", tty);
}

/// Feeds one input byte from a keyboard or serial interrupt.
pub fn input(c: u8) {
    let Some(tty) = CONSOLE.lock().clone() else {
        return;
    };
    tty.receive(c);
}

impl Tty {
    pub fn receive(&self, mut c: u8) {
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
                self.echo_char(&t, c);
                if t.c_lflag & ECHO != 0 {
                    self.emit(&t, b"\n");
                }
            } else if t.c_lflag & ICANON != 0 {
                if c == t.c_cc[VERASE] || c == 0x08 {
                    if s.line.pop().is_some() && t.c_lflag & ECHOE != 0 {
                        self.emit(&t, b"\x08 \x08");
                    }
                } else if c == t.c_cc[VKILL] {
                    while s.line.pop().is_some() {
                        if t.c_lflag & ECHOK != 0 {
                            self.emit(&t, b"\x08 \x08");
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
                    self.echo_char(&t, b'\n');
                } else if s.line.len() < MAX_LINE {
                    s.line.push(c);
                    self.echo_char(&t, c);
                }
            } else {
                if s.ready.len() < MAX_INPUT {
                    s.ready.push_back(c);
                }
                self.echo_char(&t, c);
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
        m.rdev = self.rdev;
        m
    }

    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> KResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let (canonical, vmin, vtime) = {
            let s = self.state.lock();
            (s.termios.c_lflag & ICANON != 0, s.termios.c_cc[VMIN], s.termios.c_cc[VTIME])
        };
        if !canonical && vmin == 0 {
            // Polling read (VTIME = 0) or read with a timeout in 1/10 s.
            let deadline = crate::time::ticks() + crate::time::ms_to_ticks(vtime as u64 * 100);
            let take = || {
                let mut s = self.state.lock();
                if s.ready.is_empty() {
                    return None;
                }
                let n = buf.len().min(s.ready.len());
                for (dst, src) in buf.iter_mut().zip(s.ready.drain(..n)) {
                    *dst = src;
                }
                Some(n)
            };
            return Ok(self.readers.wait_until_deadline(take, deadline)?.unwrap_or(0));
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
                return s.hung_up.then_some(0);
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
        let t = {
            let s = self.state.lock();
            if s.hung_up {
                return Err(Errno::EIO);
            }
            s.termios
        };
        self.emit(&t, buf);
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
                write_user(arg, &self.winsize())?;
                Ok(0)
            }
            TIOCSWINSZ => {
                let w: Winsize = read_user(arg)?;
                self.set_winsize(w);
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

    fn poll(&self) -> PollState {
        let s = self.state.lock();
        let readable = if s.termios.c_lflag & ICANON != 0 {
            s.ready.contains(&b'\n') || !s.eof_marks.is_empty() || s.ready.len() >= MAX_INPUT
        } else {
            !s.ready.is_empty()
        };
        PollState { readable: readable || s.hung_up, writable: true, hangup: s.hung_up }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
