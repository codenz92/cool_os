extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

#[derive(Clone)]
pub struct DeviceInfo {
    pub bus: &'static str,
    pub location: String,
    pub name: String,
    pub class_name: String,
    pub status: String,
}

static DEVICES: Mutex<Vec<DeviceInfo>> = Mutex::new(Vec::new());

pub fn refresh_pci() {
    let mut devices = Vec::new();
    crate::pci::scan(|loc, hdr| {
        let class_name = pci_class_name(hdr.class, hdr.subclass, hdr.prog_if);
        devices.push(DeviceInfo {
            bus: "PCI",
            location: format!("{:02x}:{:02x}.{}", loc.bus, loc.device, loc.function),
            name: format!("vendor {:04x} device {:04x}", hdr.vendor_id, hdr.device_id),
            class_name: String::from(class_name),
            status: String::from("present"),
        });
    });
    *DEVICES.lock() = devices;
}

pub fn set_usb_input(keyboard: bool, mouse: bool) {
    let mut devices = DEVICES.lock();
    upsert_virtual(
        &mut devices,
        "USB keyboard",
        if keyboard {
            "active"
        } else {
            "fallback disabled"
        },
    );
    upsert_virtual(
        &mut devices,
        "USB mouse",
        if mouse { "active" } else { "fallback disabled" },
    );
}

pub fn register_virtual(name: &str, class_name: &str, status: &str) {
    let mut devices = DEVICES.lock();
    if let Some(device) = devices
        .iter_mut()
        .find(|device| device.bus == "SYS" && device.name == name)
    {
        device.class_name = String::from(class_name);
        device.status = String::from(status);
        return;
    }
    devices.push(DeviceInfo {
        bus: "SYS",
        location: String::from("-"),
        name: String::from(name),
        class_name: String::from(class_name),
        status: String::from(status),
    });
}

#[allow(dead_code)]
pub fn list() -> Vec<DeviceInfo> {
    DEVICES.lock().clone()
}

pub fn lines() -> Vec<String> {
    DEVICES
        .lock()
        .iter()
        .map(|device| {
            format!(
                "{} {}  {}  {}  {}",
                device.bus, device.location, device.class_name, device.name, device.status
            )
        })
        .collect()
}

fn upsert_virtual(devices: &mut Vec<DeviceInfo>, name: &str, status: &str) {
    if let Some(device) = devices
        .iter_mut()
        .find(|device| device.bus == "USB" && device.name == name)
    {
        device.status = String::from(status);
        return;
    }
    devices.push(DeviceInfo {
        bus: "USB",
        location: String::from("runtime"),
        name: String::from(name),
        class_name: String::from("input"),
        status: String::from(status),
    });
}

fn pci_class_name(class: u8, subclass: u8, prog_if: u8) -> &'static str {
    match (class, subclass, prog_if) {
        (0x01, 0x01, _) => "storage/ide",
        (0x02, _, _) => "network",
        (0x03, _, _) => "display",
        (0x06, 0x00, _) => "bridge/host",
        (0x06, 0x01, _) => "bridge/isa",
        (0x06, 0x04, _) => "bridge/pci",
        (0x0c, 0x03, 0x30) => "usb/xhci",
        (0x0c, 0x03, _) => "usb",
        _ => "device",
    }
}
