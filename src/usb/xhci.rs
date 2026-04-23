/// xHCI host controller driver — Phase 14 slice 1.
///
/// This slice only **detects** the controller and reads its capability
/// registers. It does not reset or run the controller yet, because coolOS
/// still depends on the PS/2 input path for live keyboard and mouse support.
/// Boot-time xHCI bring-up resumes once the USB HID path exists.

extern crate alloc;

use alloc::vec::Vec;

use crate::pci::{self, Header, Location};
use crate::println;

/// PCI class code 0x0C (serial bus controller), subclass 0x03 (USB),
/// programming interface 0x30 (xHCI).
const PCI_CLASS_SERIAL: u8 = 0x0C;
const PCI_SUBCLASS_USB: u8 = 0x03;
const PCI_PROGIF_XHCI: u8 = 0x30;

/// Offsets inside the xHCI capability register block (all MMIO reads).
const CAP_HCSPARAMS1: u64 = 0x04; // u32 — slots, interrupters, ports
const CAP_HCSPARAMS2: u64 = 0x08; // u32 — IST, ERST max, scratchpad size
const CAP_HCCPARAMS1: u64 = 0x10; // u32 — addressing, extended caps
const OP_PORTSC_BASE: u64 = 0x400;

// xHCI extended capability IDs.
const EXT_CAP_LEGACY_SUPPORT: u8 = 1;
const EXT_CAP_SUPPORTED_PROTOCOL: u8 = 2;

struct SupportedProtocol {
    label: &'static str,
    major: u8,
    minor: u8,
    port_offset: u8,
    port_count: u8,
    psi_count: u8,
    slot_type: u8,
}

pub fn init() {
    let Some((loc, hdr, mmio)) = find_controller() else {
        println!("[xhci] no controller found on PCI bus");
        return;
    };

    println!(
        "[xhci] {:04x}:{:02x}.{} vendor={:04x} device={:04x} mmio={:#x}",
        loc.bus, loc.device, loc.function, hdr.vendor_id, hdr.device_id, mmio,
    );

    // Enable memory decoding so we can probe the capability registers.
    // Do not reset or run the controller yet; the PS/2 path still owns input.
    pci::enable_memory_space(loc);

    // The MMIO region lives in the physical-memory window mapped by the
    // bootloader, so we can read it directly after applying the phys offset.
    let virt = crate::vmm::phys_to_virt(x86_64::PhysAddr::new(mmio)).as_u64();
    let cap_word = unsafe { read_u32(virt) };
    let caplength = (cap_word & 0xFF) as u8;
    let version = (cap_word >> 16) as u16;
    let hcsparams1 = unsafe { read_u32(virt + CAP_HCSPARAMS1) };
    let hcsparams2 = unsafe { read_u32(virt + CAP_HCSPARAMS2) };
    let hccparams1 = unsafe { read_u32(virt + CAP_HCCPARAMS1) };

    let max_slots = (hcsparams1 & 0xFF) as u8;
    let max_interrupters = ((hcsparams1 >> 8) & 0x7FF) as u16;
    let max_ports = ((hcsparams1 >> 24) & 0xFF) as u8;
    let scratch_hi = (hcsparams2 >> 21) & 0x1F;
    let scratch_lo = (hcsparams2 >> 27) & 0x1F;
    let scratchpad_count = (scratch_hi << 5) | scratch_lo;
    let ac64 = hccparams1 & 0x1 != 0;
    let xecp = ((hccparams1 >> 16) & 0xFFFF) as u64 * 4;

    println!(
        "[xhci] version=0x{:04x} caplength={} op_regs@{:#x}",
        version,
        caplength,
        virt + caplength as u64,
    );
    println!(
        "[xhci] slots={} interrupters={} ports={} scratchpads={} 64bit={} xecp={:#x}",
        max_slots, max_interrupters, max_ports, scratchpad_count, ac64, xecp,
    );
    let protocols = scan_extended_caps(virt, xecp);
    scan_ports(virt + caplength as u64, max_ports, &protocols);
    println!("[xhci] passive probe only; controller bring-up disabled to preserve PS/2 input");
}

/// Find the first xHCI function on the PCI bus. Returns its PCI location,
/// parsed header, and the physical base address of its MMIO region.
fn find_controller() -> Option<(Location, Header, u64)> {
    let mut found: Option<(Location, Header, u64)> = None;
    pci::scan(|loc, hdr| {
        if found.is_some() {
            return;
        }
        if hdr.class == PCI_CLASS_SERIAL
            && hdr.subclass == PCI_SUBCLASS_USB
            && hdr.prog_if == PCI_PROGIF_XHCI
        {
            if let Some(base) = pci::bar(loc, 0) {
                found = Some((loc, hdr, base));
            }
        }
    });
    found
}

unsafe fn read_u32(addr: u64) -> u32 {
    core::ptr::read_volatile(addr as *const u32)
}

fn scan_extended_caps(base: u64, mut off: u64) -> Vec<SupportedProtocol> {
    let mut protocols = Vec::new();
    if off == 0 {
        println!("[xhci] no extended capabilities");
        return protocols;
    }

    for _ in 0..32 {
        let header = unsafe { read_u32(base + off) };
        let cap_id = (header & 0xFF) as u8;
        let next = ((header >> 8) & 0xFF) as u64 * 4;

        match cap_id {
            EXT_CAP_LEGACY_SUPPORT => {
                println!("[xhci] ext cap @+{:#x}: USB legacy support", off);
            }
            EXT_CAP_SUPPORTED_PROTOCOL => {
                protocols.push(log_supported_protocol(base, off, header));
            }
            0 => {
                println!("[xhci] ext cap @+{:#x}: invalid id=0", off);
            }
            _ => {
                println!("[xhci] ext cap @+{:#x}: id={} header={:#x}", off, cap_id, header);
            }
        }

        if next == 0 {
            return protocols;
        }
        off += next;
    }

    println!("[xhci] extended capability scan truncated");
    protocols
}

fn log_supported_protocol(base: u64, off: u64, header: u32) -> SupportedProtocol {
    let name = unsafe { read_u32(base + off + 0x04) };
    let ports = unsafe { read_u32(base + off + 0x08) };
    let slot = unsafe { read_u32(base + off + 0x0C) };

    let rev_minor = ((header >> 16) & 0xFF) as u8;
    let rev_major = ((header >> 24) & 0xFF) as u8;
    let port_offset = (ports & 0xFF) as u8;
    let port_count = ((ports >> 8) & 0xFF) as u8;
    let psi_count = ((ports >> 28) & 0xF) as u8;
    let slot_type = (slot & 0x1F) as u8;
    let name_str = protocol_name(name);
    let label = protocol_label(name_str, rev_major, rev_minor);
    let port_last = port_offset.saturating_add(port_count.saturating_sub(1));

    println!(
        "[xhci] ext cap @+{:#x}: supported protocol {} rev={}.{} ports={}..{} psic={} slot_type={}",
        off,
        label,
        rev_major,
        bcd_hex(rev_minor),
        port_offset,
        port_last,
        psi_count,
        slot_type,
    );

    for idx in 0..psi_count {
        let psi = unsafe { read_u32(base + off + 0x10 + idx as u64 * 4) };
        log_psi(idx, psi);
    }

    SupportedProtocol {
        label,
        major: rev_major,
        minor: rev_minor,
        port_offset,
        port_count,
        psi_count,
        slot_type,
    }
}

fn scan_ports(op_base: u64, max_ports: u8, protocols: &[SupportedProtocol]) {
    let mut any = false;
    for port in 0..max_ports {
        let port_num = port + 1;
        let portsc = unsafe { read_u32(op_base + OP_PORTSC_BASE + 0x10 * port as u64) };
        let connected = portsc & 0x1 != 0;
        let enabled = portsc & 0x2 != 0;
        let speed_id = ((portsc >> 10) & 0xF) as u8;

        if !connected && !enabled {
            continue;
        }

        any = true;
        if let Some(proto) = protocol_for_port(protocols, port_num) {
            println!(
                "[xhci] port {} proto={} rev={}.{} slot_type={} ccs={} ped={} speed_id={} speed={} portsc={:#x}",
                port_num,
                proto.label,
                proto.major,
                bcd_hex(proto.minor),
                proto.slot_type,
                connected as u8,
                enabled as u8,
                speed_id,
                port_speed_name(proto, speed_id),
                portsc,
            );
        } else {
            println!(
                "[xhci] port {} proto=? ccs={} ped={} speed_id={} portsc={:#x}",
                port_num,
                connected as u8,
                enabled as u8,
                speed_id,
                portsc,
            );
        }
    }

    if !any {
        println!("[xhci] no active root-hub ports reported");
    }
}

fn protocol_for_port(
    protocols: &[SupportedProtocol],
    port_num: u8,
) -> Option<&SupportedProtocol> {
    protocols.iter().find(|proto| {
        let start = proto.port_offset;
        let end = proto.port_offset.saturating_add(proto.port_count.saturating_sub(1));
        port_num >= start && port_num <= end
    })
}

fn port_speed_name(proto: &SupportedProtocol, speed_id: u8) -> &'static str {
    if proto.psi_count != 0 {
        return "psi";
    }

    match (proto.major, proto.minor, speed_id) {
        (2, _, 1) => "Full",
        (2, _, 2) => "Low",
        (2, _, 3) => "High",
        (3, 0x00, 4) => "Super",
        (3, 0x10, 4) => "Super",
        (3, 0x10, 5) => "Super+",
        (3, 0x20, 4) => "Super",
        (3, 0x20, 5) => "Super+ Gen2x1",
        (3, 0x20, 6) => "Super+ Gen1x2",
        (3, 0x20, 7) => "Super+ Gen2x2",
        _ => "?",
    }
}

fn protocol_name(raw: u32) -> [u8; 4] {
    raw.to_le_bytes()
}

fn protocol_label(name: [u8; 4], major: u8, minor: u8) -> &'static str {
    if name == *b"USB " {
        match (major, minor) {
            (2, 0x00) => "USB 2.0",
            (3, 0x00) => "USB 3.0",
            (3, 0x10) => "USB 3.1",
            (3, 0x20) => "USB 3.2",
            _ => "USB",
        }
    } else {
        "unknown"
    }
}

fn bcd_hex(v: u8) -> u8 {
    ((v >> 4) * 10) + (v & 0x0F)
}

fn log_psi(idx: u8, psi: u32) {
    let psiv = (psi & 0x0F) as u8;
    let psie = ((psi >> 4) & 0x03) as u8;
    let plt = ((psi >> 6) & 0x03) as u8;
    let full_duplex = ((psi >> 8) & 0x01) != 0;
    let lp = ((psi >> 14) & 0x03) as u8;
    let psim = ((psi >> 16) & 0xFFFF) as u16;

    println!(
        "[xhci]   psi{}: id={} rate={} {} kind={} duplex={} link={} raw={:#x}",
        idx,
        psiv,
        psim,
        psi_units(psie),
        psi_type(plt),
        if full_duplex { "full" } else { "half" },
        link_protocol(lp),
        psi,
    );
}

fn psi_units(psie: u8) -> &'static str {
    match psie {
        0 => "b/s",
        1 => "Kb/s",
        2 => "Mb/s",
        3 => "Gb/s",
        _ => "?",
    }
}

fn psi_type(plt: u8) -> &'static str {
    match plt {
        0 => "sym",
        2 => "rx",
        3 => "tx",
        _ => "?",
    }
}

fn link_protocol(lp: u8) -> &'static str {
    match lp {
        0 => "SS",
        1 => "SSP",
        _ => "?",
    }
}
