//! Pseudo-terminals: opening `/dev/ptmx` creates a master and a slave
//! `/dev/pts/N`. The slave is an ordinary terminal (line discipline, job
//! control); what programs write to it is read from the master, and what
//! is written to the master is typed into it. Terminal emulators use them.

use super::tty::{Output, Tty};
use crate::fs::vfs::*;
use crate::sync::SpinLock;
use crate::task::wait::WaitQueue;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::any::Any;
use core::sync::atomic::{AtomicU32, Ordering};
use huldra_abi::errno::Errno;
use huldra_abi::termios::*;

const BUFFER: usize = 64 * 1024;

static PTYS: SpinLock<BTreeMap<u32, Weak<Tty>>> = SpinLock::new(BTreeMap::new());
static NEXT: AtomicU32 = AtomicU32::new(0);

pub struct PtyMaster {
    n: u32,
    ino: u64,
    slave: Arc<Tty>,
    out: SpinLock<VecDeque<u8>>,
    readers: WaitQueue,
}

impl PtyMaster {
    /// Output written to the slave.
    pub fn push(&self, bytes: &[u8]) {
        {
            let mut out = self.out.lock();
            let room = BUFFER.saturating_sub(out.len());
            out.extend(&bytes[..bytes.len().min(room)]);
        }
        self.readers.wake_all();
    }
}

impl Inode for PtyMaster {
    fn metadata(&self) -> Metadata {
        let mut m = Metadata::new(0, self.ino, FileType::CharDevice, 0o666);
        m.rdev = makedev(5, 2);
        m
    }

    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> KResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.readers.wait_until(|| {
            let mut out = self.out.lock();
            if out.is_empty() {
                return None;
            }
            let n = buf.len().min(out.len());
            for (d, s) in buf.iter_mut().zip(out.drain(..n)) {
                *d = s;
            }
            Some(n)
        })
    }

    fn write_at(&self, _offset: u64, buf: &[u8]) -> KResult<usize> {
        for &b in buf {
            self.slave.receive(b);
        }
        Ok(buf.len())
    }

    fn ioctl(&self, cmd: u32, arg: u64) -> KResult<u64> {
        match cmd {
            TIOCGPTN => {
                crate::proc::uaccess::write_user(arg, &self.n)?;
                Ok(0)
            }
            TIOCSPTLCK => Ok(0),
            TIOCGWINSZ | TIOCSWINSZ | TCGETS | TCSETS | TCSETSW | TCSETSF => self.slave.ioctl(cmd, arg),
            _ => Err(Errno::ENOTTY),
        }
    }

    fn poll(&self) -> PollState {
        PollState { readable: !self.out.lock().is_empty(), writable: true, hangup: false }
    }

    fn release(&self, _flags: u32) {
        PTYS.lock().remove(&self.n);
        self.slave.hang_up();
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `/dev/ptmx`: every open makes a new pair.
struct Ptmx {
    ino: u64,
}

impl Inode for Ptmx {
    fn metadata(&self) -> Metadata {
        let mut m = Metadata::new(0, self.ino, FileType::CharDevice, 0o666);
        m.rdev = makedev(5, 2);
        m
    }

    fn open_instance(&self) -> Option<KResult<Arc<dyn Inode>>> {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let master = Arc::new_cyclic(|weak: &Weak<PtyMaster>| PtyMaster {
            n,
            ino: crate::fs::devfs::dev_ino(),
            slave: Arc::new(Tty::new(makedev(136, n), Output::Pty(weak.clone()), Winsize { ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 })),
            out: SpinLock::new(VecDeque::new()),
            readers: WaitQueue::new(),
        });
        PTYS.lock().insert(n, Arc::downgrade(&master.slave));
        Some(Ok(master))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `/dev/pts`: the slave ends.
struct PtsDir {
    ino: u64,
}

impl Inode for PtsDir {
    fn metadata(&self) -> Metadata {
        Metadata::new(0, self.ino, FileType::Directory, 0o755)
    }

    fn lookup(&self, name: &str) -> KResult<Arc<dyn Inode>> {
        let n: u32 = name.parse().map_err(|_| Errno::ENOENT)?;
        let tty = PTYS.lock().get(&n).and_then(|w| w.upgrade()).ok_or(Errno::ENOENT)?;
        Ok(tty)
    }

    fn readdir(&self) -> KResult<Vec<DirEntry>> {
        Ok(PTYS
            .lock()
            .iter()
            .filter_map(|(n, w)| w.upgrade().map(|t| DirEntry { name: format!("{}", n), ino: t.metadata().ino, kind: FileType::CharDevice }))
            .collect())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn init() {
    crate::fs::devfs::register("ptmx", Arc::new(Ptmx { ino: crate::fs::devfs::dev_ino() }));
    crate::fs::devfs::register("pts", Arc::new(PtsDir { ino: crate::fs::devfs::dev_ino() }));
}
