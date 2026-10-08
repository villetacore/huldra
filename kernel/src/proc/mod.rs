//! Processes: user address spaces, files, signals. (Filled in by later stages.)

pub mod signal;

use crate::task::Task;
use alloc::sync::Arc;

/// Switches page tables when the scheduler moves from `prev` to `next`.
pub fn switch_address_space(_prev: &Arc<Task>, _next: &Arc<Task>) {}
