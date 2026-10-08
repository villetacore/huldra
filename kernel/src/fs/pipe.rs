//! Pipes: a bounded byte queue with a read end and a write end.

use super::vfs::*;
use crate::sync::SpinLock;
use crate::task::wait::WaitQueue;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use core::any::Any;
use core::sync::atomic::{AtomicU64, Ordering};
use huldra_abi::errno::Errno;
use huldra_abi::fs::{O_ACCMODE, O_NONBLOCK, O_RDONLY, O_RDWR, O_WRONLY};

const CAPACITY: usize = 64 * 1024;
static NEXT_INO: AtomicU64 = AtomicU64::new(1);

struct State {
    data: VecDeque<u8>,
    readers: usize,
    writers: usize,
}

pub struct Pipe {
    ino: u64,
    state: SpinLock<State>,
    readable: WaitQueue,
    writable: WaitQueue,
}

impl Pipe {
    pub fn new() -> Arc<Pipe> {
        Arc::new(Pipe {
            ino: NEXT_INO.fetch_add(1, Ordering::Relaxed),
            state: SpinLock::new(State {
                data: VecDeque::new(),
                readers: 0,
                writers: 0,
            }),
            readable: WaitQueue::new(),
            writable: WaitQueue::new(),
        })
    }
}

impl Inode for Pipe {
    fn metadata(&self) -> Metadata {
        let mut m = Metadata::new(0, self.ino, FileType::Fifo, 0o600);
        m.size = self.state.lock().data.len() as u64;
        m
    }

    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> KResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let n = self.readable.wait_until(|| {
            let mut s = self.state.lock();
            if s.data.is_empty() {
                // End of file once every writer is gone.
                return (s.writers == 0).then_some(0);
            }
            let n = buf.len().min(s.data.len());
            for (dst, src) in buf.iter_mut().zip(s.data.drain(..n)) {
                *dst = src;
            }
            Some(n)
        })?;
        self.writable.wake_all();
        Ok(n)
    }

    fn write_at(&self, _offset: u64, buf: &[u8]) -> KResult<usize> {
        let mut written = 0;
        while written < buf.len() {
            let n = self.writable.wait_until(|| {
                let mut s = self.state.lock();
                if s.readers == 0 {
                    return Some(Err(Errno::EPIPE));
                }
                let room = CAPACITY - s.data.len();
                if room == 0 {
                    return None;
                }
                let n = room.min(buf.len() - written);
                s.data.extend(&buf[written..written + n]);
                Some(Ok(n))
            });
            match n {
                Ok(Ok(n)) => {
                    written += n;
                    self.readable.wake_all();
                }
                Ok(Err(e)) | Err(e) => {
                    if e == Errno::EPIPE {
                        crate::proc::signal::send_current(huldra_abi::signal::SIGPIPE);
                    }
                    return if written > 0 { Ok(written) } else { Err(e) };
                }
            }
        }
        Ok(written)
    }

    fn open(&self, flags: u32) -> KResult<()> {
        let mut s = self.state.lock();
        match flags & O_ACCMODE {
            O_RDONLY => s.readers += 1,
            O_WRONLY => s.writers += 1,
            O_RDWR => {
                s.readers += 1;
                s.writers += 1;
            }
            _ => return Err(Errno::EINVAL),
        }
        Ok(())
    }

    fn release(&self, flags: u32) {
        {
            let mut s = self.state.lock();
            match flags & O_ACCMODE {
                O_RDONLY => s.readers -= 1,
                O_WRONLY => s.writers -= 1,
                _ => {
                    s.readers -= 1;
                    s.writers -= 1;
                }
            }
        }
        self.readable.wake_all();
        self.writable.wake_all();
    }

    fn bytes_available(&self) -> Option<usize> {
        Some(self.state.lock().data.len())
    }

    fn poll(&self) -> PollState {
        let s = self.state.lock();
        PollState {
            readable: !s.data.is_empty() || s.writers == 0,
            writable: s.data.len() < CAPACITY || s.readers == 0,
            hangup: s.writers == 0,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Creates a pipe and returns its (read end, write end).
pub fn create() -> KResult<(Arc<super::OpenFile>, Arc<super::OpenFile>)> {
    let pipe = Pipe::new();
    let r = super::OpenFile::new(pipe.clone(), O_RDONLY, "pipe:".into())?;
    let w = super::OpenFile::new(pipe, O_WRONLY, "pipe:".into())?;
    let _ = O_NONBLOCK;
    Ok((r, w))
}
