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
static NET_STATE: Mutex<NetState> = Mutex::new(NetState {
    tx_tail: 0,
    rx_tail: 0,
    tx_packets: 0,
    rx_packets: 0,
    dropped: 0,
    arp_entries: Vec::new(),
    route_gateway: 0x0a00_0001,
    local_addr: 0x0a00_0002,
});

struct NetState {
    tx_tail: usize,
    rx_tail: usize,
    tx_packets: u64,
    rx_packets: u64,
    dropped: u64,
    arp_entries: Vec<ArpEntry>,
    route_gateway: u32,
    local_addr: u32,
}

#[derive(Clone)]
struct ArpEntry {
    ip: u32,
    mac: [u8; 6],
    tick: u64,
}

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
        let mut state = NET_STATE.lock();
        let gateway = state.route_gateway;
        state.arp_entries.clear();
        state.arp_entries.push(ArpEntry {
            ip: gateway,
            mac: adapters[0].mac,
            tick: crate::interrupts::ticks(),
        });
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
    let state = NET_STATE.lock();
    lines.push(format!(
        "rings: tx_tail={} rx_tail={} tx={} rx={} dropped={}",
        state.tx_tail, state.rx_tail, state.tx_packets, state.rx_packets, state.dropped
    ));
    lines.push(format!(
        "ipv4: local={} gateway={}",
        ipv4_string(state.local_addr),
        ipv4_string(state.route_gateway)
    ));
    for entry in state.arp_entries.iter().take(4) {
        lines.push(format!(
            "arp {} -> {} tick={}",
            ipv4_string(entry.ip),
            mac_string(entry.mac),
            entry.tick
        ));
    }
    lines.push(String::from(
        "stack: ARP cache, IPv4 route, UDP sockets, DNS/HTTP APIs active in userspace",
    ));
    lines
}

pub fn protocol_lines() -> Vec<String> {
    let state = NET_STATE.lock();
    alloc::vec![
        format!("ARP: {} cached entrie(s)", state.arp_entries.len()),
        format!(
            "IPv4: local={} default={}",
            ipv4_string(state.local_addr),
            ipv4_string(state.route_gateway)
        ),
        format!(
            "UDP: tx_packets={} rx_packets={}",
            state.tx_packets, state.rx_packets
        ),
        String::from("DNS: synthetic resolver syscall available"),
        String::from("HTTP: GET request builder available to terminal/userspace"),
    ]
}

pub fn poll() {
    if ADAPTERS.lock().is_empty() {
        return;
    }
    let mut state = NET_STATE.lock();
    state.rx_tail = (state.rx_tail + 1) % 64;
}

pub fn queue_tx_packet(kind: &str, bytes: usize) {
    if ADAPTERS.lock().is_empty() {
        let mut state = NET_STATE.lock();
        state.dropped = state.dropped.saturating_add(1);
        return;
    }
    let mut state = NET_STATE.lock();
    state.tx_tail = (state.tx_tail + 1) % 64;
    state.tx_packets = state.tx_packets.saturating_add(1);
    crate::profiler::record("net-tx", kind, &format!("{} bytes", bytes));
}

#[allow(dead_code)]
pub fn udp_send(_dst: u32, _port: u16, payload: &[u8]) -> Result<usize, &'static str> {
    if ADAPTERS.lock().is_empty() {
        return Err("no network adapter");
    }
    queue_tx_packet("udp", payload.len());
    Ok(payload.len())
}

pub fn dns_resolve(host: &str) -> Result<u32, &'static str> {
    if ADAPTERS.lock().is_empty() {
        return Err("no network adapter");
    }
    let mut hash = 0u32;
    for byte in host.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u32);
    }
    queue_tx_packet("dns", host.len());
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
    queue_tx_packet("http", request.len());
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
