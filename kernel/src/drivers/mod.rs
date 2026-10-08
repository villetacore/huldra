pub mod keyboard;
pub mod rtc;
pub mod serial;
pub mod tty;
pub mod vga;

/// Registers interrupt handlers for the built-in devices.
pub fn init() {
    crate::arch::irq::register(1, "keyboard", keyboard::handle_irq);
    crate::arch::irq::register(4, "serial", serial::handle_irq);
}
