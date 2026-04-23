/// xHCI host controller driver — Phase 14 slice 1.
///
/// This first slice only **detects** the controller, reads its capability
/// registers, and logs what it found.  No reset, no rings, no device
/// enumeration yet — those land in slice 2.

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

pub fn init() {
    let Some((loc, hdr, mmio)) = find_controller() else {
        println!("[xhci] no controller found on PCI bus");
        return;
    };

    println!(
        "[xhci] {:04x}:{:02x}.{} vendor={:04x} device={:04x} mmio={:#x}",
        loc.bus, loc.device, loc.function, hdr.vendor_id, hdr.device_id, mmio,
    );

    // Enable memory decoding + bus mastering so we can read the MMIO block and
    // so the controller can later DMA into our rings.
    pci::enable_bus_master(loc);

    // The MMIO region lives in the "identity-mapped physical memory" window that
    // the bootloader set up via `physical_memory_offset`; we can address it
    // directly after adding the VMM's phys offset.
    let virt = crate::vmm::phys_to_virt(x86_64::PhysAddr::new(mmio)).as_u64();
    // The xHCI capability register at offset 0 packs CAPLENGTH in bits 0..7
    // and HCIVERSION in bits 16..31.  Some emulators don't honour a narrow
    // 8/16-bit MMIO access to this register, so read it as a single u32.
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

    println!(
        "[xhci] version=0x{:04x} caplength={} op_regs@{:#x}",
        version,
        caplength,
        virt + caplength as u64,
    );
    println!(
        "[xhci] slots={} interrupters={} ports={} scratchpads={} 64bit={}",
        max_slots, max_interrupters, max_ports, scratchpad_count, ac64,
    );
}

/// Find the first xHCI function on the PCI bus.  Returns its PCI location,
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
