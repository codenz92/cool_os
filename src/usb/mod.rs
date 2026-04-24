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
use crate::println;
use spin::Mutex;

pub mod xhci;

static USB_STATUS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static LAST_INPUT_PRESENCE: Mutex<Option<(bool, bool)>> = Mutex::new(None);

pub fn status_lines() -> Vec<String> {
    let mut lines = USB_STATUS.lock().clone();
    lines.extend(xhci::runtime_status_lines());
    lines
}

pub fn input_presence() -> (bool, bool) {
    xhci::runtime_input_presence()
}

pub fn reconcile_input_fallbacks() {
    let current = input_presence();
    let mut last = LAST_INPUT_PRESENCE.lock();
    let previous = *last;
    if previous == Some(current) {
        return;
    }

    let (usb_keyboard, usb_mouse) = current;
    if previous.map(|(prev_keyboard, _)| prev_keyboard) != Some(usb_keyboard) {
        if usb_keyboard {
            println!("[input] USB keyboard detected; PS/2 keyboard fallback disabled");
            crate::keyboard::disable_ps2_fallback();
        } else {
            println!("[input] no USB keyboard detected; enabling PS/2 keyboard fallback");
            crate::keyboard::enable_ps2_fallback();
        }
    }

    if previous.map(|(_, prev_mouse)| prev_mouse) != Some(usb_mouse) {
        if usb_mouse {
            println!("[input] USB mouse detected; PS/2 mouse fallback disabled");
            crate::mouse::disable_ps2_fallback();
        } else {
            println!("[input] no USB mouse detected; enabling PS/2 mouse fallback");
            crate::mouse::enable_ps2_fallback();
        }
    }

    *last = Some(current);
}

/// Called once from `kernel_main` after the VMM and interrupt controller are up.
/// Logs a line on success; silently does nothing if no xHCI controller is present
/// (the PS/2 path still owns input).
pub fn init() {
    *USB_STATUS.lock() = xhci::probe();
    reconcile_input_fallbacks();
}

pub fn poll() {
    xhci::poll();
    reconcile_input_fallbacks();
}
