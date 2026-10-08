//! PCI bus enumeration (configuration mechanism #1).

use crate::arch::port::{inl, outl};
use crate::sync::SpinLock;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug)]
pub struct Device {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub vendor: u16,
    pub device: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub irq_line: u8,
}

static DEVICES: SpinLock<Vec<Device>> = SpinLock::new(Vec::new());

pub fn read32(bus: u8, slot: u8, function: u8, offset: u8) -> u32 {
    let address = 0x8000_0000 | (bus as u32) << 16 | (slot as u32) << 11 | (function as u32) << 8 | (offset as u32 & 0xFC);
    unsafe {
        outl(0xCF8, address);
        inl(0xCFC)
    }
}

pub fn class_name(class: u8, subclass: u8) -> &'static str {
    match (class, subclass) {
        (0x01, 0x01) => "IDE controller",
        (0x01, 0x06) => "SATA controller",
        (0x01, 0x08) => "NVMe controller",
        (0x01, _) => "Mass storage controller",
        (0x02, 0x00) => "Ethernet controller",
        (0x02, _) => "Network controller",
        (0x03, 0x00) => "VGA compatible controller",
        (0x03, _) => "Display controller",
        (0x04, _) => "Multimedia controller",
        (0x06, 0x00) => "Host bridge",
        (0x06, 0x01) => "ISA bridge",
        (0x06, 0x04) => "PCI bridge",
        (0x06, 0x80) => "Bridge",
        (0x06, _) => "Bridge device",
        (0x0C, 0x03) => "USB controller",
        (0x0C, 0x05) => "SMBus",
        (0x0C, _) => "Serial bus controller",
        (0xFF, _) => "Unassigned class",
        _ => "Unknown device",
    }
}

fn vendor_name(vendor: u16) -> &'static str {
    match vendor {
        0x8086 => "Intel",
        0x1234 => "QEMU",
        0x1AF4 => "Red Hat (virtio)",
        0x1022 => "AMD",
        0x10DE => "NVIDIA",
        0x10EC => "Realtek",
        0x15AD => "VMware",
        0x80EE => "VirtualBox",
        _ => "",
    }
}

pub fn scan() {
    let mut found = Vec::new();
    for bus in 0..=255u8 {
        for slot in 0..32u8 {
            let id = read32(bus, slot, 0, 0);
            if id & 0xFFFF == 0xFFFF {
                continue;
            }
            let multi = read32(bus, slot, 0, 0x0C) >> 16 & 0x80 != 0;
            for function in 0..if multi { 8 } else { 1 } {
                let id = read32(bus, slot, function, 0);
                if id & 0xFFFF == 0xFFFF {
                    continue;
                }
                let class = read32(bus, slot, function, 0x08);
                let irq = read32(bus, slot, function, 0x3C);
                found.push(Device {
                    bus,
                    slot,
                    function,
                    vendor: id as u16,
                    device: (id >> 16) as u16,
                    class: (class >> 24) as u8,
                    subclass: (class >> 16) as u8,
                    prog_if: (class >> 8) as u8,
                    irq_line: irq as u8,
                });
            }
        }
    }
    kinfo!("pci: {} devices", found.len());
    *DEVICES.lock() = found;
}

pub fn devices() -> Vec<Device> {
    DEVICES.lock().clone()
}

/// Text for /proc/pci.
pub fn listing() -> String {
    let mut s = String::new();
    for d in devices() {
        let vendor = vendor_name(d.vendor);
        s.push_str(&format!(
            "{:02x}:{:02x}.{} {} [{:02x}{:02x}]: {}{}[{:04x}:{:04x}]\n",
            d.bus,
            d.slot,
            d.function,
            class_name(d.class, d.subclass),
            d.class,
            d.subclass,
            vendor,
            if vendor.is_empty() { "" } else { " " },
            d.vendor,
            d.device
        ));
    }
    s
}
