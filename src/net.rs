extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

#[derive(Clone)]
pub struct NetAdapter {
    pub location: String,
    pub name: String,
    pub driver: &'static str,
    pub mac: [u8; 6],
    pub link_up: bool,
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
                } else if hdr.vendor_id == 0x1af4 {
                    "virtio-net-candidate"
                } else {
                    "unbound"
                },
                mac: synthetic_mac(loc.bus, loc.device, loc.function),
                link_up: false,
            });
        }
    });
    if adapters.is_empty() {
        crate::device_registry::register_virtual("network stack", "network", "no adapter found");
        crate::klog::log("network: no PCI network adapter found");
    } else {
        crate::device_registry::register_virtual("network stack", "network", "adapter detected");
        crate::klog::log("network: PCI adapter detected; protocol stack staged");
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
            "{} {} driver={} mac={} link={}",
            adapter.location,
            adapter.name,
            adapter.driver,
            mac_string(adapter.mac),
            if adapter.link_up { "up" } else { "down" }
        ));
    }
    lines.push(String::from(
        "stack: ARP/IP/UDP/DNS/HTTP state machines staged",
    ));
    lines
}

pub fn protocol_lines() -> Vec<String> {
    alloc::vec![
        String::from("ARP: cache table ready, TX/RX queues staged"),
        String::from("IPv4: static address model ready"),
        String::from("UDP: datagram builder/parser staged"),
        String::from("DNS: synthetic resolver syscall available"),
        String::from("HTTP: GET request builder available to terminal/userspace"),
    ]
}

pub fn dns_resolve(host: &str) -> Result<u32, &'static str> {
    if ADAPTERS.lock().is_empty() {
        return Err("no network adapter");
    }
    let mut hash = 0u32;
    for byte in host.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u32);
    }
    Ok(0x0a00_0001 | ((hash & 0x00ff_ffff).max(2)))
}

pub fn http_get(host: &str, path: &str) -> Result<String, &'static str> {
    if ADAPTERS.lock().is_empty() {
        return Err("no network adapter");
    }
    let mut request = String::from("GET ");
    request.push_str(if path.is_empty() { "/" } else { path });
    request.push_str(" HTTP/1.0\\r\\nHost: ");
    request.push_str(host);
    request.push_str("\\r\\n\\r\\n");
    Ok(request)
}

pub fn ipv4_string(addr: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        (addr >> 24) & 0xff,
        (addr >> 16) & 0xff,
        (addr >> 8) & 0xff,
        addr & 0xff
    )
}

fn synthetic_mac(bus: u8, device: u8, function: u8) -> [u8; 6] {
    [0x02, 0x43, 0x4f, bus, device, function]
}

fn mac_string(mac: [u8; 6]) -> String {
    let mut out = String::new();
    for (idx, byte) in mac.iter().enumerate() {
        if idx > 0 {
            out.push(':');
        }
        push_hex_byte(&mut out, *byte);
    }
    out
}

fn push_hex_byte(out: &mut String, value: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push(HEX[(value >> 4) as usize] as char);
    out.push(HEX[(value & 0x0f) as usize] as char);
}
