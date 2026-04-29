extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

#[derive(Clone)]
pub struct NetAdapter {
    pub location: String,
    pub name: String,
    pub driver: &'static str,
}

static ADAPTERS: Mutex<Vec<NetAdapter>> = Mutex::new(Vec::new());

pub fn init() {
    let mut adapters = Vec::new();
    crate::pci::scan(|loc, hdr| {
        if hdr.class == 0x02 {
            adapters.push(NetAdapter {
                location: format!("{:02x}:{:02x}.{}", loc.bus, loc.device, loc.function),
                name: format!("vendor {:04x} device {:04x}", hdr.vendor_id, hdr.device_id),
                driver: if hdr.vendor_id == 0x8086 {
                    "e1000-candidate"
                } else {
                    "unbound"
                },
            });
        }
    });
    if adapters.is_empty() {
        crate::device_registry::register_virtual("network stack", "network", "no adapter found");
        crate::klog::log("network: no PCI network adapter found");
    } else {
        crate::device_registry::register_virtual("network stack", "network", "adapter detected");
        crate::klog::log("network: PCI adapter detected; IP stack not yet online");
    }
    *ADAPTERS.lock() = adapters;
}

pub fn status_lines() -> Vec<String> {
    let adapters = ADAPTERS.lock();
    if adapters.is_empty() {
        return alloc::vec![
            String::from("network: no PCI adapter detected"),
            String::from("stack: driver probe foundation only; TCP/IP offline"),
        ];
    }
    let mut lines = Vec::new();
    for adapter in adapters.iter() {
        lines.push(format!(
            "{} {} driver={}",
            adapter.location, adapter.name, adapter.driver
        ));
    }
    lines.push(String::from("stack: ARP/IP/UDP/DNS/HTTP offline"));
    lines
}
