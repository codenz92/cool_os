/// Minimal PCI configuration-space access via the legacy I/O ports 0xCF8/0xCFC.
///
/// Only what we need for Phase 14: brute-force scan every bus/device/function,
/// read class codes, pull BAR values. No MMCONFIG / ECAM support — coolOS runs
/// in QEMU where port-based access works for all emulated PCI devices.

use core::sync::atomic::Ordering;
use x86_64::instructions::port::Port;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

/// A fully-qualified PCI address (bus / device / function).
#[derive(Clone, Copy, Debug)]
pub struct Location {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl Location {
    const fn address(self, offset: u8) -> u32 {
        // Bit 31: enable.  Bits 23-16 bus, 15-11 device, 10-8 function, 7-2 offset.
        0x8000_0000
            | ((self.bus as u32) << 16)
            | ((self.device as u32) << 11)
            | ((self.function as u32) << 8)
            | ((offset as u32) & 0xFC)
    }
}

/// Read a 32-bit word from PCI configuration space.
pub fn read32(loc: Location, offset: u8) -> u32 {
    // PCI config reads aren't interruptible but can race with any other CPU
    // issuing its own config read; on our single-core QEMU target this is fine.
    let _ = Ordering::SeqCst;
    unsafe {
        Port::<u32>::new(CONFIG_ADDRESS).write(loc.address(offset));
        Port::<u32>::new(CONFIG_DATA).read()
    }
}

/// Write a 32-bit word to PCI configuration space.
pub fn write32(loc: Location, offset: u8, value: u32) {
    unsafe {
        Port::<u32>::new(CONFIG_ADDRESS).write(loc.address(offset));
        Port::<u32>::new(CONFIG_DATA).write(value);
    }
}

/// Parsed identifying fields from offset 0x00 / 0x08 of a PCI function header.
#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub revision: u8,
    pub header_type: u8,
}

impl Header {
    pub fn read(loc: Location) -> Option<Self> {
        let id = read32(loc, 0x00);
        let vendor_id = id as u16;
        if vendor_id == 0xFFFF {
            return None; // no function present
        }
        let device_id = (id >> 16) as u16;

        let class_word = read32(loc, 0x08);
        let revision = class_word as u8;
        let prog_if = (class_word >> 8) as u8;
        let subclass = (class_word >> 16) as u8;
        let class = (class_word >> 24) as u8;

        let header_type = ((read32(loc, 0x0C) >> 16) & 0xFF) as u8;

        Some(Header {
            vendor_id,
            device_id,
            class,
            subclass,
            prog_if,
            revision,
            header_type,
        })
    }
}

/// Resolve one of the six BAR slots of a type-0 header into a usable address.
/// Returns the physical base address of the BAR region, or `None` if the slot
/// is unused / unsupported.
///
/// Handles 32-bit memory BARs and 64-bit memory BARs (which consume two slots).
/// I/O-space BARs return `None`; we don't need them for xHCI.
pub fn bar(loc: Location, index: u8) -> Option<u64> {
    assert!(index < 6, "BAR index out of range");
    let offset = 0x10 + index * 4;
    let low = read32(loc, offset);
    if low & 0x1 != 0 {
        // I/O-space BAR — unused by MMIO-only devices like xHCI.
        return None;
    }
    let bar_type = (low >> 1) & 0x3;
    match bar_type {
        0x0 => Some((low & 0xFFFF_FFF0) as u64),
        0x2 => {
            // 64-bit memory BAR — high half lives in the next slot.
            if index >= 5 {
                return None;
            }
            let high = read32(loc, offset + 4) as u64;
            Some(((high << 32) | (low as u64)) & !0xFu64)
        }
        _ => None,
    }
}

/// Enable bus-mastering and memory decoding in the command register so the
/// device can DMA to host memory and respond to MMIO cycles.
pub fn enable_bus_master(loc: Location) {
    let cmd = read32(loc, 0x04);
    // Bit 1: memory space enable, bit 2: bus master enable.
    write32(loc, 0x04, cmd | 0x0006);
}

/// Iterate every function on every device on every bus once, invoking `f` with
/// the location and parsed header.
///
/// The brute-force 256×32×8 scan is fine on QEMU where there are only a handful
/// of devices; we do not attempt to honour multi-function header flags.
pub fn scan<F: FnMut(Location, Header)>(mut f: F) {
    for bus in 0..=255u8 {
        for device in 0..32u8 {
            for function in 0..8u8 {
                let loc = Location {
                    bus,
                    device,
                    function,
                };
                if let Some(hdr) = Header::read(loc) {
                    f(loc, hdr);
                }
            }
        }
    }
}
