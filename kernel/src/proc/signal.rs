//! Signals.

use crate::task::Pid;

/// True if the current task has a deliverable signal pending.
pub fn current_has_pending() -> bool {
    false
}

/// Sends `sig` to every process in process group `pgrp`.
pub fn send_to_group(_pgrp: Pid, _sig: u32) {}

/// Sends `sig` to the current process.
pub fn send_current(_sig: u32) {}
