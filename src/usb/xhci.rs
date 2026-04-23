/// xHCI host controller driver — Phase 14 slices 1–4.
///
/// Default boot stays on a passive probe so the existing PS/2 input path
/// remains stable. An opt-in build (`COOLOS_XHCI_ACTIVE_INIT=1`) exercises the
/// real host-controller bring-up sequence: ownership handoff, reset, DCBAA,
/// command ring, event ring, and controller run.
extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{fence, Ordering};

use crate::pci::{self, Header, Location};
use crate::println;

const PCI_CLASS_SERIAL: u8 = 0x0C;
const PCI_SUBCLASS_USB: u8 = 0x03;
const PCI_PROGIF_XHCI: u8 = 0x30;

const CAP_HCSPARAMS1: u64 = 0x04;
const CAP_HCSPARAMS2: u64 = 0x08;
const CAP_HCCPARAMS1: u64 = 0x10;
const CAP_DBOFF: u64 = 0x14;
const CAP_RTSOFF: u64 = 0x18;

const OP_USBCMD: u64 = 0x00;
const OP_USBSTS: u64 = 0x04;
const OP_PAGESIZE: u64 = 0x08;
const OP_CRCR: u64 = 0x18;
const OP_DCBAAP: u64 = 0x30;
const OP_CONFIG: u64 = 0x38;
const OP_PORTSC_BASE: u64 = 0x400;

const RT_IR0: u64 = 0x20;
const IR0_ERSTSZ: u64 = 0x08;
const IR0_ERSTBA: u64 = 0x10;
const IR0_ERDP: u64 = 0x18;

const USBCMD_RS: u32 = 1 << 0;
const USBCMD_HCRST: u32 = 1 << 1;

const USBSTS_HCH: u32 = 1 << 0;
const USBSTS_CNR: u32 = 1 << 11;

const PORTSC_CCS: u32 = 1 << 0;
const PORTSC_PED: u32 = 1 << 1;

const EXT_CAP_LEGACY_SUPPORT: u8 = 1;
const EXT_CAP_SUPPORTED_PROTOCOL: u8 = 2;
const EXT_CAP_EXT_POWER_MGMT: u8 = 3;
const EXT_CAP_IO_VIRT: u8 = 4;
const EXT_CAP_MSG_INTERRUPT: u8 = 5;
const EXT_CAP_USB_DEBUG: u8 = 10;
const EXT_CAP_EXT_MSG_INTERRUPT: u8 = 17;

const COMMAND_RING_TRBS: usize = 256;
const EVENT_RING_TRBS: usize = 256;
const TRB_TYPE_LINK: u32 = 6;
const TRB_TYPE_NOOP_CMD: u32 = 23;
const TRB_TYPE_CMD_COMPLETION: u32 = 33;
const TRB_TYPE_PORT_STATUS_CHANGE: u32 = 34;
const TRB_TC: u32 = 1 << 1;
const TRB_CYCLE: u32 = 1 << 0;
const COMPLETION_SUCCESS: u8 = 1;
const ERDP_EHB_CLEAR: u64 = 1 << 3;

const ACTIVE_INIT: bool = option_env!("COOLOS_XHCI_ACTIVE_INIT").is_some();
const SPIN_TIMEOUT: u64 = 10_000_000;

struct LegacySupport {
    off: u64,
}

struct ProtocolSpeedId {
    psiv: u8,
    psie: u8,
    plt: u8,
    pfd: bool,
    lp: u8,
    psim: u16,
}

struct SupportedProtocol {
    label: &'static str,
    major: u8,
    minor: u8,
    port_offset: u8,
    port_count: u8,
    psi_count: u8,
    slot_type: u8,
    psis: Vec<ProtocolSpeedId>,
}

struct XhciInfo {
    mmio_virt: u64,
    caplength: u8,
    version: u16,
    max_slots: u8,
    max_interrupters: u16,
    max_ports: u8,
    scratchpad_count: u32,
    ac64: bool,
    xecp: u64,
    op_base: u64,
    rt_base: u64,
    db_base: u64,
    legacy: Option<LegacySupport>,
    protocols: Vec<SupportedProtocol>,
}

struct ActiveState {
    dcbaa_phys: u64,
    cmd_ring_phys: u64,
    event_ring_phys: u64,
    erst_phys: u64,
}

struct CommandRingState {
    phys: u64,
    virt: u64,
    enqueue_idx: usize,
    cycle: bool,
}

struct EventRingState {
    phys: u64,
    virt: u64,
    dequeue_idx: usize,
    cycle: bool,
}

struct EventTrb {
    parameter: u64,
    status: u32,
    control: u32,
}

impl EventTrb {
    fn trb_type(&self) -> u8 {
        ((self.control >> 10) & 0x3F) as u8
    }

    fn completion_code(&self) -> u8 {
        (self.status >> 24) as u8
    }

    fn slot_id(&self) -> u8 {
        (self.control >> 24) as u8
    }
}

struct CommandCompletion {
    ptr: u64,
    completion_code: u8,
    slot_id: u8,
}

pub fn init() {
    let Some((loc, hdr, mmio_phys)) = find_controller() else {
        println!("[xhci] no controller found on PCI bus");
        return;
    };

    println!(
        "[xhci] {:04x}:{:02x}.{} vendor={:04x} device={:04x} mmio={:#x}",
        loc.bus, loc.device, loc.function, hdr.vendor_id, hdr.device_id, mmio_phys,
    );

    if ACTIVE_INIT {
        pci::enable_bus_master(loc);
        println!("[xhci] active init enabled");
    } else {
        pci::enable_memory_space(loc);
    }

    let mmio_virt = crate::vmm::phys_to_virt(x86_64::PhysAddr::new(mmio_phys)).as_u64();
    let info = read_info(mmio_virt);

    println!(
        "[xhci] version=0x{:04x} caplength={} op={:#x} rt={:#x} db={:#x}",
        info.version, info.caplength, info.op_base, info.rt_base, info.db_base,
    );
    println!(
        "[xhci] slots={} interrupters={} ports={} scratchpads={} 64bit={} xecp={:#x}",
        info.max_slots,
        info.max_interrupters,
        info.max_ports,
        info.scratchpad_count,
        info.ac64,
        info.xecp,
    );

    if ACTIVE_INIT {
        match active_init(&info) {
            Ok(state) => {
                println!(
                    "[xhci] active init ready dcbaa={:#x} cmd={:#x} evt={:#x} erst={:#x}",
                    state.dcbaa_phys, state.cmd_ring_phys, state.event_ring_phys, state.erst_phys,
                );
            }
            Err(err) => {
                println!("[xhci] active init failed: {}", err);
            }
        }
    }

    scan_ports(&info);
    if !ACTIVE_INIT {
        println!("[xhci] passive probe only; controller bring-up disabled to preserve PS/2 input");
    }
}

fn read_info(mmio_virt: u64) -> XhciInfo {
    let cap_word = unsafe { read_u32(mmio_virt) };
    let caplength = (cap_word & 0xFF) as u8;
    let version = (cap_word >> 16) as u16;
    let hcsparams1 = unsafe { read_u32(mmio_virt + CAP_HCSPARAMS1) };
    let hcsparams2 = unsafe { read_u32(mmio_virt + CAP_HCSPARAMS2) };
    let hccparams1 = unsafe { read_u32(mmio_virt + CAP_HCCPARAMS1) };
    let xecp = ((hccparams1 >> 16) & 0xFFFF) as u64 * 4;
    let op_base = mmio_virt + caplength as u64;
    let rt_base = mmio_virt + (unsafe { read_u32(mmio_virt + CAP_RTSOFF) } as u64 & !0x1F);
    let db_base = mmio_virt + (unsafe { read_u32(mmio_virt + CAP_DBOFF) } as u64 & !0x3);

    let max_slots = (hcsparams1 & 0xFF) as u8;
    let max_interrupters = ((hcsparams1 >> 8) & 0x7FF) as u16;
    let max_ports = ((hcsparams1 >> 24) & 0xFF) as u8;
    let scratch_hi = (hcsparams2 >> 21) & 0x1F;
    let scratch_lo = (hcsparams2 >> 27) & 0x1F;
    let scratchpad_count = (scratch_hi << 5) | scratch_lo;
    let ac64 = hccparams1 & 0x1 != 0;
    let (legacy, protocols) = scan_extended_caps(mmio_virt, xecp);

    XhciInfo {
        mmio_virt,
        caplength,
        version,
        max_slots,
        max_interrupters,
        max_ports,
        scratchpad_count,
        ac64,
        xecp,
        op_base,
        rt_base,
        db_base,
        legacy,
        protocols,
    }
}

fn active_init(info: &XhciInfo) -> Result<ActiveState, &'static str> {
    if info.scratchpad_count != 0 {
        return Err("scratchpad buffers not implemented");
    }

    let pagesize = unsafe { read_u32(info.op_base + OP_PAGESIZE) };
    if pagesize & 0x1 == 0 {
        return Err("4KiB pages not supported by controller");
    }

    if let Some(legacy) = &info.legacy {
        request_handoff(info.mmio_virt, legacy.off)?;
    }

    stop_controller(info.op_base)?;
    reset_controller(info.op_base)?;

    unsafe {
        let cfg = read_u32(info.op_base + OP_CONFIG);
        write_u32(
            info.op_base + OP_CONFIG,
            (cfg & !0xFF) | info.max_slots as u32,
        );
    }

    let (dcbaa_phys, _) = alloc_zeroed_phys().ok_or("dcbaa alloc failed")?;
    let (cmd_ring_phys, cmd_ring_virt) = alloc_zeroed_phys().ok_or("command ring alloc failed")?;
    let (event_ring_phys, event_ring_virt) =
        alloc_zeroed_phys().ok_or("event ring alloc failed")?;
    let (erst_phys, erst_virt) = alloc_zeroed_phys().ok_or("erst alloc failed")?;
    let mut cmd_ring = CommandRingState {
        phys: cmd_ring_phys,
        virt: cmd_ring_virt,
        enqueue_idx: 0,
        cycle: true,
    };
    let mut event_ring = EventRingState {
        phys: event_ring_phys,
        virt: event_ring_virt,
        dequeue_idx: 0,
        cycle: true,
    };

    unsafe {
        init_link_trb(cmd_ring.phys, COMMAND_RING_TRBS, cmd_ring.phys);

        write_u64(info.op_base + OP_DCBAAP, dcbaa_phys);
        write_u64(info.op_base + OP_CRCR, cmd_ring.phys | 0x1);

        write_u64(erst_virt, event_ring.phys);
        write_u32(erst_virt + 8, EVENT_RING_TRBS as u32);
        write_u32(erst_virt + 12, 0);

        let ir0 = info.rt_base + RT_IR0;
        write_u32(ir0 + IR0_ERSTSZ, 1);
        write_u64(ir0 + IR0_ERSTBA, erst_phys);
        write_u64(ir0 + IR0_ERDP, event_ring.phys);
    }

    unsafe {
        let cmd = read_u32(info.op_base + OP_USBCMD);
        write_u32(info.op_base + OP_USBCMD, cmd | USBCMD_RS);
    }
    wait_until("controller start", || unsafe {
        read_u32(info.op_base + OP_USBSTS) & USBSTS_HCH == 0
    })?;
    run_command_ring_noop(info, &mut cmd_ring, &mut event_ring)?;

    Ok(ActiveState {
        dcbaa_phys,
        cmd_ring_phys,
        event_ring_phys,
        erst_phys,
    })
}

fn request_handoff(mmio_virt: u64, off: u64) -> Result<(), &'static str> {
    let addr = mmio_virt + off;
    let header = unsafe { read_u32(addr) };
    let bios_owned = (header >> 16) & 0x1 != 0;
    if !bios_owned {
        return Ok(());
    }

    unsafe {
        write_u32(addr, header | (1 << 24));
    }
    wait_until("bios handoff", || unsafe {
        read_u32(addr) & (1 << 16) == 0
    })
}

fn stop_controller(op_base: u64) -> Result<(), &'static str> {
    let cmd = unsafe { read_u32(op_base + OP_USBCMD) };
    if cmd & USBCMD_RS == 0 {
        return Ok(());
    }

    unsafe {
        write_u32(op_base + OP_USBCMD, cmd & !USBCMD_RS);
    }
    wait_until("controller halt", || unsafe {
        read_u32(op_base + OP_USBSTS) & USBSTS_HCH != 0
    })
}

fn reset_controller(op_base: u64) -> Result<(), &'static str> {
    wait_until("controller ready", || unsafe {
        read_u32(op_base + OP_USBSTS) & USBSTS_CNR == 0
    })?;

    unsafe {
        let cmd = read_u32(op_base + OP_USBCMD);
        write_u32(op_base + OP_USBCMD, cmd | USBCMD_HCRST);
    }

    wait_until("reset complete", || unsafe {
        let cmd = read_u32(op_base + OP_USBCMD);
        let sts = read_u32(op_base + OP_USBSTS);
        cmd & USBCMD_HCRST == 0 && sts & USBSTS_CNR == 0
    })
}

fn wait_until<F: Fn() -> bool>(label: &'static str, ready: F) -> Result<(), &'static str> {
    for _ in 0..SPIN_TIMEOUT {
        if ready() {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    println!("[xhci] timeout while waiting for {}", label);
    Err(label)
}

fn alloc_zeroed_phys() -> Option<(u64, u64)> {
    let frame = crate::vmm::alloc_zeroed_frame()?;
    let phys = frame.start_address().as_u64();
    let virt = crate::vmm::phys_to_virt(frame.start_address()).as_u64();
    Some((phys, virt))
}

fn run_command_ring_noop(
    info: &XhciInfo,
    cmd_ring: &mut CommandRingState,
    event_ring: &mut EventRingState,
) -> Result<(), &'static str> {
    let trb_phys = push_command_trb(cmd_ring, 0, 0, TRB_TYPE_NOOP_CMD << 10);
    fence(Ordering::SeqCst);
    unsafe {
        write_u32(info.db_base, 0);
    }

    let completion = wait_for_command_completion(info, event_ring, trb_phys)?;
    if completion.completion_code != COMPLETION_SUCCESS {
        println!(
            "[xhci] command ring no-op failed code={} ptr={:#x} slot={}",
            completion.completion_code, completion.ptr, completion.slot_id,
        );
        return Err("command ring no-op failed");
    }

    println!(
        "[xhci] command ring no-op complete ptr={:#x} slot={}",
        completion.ptr, completion.slot_id,
    );
    Ok(())
}

fn push_command_trb(ring: &mut CommandRingState, parameter: u64, status: u32, control: u32) -> u64 {
    let trb_phys = ring.phys + ring.enqueue_idx as u64 * 16;
    let trb_virt = ring.virt + ring.enqueue_idx as u64 * 16;
    let control = control | if ring.cycle { TRB_CYCLE } else { 0 };

    unsafe {
        write_u64(trb_virt, parameter);
        write_u32(trb_virt + 8, status);
        write_u32(trb_virt + 12, control);
    }

    ring.enqueue_idx += 1;
    if ring.enqueue_idx == COMMAND_RING_TRBS - 1 {
        ring.enqueue_idx = 0;
        ring.cycle = !ring.cycle;
    }

    trb_phys
}

fn wait_for_command_completion(
    info: &XhciInfo,
    event_ring: &mut EventRingState,
    expected_trb_phys: u64,
) -> Result<CommandCompletion, &'static str> {
    for _ in 0..SPIN_TIMEOUT {
        if let Some(event) = next_event(info, event_ring) {
            match event.trb_type() as u32 {
                TRB_TYPE_CMD_COMPLETION => {
                    let completion = CommandCompletion {
                        ptr: event.parameter & !0xFu64,
                        completion_code: event.completion_code(),
                        slot_id: event.slot_id(),
                    };
                    if completion.ptr == expected_trb_phys {
                        return Ok(completion);
                    }
                    println!(
                        "[xhci] unexpected command completion ptr={:#x} code={} slot={}",
                        completion.ptr, completion.completion_code, completion.slot_id,
                    );
                }
                TRB_TYPE_PORT_STATUS_CHANGE => {
                    println!(
                        "[xhci] event: port status change param={:#x} status={:#x} control={:#x}",
                        event.parameter, event.status, event.control,
                    );
                }
                _ => {
                    println!(
                        "[xhci] event: type={} code={} param={:#x} status={:#x} control={:#x}",
                        event.trb_type(),
                        event.completion_code(),
                        event.parameter,
                        event.status,
                        event.control,
                    );
                }
            }
        }
        core::hint::spin_loop();
    }

    println!(
        "[xhci] timeout while waiting for command completion ptr={:#x}",
        expected_trb_phys,
    );
    Err("command completion timeout")
}

fn next_event(info: &XhciInfo, ring: &mut EventRingState) -> Option<EventTrb> {
    let trb_virt = ring.virt + ring.dequeue_idx as u64 * 16;
    let control = unsafe { read_u32(trb_virt + 12) };
    if (control & TRB_CYCLE != 0) != ring.cycle {
        return None;
    }

    let event = EventTrb {
        parameter: unsafe { read_u64(trb_virt) },
        status: unsafe { read_u32(trb_virt + 8) },
        control,
    };
    advance_event_ring(info, ring);
    Some(event)
}

fn advance_event_ring(info: &XhciInfo, ring: &mut EventRingState) {
    ring.dequeue_idx += 1;
    if ring.dequeue_idx == EVENT_RING_TRBS {
        ring.dequeue_idx = 0;
        ring.cycle = !ring.cycle;
    }

    let erdp = ring.phys + ring.dequeue_idx as u64 * 16;
    unsafe {
        write_u64(info.rt_base + RT_IR0 + IR0_ERDP, erdp | ERDP_EHB_CLEAR);
    }
}

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

unsafe fn read_u64(addr: u64) -> u64 {
    core::ptr::read_volatile(addr as *const u64)
}

unsafe fn write_u32(addr: u64, val: u32) {
    core::ptr::write_volatile(addr as *mut u32, val)
}

unsafe fn write_u64(addr: u64, val: u64) {
    core::ptr::write_volatile(addr as *mut u64, val)
}

fn scan_extended_caps(base: u64, mut off: u64) -> (Option<LegacySupport>, Vec<SupportedProtocol>) {
    let mut legacy = None;
    let mut protocols = Vec::new();

    if off == 0 {
        println!("[xhci] no extended capabilities");
        return (legacy, protocols);
    }

    for _ in 0..32 {
        let header = unsafe { read_u32(base + off) };
        let cap_id = (header & 0xFF) as u8;
        let next = ((header >> 8) & 0xFF) as u64 * 4;

        match cap_id {
            EXT_CAP_LEGACY_SUPPORT => {
                legacy = Some(log_legacy_support(base, off, header));
            }
            EXT_CAP_SUPPORTED_PROTOCOL => {
                protocols.push(log_supported_protocol(base, off, header));
            }
            EXT_CAP_EXT_POWER_MGMT => {
                println!("[xhci] ext cap @+{:#x}: extended power management", off);
            }
            EXT_CAP_IO_VIRT => {
                println!("[xhci] ext cap @+{:#x}: I/O virtualization", off);
            }
            EXT_CAP_MSG_INTERRUPT => {
                println!("[xhci] ext cap @+{:#x}: message interrupt", off);
            }
            EXT_CAP_USB_DEBUG => {
                println!("[xhci] ext cap @+{:#x}: USB debug capability", off);
            }
            EXT_CAP_EXT_MSG_INTERRUPT => {
                println!("[xhci] ext cap @+{:#x}: extended message interrupt", off);
            }
            0 => {
                println!("[xhci] ext cap @+{:#x}: invalid id=0", off);
            }
            _ => {
                println!(
                    "[xhci] ext cap @+{:#x}: id={} header={:#x}",
                    off, cap_id, header
                );
            }
        }

        if next == 0 {
            if legacy.is_none() {
                println!("[xhci] no USB legacy support capability");
            }
            return (legacy, protocols);
        }
        off += next;
    }

    if legacy.is_none() {
        println!("[xhci] no USB legacy support capability");
    }
    println!("[xhci] extended capability scan truncated");
    (legacy, protocols)
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
    let label = protocol_label(protocol_name(name), rev_major, rev_minor);
    let port_last = port_offset.saturating_add(port_count.saturating_sub(1));
    let mut psis = Vec::new();

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
        psis.push(log_psi(idx, psi));
    }

    SupportedProtocol {
        label,
        major: rev_major,
        minor: rev_minor,
        port_offset,
        port_count,
        psi_count,
        slot_type,
        psis,
    }
}

fn log_legacy_support(base: u64, off: u64, header: u32) -> LegacySupport {
    let ctlsts = unsafe { read_u32(base + off + 0x04) };
    let bios_owned = (header >> 16) & 0x1 != 0;
    let os_owned = (header >> 24) & 0x1 != 0;
    let smi_usb_en = ctlsts & (1 << 0) != 0;
    let smi_hse_en = ctlsts & (1 << 4) != 0;
    let smi_os_own_en = ctlsts & (1 << 13) != 0;
    let smi_pci_cmd_en = ctlsts & (1 << 14) != 0;
    let smi_bar_en = ctlsts & (1 << 15) != 0;
    let smi_usb = ctlsts & (1 << 16) != 0;
    let smi_hse = ctlsts & (1 << 20) != 0;
    let smi_os_own = ctlsts & (1 << 29) != 0;
    let smi_pci_cmd = ctlsts & (1 << 30) != 0;
    let smi_bar = ctlsts & (1 << 31) != 0;

    println!(
        "[xhci] ext cap @+{:#x}: USB legacy support bios_owned={} os_owned={} ctlsts={:#x}",
        off, bios_owned as u8, os_owned as u8, ctlsts,
    );

    if smi_usb_en
        || smi_hse_en
        || smi_os_own_en
        || smi_pci_cmd_en
        || smi_bar_en
        || smi_usb
        || smi_hse
        || smi_os_own
        || smi_pci_cmd
        || smi_bar
    {
        println!(
            "[xhci]   legacy smi en usb={} hse={} own={} pci={} bar={} pending usb={} hse={} own={} pci={} bar={}",
            smi_usb_en as u8,
            smi_hse_en as u8,
            smi_os_own_en as u8,
            smi_pci_cmd_en as u8,
            smi_bar_en as u8,
            smi_usb as u8,
            smi_hse as u8,
            smi_os_own as u8,
            smi_pci_cmd as u8,
            smi_bar as u8,
        );
    }

    LegacySupport { off }
}

fn scan_ports(info: &XhciInfo) {
    let mut any = false;
    for port in 0..info.max_ports {
        let port_num = port + 1;
        let portsc = unsafe { read_u32(info.op_base + OP_PORTSC_BASE + 0x10 * port as u64) };
        let connected = portsc & PORTSC_CCS != 0;
        let enabled = portsc & PORTSC_PED != 0;
        let speed_id = ((portsc >> 10) & 0xF) as u8;

        if !connected && !enabled {
            continue;
        }

        any = true;
        if let Some(proto) = protocol_for_port(&info.protocols, port_num) {
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
                port_num, connected as u8, enabled as u8, speed_id, portsc,
            );
        }
    }

    if !any {
        println!("[xhci] no active root-hub ports reported");
    }
}

fn protocol_for_port(protocols: &[SupportedProtocol], port_num: u8) -> Option<&SupportedProtocol> {
    protocols.iter().find(|proto| {
        let start = proto.port_offset;
        let end = proto
            .port_offset
            .saturating_add(proto.port_count.saturating_sub(1));
        port_num >= start && port_num <= end
    })
}

fn port_speed_name(proto: &SupportedProtocol, speed_id: u8) -> String {
    if proto.psi_count != 0 {
        if let Some(psi) = proto.psis.iter().find(|psi| psi.psiv == speed_id) {
            return speed_name_from_psi(psi);
        }
        return String::from("?");
    }

    match (proto.major, proto.minor, speed_id) {
        (2, _, 1) => String::from("Full"),
        (2, _, 2) => String::from("Low"),
        (2, _, 3) => String::from("High"),
        (3, 0x00, 4) => String::from("Super"),
        (3, 0x10, 4) => String::from("Super"),
        (3, 0x10, 5) => String::from("Super+"),
        (3, 0x20, 4) => String::from("Super"),
        (3, 0x20, 5) => String::from("Super+ Gen2x1"),
        (3, 0x20, 6) => String::from("Super+ Gen1x2"),
        (3, 0x20, 7) => String::from("Super+ Gen2x2"),
        _ => String::from("?"),
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

fn log_psi(idx: u8, psi: u32) -> ProtocolSpeedId {
    let parsed = ProtocolSpeedId {
        psiv: (psi & 0x0F) as u8,
        psie: ((psi >> 4) & 0x03) as u8,
        plt: ((psi >> 6) & 0x03) as u8,
        pfd: ((psi >> 8) & 0x01) != 0,
        lp: ((psi >> 14) & 0x03) as u8,
        psim: ((psi >> 16) & 0xFFFF) as u16,
    };

    println!(
        "[xhci]   psi{}: id={} rate={} {} kind={} duplex={} link={} raw={:#x}",
        idx,
        parsed.psiv,
        parsed.psim,
        psi_units(parsed.psie),
        psi_type(parsed.plt),
        if parsed.pfd { "full" } else { "half" },
        link_protocol(parsed.lp),
        psi,
    );

    parsed
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

fn speed_name_from_psi(psi: &ProtocolSpeedId) -> String {
    format!(
        "{} {} {} {}",
        psi.psim,
        psi_units(psi.psie),
        psi_type(psi.plt),
        if psi.pfd { "full" } else { "half" },
    )
}

unsafe fn init_link_trb(ring_phys: u64, ring_size: usize, target_phys: u64) {
    let last_trb = crate::vmm::phys_to_virt(x86_64::PhysAddr::new(ring_phys)).as_u64()
        + ((ring_size - 1) * 16) as u64;
    write_u32(last_trb, (target_phys & 0xFFFF_FFFF) as u32);
    write_u32(last_trb + 4, (target_phys >> 32) as u32);
    write_u32(last_trb + 8, 0);
    write_u32(last_trb + 12, (TRB_TYPE_LINK << 10) | TRB_TC | TRB_CYCLE);
}
