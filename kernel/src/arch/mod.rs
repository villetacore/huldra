//! Architecture layer.
//!
//! Generic kernel code only uses what this module re-exports:
//! interrupt control (`enable_interrupts`, `irq_save`, ...), `TrapFrame`,
//! paging (`paging::PageTable`, `PteFlags`), CPU control (`cpu`), timers and
//! the machine control functions (`reboot`, `poweroff`). A port to another
//! architecture provides the same items under `arch/<name>/`.

mod x86_64;

pub use self::x86_64::*;
