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
pub struct HttpResponse {
    pub host: String,
    pub path: String,
    pub resolved_addr: u32,
    pub request: String,
    pub status_line: String,
    pub body: String,
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
        let settings = crate::settings_state::snapshot();
        return alloc::vec![
            String::from("network: no PCI adapter detected"),
            format!(
                "stack: offline_api={} dns={} http={}",
                if settings.network_offline_api {
                    "on"
                } else {
                    "off"
                },
                if settings.network_dns_enabled {
                    "on"
                } else {
                    "off"
                },
                if settings.network_http_enabled {
                    "on"
                } else {
                    "off"
                }
            ),
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
    let settings = crate::settings_state::snapshot();
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
        format!(
            "DNS: resolver syscall {}",
            if settings.network_dns_enabled {
                "enabled"
            } else {
                "disabled"
            }
        ),
        format!(
            "HTTP: userspace client API {}",
            if settings.network_http_enabled {
                "enabled"
            } else {
                "disabled"
            }
        ),
        format!(
            "Offline API: {}",
            if settings.network_offline_api {
                "synthetic DNS/HTTP allowed without NIC"
            } else {
                "requires detected adapter"
            }
        ),
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
        crate::profiler::record("net-drop", kind, &format!("{} bytes", bytes));
        return;
    }
    let mut state = NET_STATE.lock();
    state.tx_tail = (state.tx_tail + 1) % 64;
    state.tx_packets = state.tx_packets.saturating_add(1);
    crate::profiler::record("net-tx", kind, &format!("{} bytes", bytes));
}

#[allow(dead_code)]
pub fn udp_send(_dst: u32, _port: u16, payload: &[u8]) -> Result<usize, &'static str> {
    if !network_available_for_api() {
        return Err("no network adapter");
    }
    queue_tx_packet("udp", payload.len());
    Ok(payload.len())
}

pub fn dns_resolve(host: &str) -> Result<u32, &'static str> {
    let settings = crate::settings_state::snapshot();
    if !settings.network_dns_enabled {
        return Err("DNS API disabled in Settings");
    }
    if !network_available_for_api() {
        return Err("no network adapter");
    }
    let host = host.trim();
    if host.is_empty() || host.len() > 253 || host.contains('/') || host.contains(' ') {
        return Err("invalid host");
    }
    let mut hash = 0u32;
    for byte in host.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u32);
    }
    queue_tx_packet("dns", host.len());
    Ok(0x0a00_0001 | ((hash & 0x00ff_ffff).max(2)))
}

pub fn http_get(host: &str, path: &str) -> Result<String, &'static str> {
    http_get_response(host, path).map(|response| response.request)
}

pub fn http_get_response(host: &str, path: &str) -> Result<HttpResponse, &'static str> {
    let settings = crate::settings_state::snapshot();
    if !settings.network_http_enabled {
        return Err("HTTP API disabled in Settings");
    }
    if !network_available_for_api() {
        return Err("no network adapter");
    }
    let host = host.trim();
    if host.is_empty() || host.len() > 253 || host.contains('/') || host.contains(' ') {
        return Err("invalid host");
    }
    let resolved_addr = dns_resolve(host)?;
    let mut request = String::from("GET ");
    request.push_str(if path.is_empty() { "/" } else { path });
    request.push_str(" HTTP/1.0\\r\\nHost: ");
    request.push_str(host);
    request.push_str("\\r\\n\\r\\n");
    queue_tx_packet("http", request.len());
    let mut body = String::from("coolOS synthetic HTTP response from ");
    body.push_str(host);
    body.push_str(" at ");
    body.push_str(&ipv4_string(resolved_addr));
    Ok(HttpResponse {
        host: String::from(host),
        path: String::from(if path.is_empty() { "/" } else { path }),
        resolved_addr,
        request,
        status_line: String::from("HTTP/1.0 200 OK (synthetic)"),
        body,
    })
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

fn network_available_for_api() -> bool {
    !ADAPTERS.lock().is_empty() || crate::settings_state::snapshot().network_offline_api
}

fn push_hex_byte(out: &mut String, value: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push(HEX[(value >> 4) as usize] as char);
    out.push(HEX[(value & 0x0f) as usize] as char);
}
