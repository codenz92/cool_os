/// USB subsystem (Phase 14).
///
/// Entry point for the xHCI host-controller driver.  Boot-time flow:
///
///   1. `usb::init()` scans PCI for a class-0C/subclass-03/prog-if-0x30 device
///      (xHCI).
///   2. If found, the capability registers (version, structural parameters) are
///      probed and logged.
///   3. Later phases: controller reset, command/event ring setup, device
///      enumeration, HID class driver.

pub mod xhci;

/// Called once from `kernel_main` after the VMM and interrupt controller are up.
/// Logs a line on success; silently does nothing if no xHCI controller is present
/// (the PS/2 path still owns input).
pub fn init() {
    xhci::init();
}
