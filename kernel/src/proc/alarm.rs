//! `alarm(2)`: SIGALRM after a delay, checked on every timer tick.

use crate::sync::SpinLock;
use crate::task::{self, Pid, Task};
use alloc::collections::BTreeMap;

/// pid -> deadline in milliseconds of uptime.
static ALARMS: SpinLock<BTreeMap<Pid, u64>> = SpinLock::new(BTreeMap::new());

/// Arms (or with 0, cancels) the alarm; returns the milliseconds that were left.
pub fn set(task: &Task, ms: u64) -> u64 {
    let now = crate::time::uptime_ms();
    let mut alarms = ALARMS.lock();
    let left = alarms.remove(&task.tgid()).map_or(0, |d| d.saturating_sub(now));
    if ms > 0 {
        alarms.insert(task.tgid(), now + ms);
    }
    left
}

/// Called from the timer interrupt.
pub fn tick() {
    let now = crate::time::uptime_ms();
    let due: alloc::vec::Vec<Pid> = {
        let mut alarms = ALARMS.lock();
        if alarms.is_empty() {
            return;
        }
        let due: alloc::vec::Vec<Pid> = alarms.iter().filter(|(_, &d)| d <= now).map(|(&p, _)| p).collect();
        for p in &due {
            alarms.remove(p);
        }
        due
    };
    for pid in due {
        if let Some(t) = task::lookup(pid) {
            super::signal::send(&t, huldra_abi::signal::SIGALRM);
        }
    }
}
