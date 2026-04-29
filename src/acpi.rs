extern crate alloc;

use alloc::{string::String, vec::Vec};

pub fn init() {
    crate::device_registry::register_virtual("power manager", "ACPI", "legacy reboot only");
    crate::klog::log("acpi: tables not parsed; legacy reboot path active");
}

pub fn status_lines() -> Vec<String> {
    alloc::vec![
        String::from("ACPI: table parser not initialized"),
        String::from("shutdown: unavailable"),
        String::from("sleep: unavailable"),
        String::from("reboot: legacy controller reset available"),
    ]
}

pub fn shutdown() -> Result<(), &'static str> {
    Err("ACPI shutdown is not available yet")
}

pub fn sleep() -> Result<(), &'static str> {
    Err("ACPI sleep is not available yet")
}
