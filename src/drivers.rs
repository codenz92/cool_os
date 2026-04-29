extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

#[derive(Clone)]
pub struct DriverBinding {
    pub location: String,
    pub driver: &'static str,
    pub node: String,
    pub status: &'static str,
}

static BINDINGS: Mutex<Vec<DriverBinding>> = Mutex::new(Vec::new());

pub fn init() {
    refresh();
    let _ = create_device_nodes();
}

pub fn refresh() {
    let previous = BINDINGS.lock().len();
    let mut bindings = Vec::new();
    crate::pci::scan(|loc, hdr| {
        let Some(driver) = bind_driver(
            hdr.vendor_id,
            hdr.device_id,
            hdr.class,
            hdr.subclass,
            hdr.prog_if,
        ) else {
            return;
        };
        let node = node_for(driver, bindings.len());
        bindings.push(DriverBinding {
            location: format!("{:02x}:{:02x}.{}", loc.bus, loc.device, loc.function),
            driver,
            node,
            status: "bound",
        });
    });
    let current = bindings.len();
    *BINDINGS.lock() = bindings;
    if current != previous {
        crate::notifications::push("Driver registry", "PCI binding table changed");
        crate::event_bus::emit("drivers", "hotplug", "binding table changed");
    }
}

pub fn lines() -> Vec<String> {
    let bindings = BINDINGS.lock();
    if bindings.is_empty() {
        return alloc::vec![String::from("no PCI drivers bound")];
    }
    bindings
        .iter()
        .map(|binding| {
            format!(
                "{} {} node={} {}",
                binding.location, binding.driver, binding.node, binding.status
            )
        })
        .collect()
}

pub fn create_device_nodes() -> Result<(), crate::fat32::FsError> {
    let _ = crate::fat32::create_dir("/DEV");
    for binding in BINDINGS.lock().iter() {
        let _ = crate::fat32::create_file(&binding.node);
        let data = format!(
            "driver={}\nlocation={}\nstatus={}\n",
            binding.driver, binding.location, binding.status
        );
        let _ = crate::fat32::write_file(&binding.node, data.as_bytes());
    }
    Ok(())
}

fn bind_driver(
    vendor: u16,
    device: u16,
    class: u8,
    subclass: u8,
    prog_if: u8,
) -> Option<&'static str> {
    match (vendor, device, class, subclass, prog_if) {
        (0x8086, _, 0x02, _, _) => Some("e1000"),
        (0x1af4, _, 0x02, _, _) => Some("virtio-net"),
        (_, _, 0x01, 0x01, _) => Some("ata-pio"),
        (_, _, 0x03, _, _) => Some("vga"),
        (_, _, 0x0c, 0x03, 0x30) => Some("xhci"),
        (_, _, 0x06, _, _) => Some("pci-bridge"),
        _ => None,
    }
}

fn node_for(driver: &str, ordinal: usize) -> String {
    let prefix = match driver {
        "e1000" | "virtio-net" => "NET",
        "xhci" => "USB",
        "ata-pio" => "DISK",
        "vga" => "VIDEO",
        _ => "PCI",
    };
    format!("/DEV/{}{}.DEV", prefix, ordinal)
}
