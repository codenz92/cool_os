/// USB subsystem (Phase 14).
///
/// Entry point for the xHCI host-controller driver.  Boot-time flow:
///
///   1. `usb::init()` scans PCI for a class-0C/subclass-03/prog-if-0x30 device
///      (xHCI).
///   2. If found, the capability registers (version, structural parameters) are
///      probed and logged.
///   3. The latest probe summary is retained so apps/commands can inspect USB
///      state without relying on the host terminal.
///   4. Later phases: controller reset, command/event ring setup, device
///      enumeration, HID class driver.

extern crate alloc;

use alloc::{string::String, vec::Vec};
use spin::Mutex;

pub mod xhci;

static USB_STATUS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn status_lines() -> Vec<String> {
    let mut lines = USB_STATUS.lock().clone();
    lines.extend(xhci::runtime_status_lines());
    lines
}

pub fn input_presence() -> (bool, bool) {
    xhci::runtime_input_presence()
}

/// Called once from `kernel_main` after the VMM and interrupt controller are up.
/// Logs a line on success; silently does nothing if no xHCI controller is present
/// (the PS/2 path still owns input).
pub fn init() {
    *USB_STATUS.lock() = xhci::probe();
}

pub fn poll() {
    xhci::poll();
}
