//! Processes: user address spaces, files, signals. (Filled in by later stages.)

pub mod signal;
pub mod uaccess;

use crate::task::{Pid, Task};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Switches page tables when the scheduler moves from `prev` to `next`.
pub fn switch_address_space(_prev: &Arc<Task>, _next: &Arc<Task>) {}

/// Process details shown by /proc.
#[derive(Default)]
pub struct PsInfo {
    pub ppid: Pid,
    pub pgid: Pid,
    pub sid: Pid,
    pub cmdline: Vec<String>,
    pub vm_bytes: u64,
    pub open_files: usize,
}

pub fn ps_info(task: &Task) -> PsInfo {
    PsInfo { cmdline: alloc::vec![task.name()], ..PsInfo::default() }
}
