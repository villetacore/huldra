pub mod ata;
pub mod block;
pub mod fb;
pub mod input;
pub mod keyboard;
pub mod pci;
pub mod pty;
pub mod rtc;
pub mod serial;
pub mod tty;
pub mod vga;

/// Registers interrupt handlers for the built-in devices.
pub fn init() {
    crate::arch::irq::register(1, "keyboard", keyboard::handle_irq);
    crate::arch::irq::register(4, "serial", serial::handle_irq);
    pci::scan();
    ata::init();
    fb::init();
    input::init();
    pty::init();
}
