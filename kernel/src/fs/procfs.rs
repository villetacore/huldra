//! procfs: kernel and process information as text files under `/proc`.

use super::vfs::*;
use crate::task::{self, Pid};
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::any::Any;
use core::fmt::Write;
use huldra_abi::errno::Errno;

type Generator = Box<dyn Fn() -> String + Send + Sync>;

/// A read-only file whose content is generated on every read.
struct ProcFile {
    dev: u64,
    ino: u64,
    generate: Generator,
}

impl Inode for ProcFile {
    fn metadata(&self) -> Metadata {
        Metadata::new(self.dev, self.ino, FileType::Regular, 0o444)
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> KResult<usize> {
        let content = (self.generate)();
        let bytes = content.as_bytes();
        let start = (offset as usize).min(bytes.len());
        let n = buf.len().min(bytes.len() - start);
        buf[..n].copy_from_slice(&bytes[start..start + n]);
        Ok(n)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

const STATIC_FILES: &[&str] = &[
    "cpuinfo",
    "interrupts",
    "kmsg",
    "meminfo",
    "mounts",
    "pci",
    "uptime",
    "version",
];
const PID_FILES: &[&str] = &["cmdline", "stat", "status"];

fn static_file(name: &str) -> Option<String> {
    Some(match name {
        "uptime" => {
            let ms = crate::time::uptime_ms();
            let idle = task::lookup(0).map_or(0, |t| {
                t.cpu_ticks.load(core::sync::atomic::Ordering::Relaxed)
            });
            let idle_ms = idle * 1000 / crate::time::HZ;
            format!(
                "{}.{:02} {}.{:02}\n",
                ms / 1000,
                ms % 1000 / 10,
                idle_ms / 1000,
                idle_ms % 1000 / 10
            )
        }
        "meminfo" => {
            let (free, total) = crate::mm::frame::stats();
            let heap = crate::mm::heap::stats();
            format!(
                "MemTotal:     {:>10} kB\nMemFree:      {:>10} kB\nKernelHeap:   {:>10} kB\nKernelSlab:   {:>10} kB\n",
                total * 4,
                free * 4,
                heap.allocated / 1024,
                heap.slab_pages_bytes / 1024
            )
        }
        "version" => format!(
            "{} version {} (rustc) #1 x86_64\n",
            crate::NAME,
            crate::VERSION
        ),
        "mounts" => {
            let mut s = String::new();
            for (path, source, fstype) in mounts() {
                let _ = writeln!(s, "{} {} {} rw 0 0", source, path, fstype);
            }
            s
        }
        "kmsg" => String::from_utf8_lossy(&crate::klog::snapshot()).into_owned(),
        "cpuinfo" => crate::arch::cpu::cpuinfo(),
        "interrupts" => crate::arch::irq::interrupts_text(),
        "pci" => crate::drivers::pci::listing(),
        _ => return None,
    })
}

fn pid_file(pid: Pid, name: &str) -> Option<String> {
    let t = task::lookup(pid)?;
    let info = crate::proc::ps_info(&t);
    Some(match name {
        "cmdline" => {
            let mut s = info.cmdline.join("\0");
            s.push('\0');
            s
        }
        "stat" => format!(
            "{} ({}) {} {} {} {} {} {}\n",
            pid,
            t.name(),
            t.state().letter(),
            info.ppid,
            info.pgid,
            info.sid,
            t.cpu_ticks.load(core::sync::atomic::Ordering::Relaxed),
            info.vm_bytes
        ),
        "status" => format!(
            "Name:\t{}\nState:\t{}\nPid:\t{}\nPPid:\t{}\nPGid:\t{}\nVmSize:\t{} kB\nFDs:\t{}\n",
            t.name(),
            t.state().letter(),
            pid,
            info.ppid,
            info.pgid,
            info.vm_bytes / 1024,
            info.open_files
        ),
        _ => return None,
    })
}

struct ProcRoot {
    dev: u64,
}

struct PidDir {
    dev: u64,
    pid: Pid,
}

fn pid_dir_ino(pid: Pid) -> u64 {
    0x1_0000 + pid as u64 * 16
}

impl Inode for PidDir {
    fn metadata(&self) -> Metadata {
        Metadata::new(self.dev, pid_dir_ino(self.pid), FileType::Directory, 0o555)
    }

    fn lookup(&self, name: &str) -> KResult<Arc<dyn Inode>> {
        let idx = PID_FILES
            .iter()
            .position(|&f| f == name)
            .ok_or(Errno::ENOENT)?;
        task::lookup(self.pid).ok_or(Errno::ENOENT)?;
        let (pid, file) = (self.pid, PID_FILES[idx]);
        Ok(Arc::new(ProcFile {
            dev: self.dev,
            ino: pid_dir_ino(pid) + 1 + idx as u64,
            generate: Box::new(move || pid_file(pid, file).unwrap_or_default()),
        }))
    }

    fn readdir(&self) -> KResult<Vec<DirEntry>> {
        Ok(PID_FILES
            .iter()
            .enumerate()
            .map(|(i, n)| DirEntry {
                name: n.to_string(),
                ino: pid_dir_ino(self.pid) + 1 + i as u64,
                kind: FileType::Regular,
            })
            .collect())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Inode for ProcRoot {
    fn metadata(&self) -> Metadata {
        Metadata::new(self.dev, 1, FileType::Directory, 0o555)
    }

    fn lookup(&self, name: &str) -> KResult<Arc<dyn Inode>> {
        if let Some(i) = STATIC_FILES.iter().position(|&f| f == name) {
            let file = STATIC_FILES[i];
            return Ok(Arc::new(ProcFile {
                dev: self.dev,
                ino: 2 + i as u64,
                generate: Box::new(move || static_file(file).unwrap_or_default()),
            }));
        }
        let pid = if name == "self" {
            crate::task::sched::current_pid()
        } else {
            name.parse().map_err(|_| Errno::ENOENT)?
        };
        task::lookup(pid).ok_or(Errno::ENOENT)?;
        Ok(Arc::new(PidDir { dev: self.dev, pid }))
    }

    fn readdir(&self) -> KResult<Vec<DirEntry>> {
        let mut v: Vec<DirEntry> = STATIC_FILES
            .iter()
            .enumerate()
            .map(|(i, n)| DirEntry {
                name: n.to_string(),
                ino: 2 + i as u64,
                kind: FileType::Regular,
            })
            .collect();
        for t in task::all_tasks() {
            v.push(DirEntry {
                name: t.pid.to_string(),
                ino: pid_dir_ino(t.pid),
                kind: FileType::Directory,
            });
        }
        Ok(v)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct ProcFs {
    root: Arc<ProcRoot>,
}

impl FileSystem for ProcFs {
    fn name(&self) -> &'static str {
        "proc"
    }

    fn statfs(&self) -> FsStats {
        FsStats { magic: 0x9FA0, block_size: 4096, ..FsStats::default() }
    }

    fn root(&self) -> Arc<dyn Inode> {
        self.root.clone()
    }
}

pub fn new() -> Arc<ProcFs> {
    Arc::new(ProcFs {
        root: Arc::new(ProcRoot { dev: alloc_dev() }),
    })
}
