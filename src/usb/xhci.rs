/// xHCI host controller driver — Phase 14 slices 1–4.
///
/// Default boot stays on a passive probe so the existing PS/2 input path
/// remains stable. An opt-in build (`COOLOS_XHCI_ACTIVE_INIT=1`) exercises the
/// real host-controller bring-up sequence: ownership handoff, reset, DCBAA,
/// command ring, event ring, and controller run.
extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{fence, Ordering};
use spin::Mutex;

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
const PORTSC_PR: u32 = 1 << 4;
const PORTSC_PP: u32 = 1 << 9;
const PORTSC_SPEED_SHIFT: u32 = 10;
const PORTSC_SPEED_MASK: u32 = 0xF << PORTSC_SPEED_SHIFT;
const PORTSC_CSC: u32 = 1 << 17;
const PORTSC_PEC: u32 = 1 << 18;
const PORTSC_WRC: u32 = 1 << 19;
const PORTSC_OCC: u32 = 1 << 20;
const PORTSC_PRC: u32 = 1 << 21;
const PORTSC_PLC: u32 = 1 << 22;
const PORTSC_CEC: u32 = 1 << 23;
const PORTSC_CHANGE_BITS: u32 =
    PORTSC_CSC | PORTSC_PEC | PORTSC_WRC | PORTSC_OCC | PORTSC_PRC | PORTSC_PLC | PORTSC_CEC;

const EXT_CAP_LEGACY_SUPPORT: u8 = 1;
const EXT_CAP_SUPPORTED_PROTOCOL: u8 = 2;
const EXT_CAP_EXT_POWER_MGMT: u8 = 3;
const EXT_CAP_IO_VIRT: u8 = 4;
const EXT_CAP_MSG_INTERRUPT: u8 = 5;
const EXT_CAP_USB_DEBUG: u8 = 10;
const EXT_CAP_EXT_MSG_INTERRUPT: u8 = 17;

const COMMAND_RING_TRBS: usize = 256;
const CONTROL_RING_TRBS: usize = 256;
const EVENT_RING_TRBS: usize = 256;
const INTERRUPT_RING_TRBS: usize = 256;
const TRB_TYPE_NORMAL: u32 = 1;
const TRB_TYPE_SETUP_STAGE: u32 = 2;
const TRB_TYPE_DATA_STAGE: u32 = 3;
const TRB_TYPE_STATUS_STAGE: u32 = 4;
const TRB_TYPE_LINK: u32 = 6;
const TRB_TYPE_ENABLE_SLOT_CMD: u32 = 9;
const TRB_TYPE_DISABLE_SLOT_CMD: u32 = 10;
const TRB_TYPE_ADDRESS_DEVICE_CMD: u32 = 11;
const TRB_TYPE_CONFIGURE_ENDPOINT_CMD: u32 = 12;
const TRB_TYPE_NOOP_CMD: u32 = 23;
const TRB_TYPE_TRANSFER_EVENT: u32 = 32;
const TRB_TYPE_CMD_COMPLETION: u32 = 33;
const TRB_TYPE_PORT_STATUS_CHANGE: u32 = 34;
const TRB_TC: u32 = 1 << 1;
const TRB_CYCLE: u32 = 1 << 0;
const TRB_IOC: u32 = 1 << 5;
const TRB_IDT: u32 = 1 << 6;
const TRB_DIR_IN: u32 = 1 << 16;
const TRB_TRT_NONE: u32 = 0 << 16;
const TRB_TRT_IN: u32 = 3 << 16;
const COMPLETION_SUCCESS: u8 = 1;
const COMPLETION_SHORT_PACKET: u8 = 13;
const ERDP_EHB_CLEAR: u64 = 1 << 3;
const CONTROL_ENDPOINT_DCI: u8 = 1;
const SETUP_GET_DESCRIPTOR: u8 = 6;
const SETUP_SET_CONFIGURATION: u8 = 9;
const SETUP_SET_IDLE: u8 = 10;
const SETUP_SET_PROTOCOL: u8 = 11;
const REQUEST_TYPE_IN: u8 = 0x80;
const REQUEST_TYPE_OUT: u8 = 0x00;
const REQUEST_TYPE_STANDARD: u8 = 0x00;
const REQUEST_TYPE_CLASS: u8 = 0x20;
const REQUEST_RECIPIENT_DEVICE: u8 = 0x00;
const REQUEST_RECIPIENT_INTERFACE: u8 = 0x01;
const DESCRIPTOR_TYPE_DEVICE: u16 = 1;
const DESCRIPTOR_TYPE_CONFIGURATION: u16 = 2;
const DESCRIPTOR_TYPE_HID: u8 = 0x21;
const DESCRIPTOR_TYPE_REPORT: u8 = 0x22;
const DEVICE_DESCRIPTOR_HEADER_LEN: usize = 8;
const DEVICE_DESCRIPTOR_LEN: usize = 18;
const CONFIG_DESCRIPTOR_HEADER_LEN: usize = 9;
const USB_DESC_TYPE_INTERFACE: u8 = 0x04;
const USB_DESC_TYPE_ENDPOINT: u8 = 0x05;
const USB_ENDPOINT_ATTR_INTERRUPT: u8 = 0x03;
const USB_CLASS_HID: u8 = 0x03;
const USB_HID_SUBCLASS_BOOT: u8 = 0x01;
const USB_HID_PROTOCOL_KEYBOARD: u8 = 0x01;
const USB_HID_PROTOCOL_MOUSE: u8 = 0x02;
const DESCRIPTOR_BUFFER_BYTES: usize = 4096;
const BOOT_KEYBOARD_REPORT_BYTES: usize = 8;
const BOOT_MOUSE_REPORT_BYTES: usize = 4;

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
    context_size: usize,
    op_base: u64,
    rt_base: u64,
    db_base: u64,
    legacy: Option<LegacySupport>,
    protocols: Vec<SupportedProtocol>,
}

struct ActiveState {
    op_base: u64,
    rt_base: u64,
    db_base: u64,
    dcbaa_phys: u64,
    cmd_ring_phys: u64,
    event_ring_phys: u64,
    event_ring: EventRingState,
    erst_phys: u64,
    devices: Vec<HidDeviceState>,
    poll_count: u64,
    event_count: u64,
    last_runtime_note: String,
    port_status: Vec<String>,
}

struct CommandRingState {
    phys: u64,
    virt: u64,
    enqueue_idx: usize,
    cycle: bool,
}

struct TransferRingState {
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

    fn endpoint_id(&self) -> u8 {
        ((self.control >> 16) & 0x1F) as u8
    }

    fn port_id(&self) -> u8 {
        (self.parameter >> 24) as u8
    }

    fn residual(&self) -> u32 {
        self.status & 0x00FF_FFFF
    }
}

struct CommandCompletion {
    ptr: u64,
    completion_code: u8,
    slot_id: u8,
}

struct TransferCompletion {
    ptr: u64,
    completion_code: u8,
    slot_id: u8,
    endpoint_id: u8,
    residual: u32,
}

struct PrimedDevice {
    default_mps: u16,
    transfer_ring: TransferRingState,
    descriptor_phys: u64,
    descriptor_virt: u64,
    input_ctx_phys: u64,
    input_ctx_virt: u64,
    output_ctx_virt: u64,
}

struct DeviceDescriptor {
    usb_bcd: u16,
    class: u8,
    subclass: u8,
    protocol: u8,
    max_packet_size0: u16,
    vendor_id: u16,
    product_id: u16,
    device_bcd: u16,
    configurations: u8,
}

struct HidInterface {
    number: u8,
    alternate_setting: u8,
    protocol: u8,
    endpoint_address: u8,
    max_packet_size: u16,
    interval: u8,
    report_descriptor_len: u16,
}

struct HidDeviceState {
    port_num: u8,
    slot_id: u8,
    protocol: u8,
    interface_number: u8,
    endpoint_address: u8,
    endpoint_dci: u8,
    report_request_len: usize,
    report_ring: TransferRingState,
    report_buffer_phys: u64,
    report_buffer_virt: u64,
    report_trb_phys: u64,
    interval: u8,
    report_count: u64,
    error_count: u64,
    last_report_len: usize,
    last_completion_code: u8,
}

static RUNTIME: Mutex<Option<ActiveState>> = Mutex::new(None);

pub fn probe() -> Vec<String> {
    *RUNTIME.lock() = None;
    let mut status = Vec::new();

    let Some((loc, hdr, mmio_phys)) = find_controller() else {
        println!("[xhci] no controller found on PCI bus");
        status.push(String::from("USB: no xHCI controller found"));
        return status;
    };

    println!(
        "[xhci] {:04x}:{:02x}.{} vendor={:04x} device={:04x} mmio={:#x}",
        loc.bus, loc.device, loc.function, hdr.vendor_id, hdr.device_id, mmio_phys,
    );
    status.push(format!(
        "USB: xHCI {:04x}:{:02x}.{} vendor={:04x} device={:04x}",
        loc.bus, loc.device, loc.function, hdr.vendor_id, hdr.device_id,
    ));

    if ACTIVE_INIT {
        pci::enable_bus_master(loc);
        println!("[xhci] active init enabled");
        status.push(String::from("USB: active controller init enabled"));
    } else {
        pci::enable_memory_space(loc);
        status.push(String::from(
            "USB: passive probe only; PS/2 remains primary input",
        ));
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
    status.push(format!(
        "USB: xHCI v0x{:04x}, slots={}, ports={}, scratchpads={}, 64bit={}",
        info.version, info.max_slots, info.max_ports, info.scratchpad_count, info.ac64 as u8,
    ));

    if ACTIVE_INIT {
        match active_init(&info) {
            Ok(state) => {
                println!(
                    "[xhci] active init ready dcbaa={:#x} cmd={:#x} evt={:#x} erst={:#x}",
                    state.dcbaa_phys, state.cmd_ring_phys, state.event_ring_phys, state.erst_phys,
                );
                status.push(String::from("USB: active init ready"));
                status.extend(state.port_status.iter().cloned());
                *RUNTIME.lock() = Some(state);
            }
            Err(err) => {
                println!("[xhci] active init failed: {}", err);
                status.push(format!("USB: active init failed: {}", err));
            }
        }
    }

    status.extend(scan_ports(&info));
    if !ACTIVE_INIT {
        println!("[xhci] passive probe only; controller bring-up disabled to preserve PS/2 input");
    }
    status
}

pub fn poll() {
    let mut runtime_guard = RUNTIME.lock();
    let Some(runtime) = runtime_guard.as_mut() else {
        return;
    };

    runtime.poll_count = runtime.poll_count.saturating_add(1);
    while let Some(event) = next_event_by_base(runtime.rt_base, &mut runtime.event_ring) {
        runtime.event_count = runtime.event_count.saturating_add(1);
        match event.trb_type() as u32 {
            TRB_TYPE_TRANSFER_EVENT => handle_runtime_transfer_event(runtime, event),
            TRB_TYPE_PORT_STATUS_CHANGE => {
                let port_num = event.port_id();
                let portsc = read_portsc_by_op_base(runtime.op_base, port_num);
                clear_port_changes_by_op_base(runtime.op_base, port_num);
                runtime.last_runtime_note = format!(
                    "port {} change ccs={} ped={} speed_id={}",
                    port_num,
                    (portsc & PORTSC_CCS != 0) as u8,
                    (portsc & PORTSC_PED != 0) as u8,
                    port_speed_id(portsc),
                );
                println!(
                    "[xhci] runtime event: port {} status change portsc={:#x}",
                    port_num,
                    portsc,
                );
            }
            TRB_TYPE_CMD_COMPLETION => {
                runtime.last_runtime_note = format!(
                    "unexpected command completion slot={} code={}",
                    event.slot_id(),
                    event.completion_code(),
                );
                println!(
                    "[xhci] runtime event: unexpected command completion ptr={:#x} code={} slot={}",
                    event.parameter & !0xFu64,
                    event.completion_code(),
                    event.slot_id(),
                );
            }
            _ => {
                runtime.last_runtime_note = format!(
                    "event type={} code={}",
                    event.trb_type(),
                    event.completion_code(),
                );
                println!(
                    "[xhci] runtime event: type={} code={} param={:#x} status={:#x}",
                    event.trb_type(),
                    event.completion_code(),
                    event.parameter,
                    event.status,
                );
            }
        }
    }
}

pub fn runtime_status_lines() -> Vec<String> {
    let runtime_guard = RUNTIME.lock();
    let Some(runtime) = runtime_guard.as_ref() else {
        return Vec::new();
    };

    let mut lines = Vec::new();
    lines.push(format!(
        "USB: runtime devices={} polls={} events={}",
        runtime.devices.len(),
        runtime.poll_count,
        runtime.event_count,
    ));
    if !runtime.last_runtime_note.is_empty() {
        lines.push(format!("USB: runtime {}", runtime.last_runtime_note));
    }
    for device in runtime.devices.iter() {
        lines.push(format!(
            "USB: runtime port={} slot={} {} ep={:#04x} reports={} last={}B errors={} cc={}",
            device.port_num,
            device.slot_id,
            hid_protocol_name(device.protocol),
            device.endpoint_address,
            device.report_count,
            device.last_report_len,
            device.error_count,
            device.last_completion_code,
        ));
    }
    lines
}

pub fn runtime_input_presence() -> (bool, bool) {
    let runtime_guard = RUNTIME.lock();
    let Some(runtime) = runtime_guard.as_ref() else {
        return (false, false);
    };

    let mut keyboard = false;
    let mut mouse = false;
    for device in runtime.devices.iter() {
        if device.protocol == USB_HID_PROTOCOL_KEYBOARD {
            keyboard = true;
        } else if device.protocol == USB_HID_PROTOCOL_MOUSE {
            mouse = true;
        }
    }

    (keyboard, mouse)
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
    let context_size = if hccparams1 & (1 << 2) != 0 { 64 } else { 32 };
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
        context_size,
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

    let (dcbaa_phys, dcbaa_virt) = alloc_zeroed_phys().ok_or("dcbaa alloc failed")?;
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
    let (port_status, devices) =
        prime_attached_ports(info, dcbaa_virt, &mut cmd_ring, &mut event_ring);

    Ok(ActiveState {
        op_base: info.op_base,
        rt_base: info.rt_base,
        db_base: info.db_base,
        dcbaa_phys,
        cmd_ring_phys,
        event_ring_phys,
        event_ring,
        erst_phys,
        devices,
        poll_count: 0,
        event_count: 0,
        last_runtime_note: String::from("polling ready"),
        port_status,
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

fn prime_attached_ports(
    info: &XhciInfo,
    dcbaa_virt: u64,
    cmd_ring: &mut CommandRingState,
    event_ring: &mut EventRingState,
) -> (Vec<String>, Vec<HidDeviceState>) {
    let mut status = Vec::new();
    let mut devices = Vec::new();

    for port_num in 1..=info.max_ports {
        let Some(proto) = protocol_for_port(&info.protocols, port_num) else {
            continue;
        };

        let mut portsc = read_portsc(info, port_num);
        if portsc & PORTSC_CCS == 0 {
            continue;
        }

        clear_port_changes(info, port_num);
        portsc = read_portsc(info, port_num);

        if proto.major == 2 && portsc & PORTSC_PED == 0 {
            match reset_port(info, event_ring, port_num) {
                Ok(updated) => portsc = updated,
                Err(err) => {
                    status.push(format!(
                        "USB: port {} {} reset failed: {}",
                        port_num, proto.label, err
                    ));
                    continue;
                }
            }
        } else if proto.major >= 3 && portsc & PORTSC_PED == 0 {
            let _ = wait_until("usb3 port enable", || {
                let current = read_portsc(info, port_num);
                current & PORTSC_PED != 0 || current & PORTSC_CCS == 0
            });
            clear_port_changes(info, port_num);
            portsc = read_portsc(info, port_num);
        }

        if portsc & PORTSC_CCS == 0 {
            status.push(format!(
                "USB: port {} {} disconnected during probe",
                port_num, proto.label
            ));
            continue;
        }

        if portsc & PORTSC_PED == 0 {
            status.push(format!(
                "USB: port {} {} connected but not enabled",
                port_num, proto.label
            ));
            continue;
        }

        match prime_default_control_endpoint(
            info, dcbaa_virt, cmd_ring, event_ring, proto, port_num, portsc,
        ) {
            Ok((lines, maybe_devices)) => {
                status.extend(lines);
                devices.extend(maybe_devices);
            }
            Err(err) => status.push(format!(
                "USB: port {} {} prime failed: {}",
                port_num, proto.label, err
            )),
        }
    }

    (status, devices)
}

fn prime_default_control_endpoint(
    info: &XhciInfo,
    dcbaa_virt: u64,
    cmd_ring: &mut CommandRingState,
    event_ring: &mut EventRingState,
    proto: &SupportedProtocol,
    port_num: u8,
    portsc: u32,
) -> Result<(Vec<String>, Vec<HidDeviceState>), &'static str> {
    let speed_id = port_speed_id(portsc);
    if speed_id == 0 {
        return Err("port speed undefined");
    }

    let slot_id = enable_slot(info, cmd_ring, event_ring, proto.slot_type)?;
    let mut device =
        match build_default_control_device(info, dcbaa_virt, slot_id, port_num, speed_id) {
        Ok(device) => device,
        Err(err) => {
            let _ = disable_slot(info, cmd_ring, event_ring, slot_id);
            return Err(err);
        }
    };

    if let Err(err) =
        address_device(info, cmd_ring, event_ring, slot_id, device.input_ctx_phys, true)
    {
        let _ = disable_slot(info, cmd_ring, event_ring, slot_id);
        return Err(err);
    }

    let descriptor8 = match read_device_descriptor_header(
        info,
        event_ring,
        slot_id,
        &mut device.transfer_ring,
        device.descriptor_phys,
        device.descriptor_virt,
    ) {
        Ok(bytes) => bytes,
        Err(err) => {
            let _ = disable_slot(info, cmd_ring, event_ring, slot_id);
            return Err(err);
        }
    };

    let descriptor = match read_device_descriptor(
        info,
        event_ring,
        slot_id,
        &mut device.transfer_ring,
        device.descriptor_phys,
        device.descriptor_virt,
    ) {
        Ok(descriptor) => descriptor,
        Err(err) => {
            let _ = disable_slot(info, cmd_ring, event_ring, slot_id);
            return Err(err);
        }
    };

    update_ep0_max_packet_size(info, &device, descriptor.max_packet_size0);
    if let Err(err) = address_device(
        info,
        cmd_ring,
        event_ring,
        slot_id,
        device.input_ctx_phys,
        false,
    ) {
        let _ = disable_slot(info, cmd_ring, event_ring, slot_id);
        return Err(err);
    }

    let config = match read_configuration_descriptor(
        info,
        event_ring,
        slot_id,
        &mut device.transfer_ring,
        device.descriptor_phys,
        device.descriptor_virt,
    ) {
        Ok(config) => config,
        Err(err) => {
            let _ = disable_slot(info, cmd_ring, event_ring, slot_id);
            return Err(err);
        }
    };

    let hid_interfaces = parse_boot_hid_interfaces(&config);
    let mut status = Vec::new();
    let mut devices = Vec::new();
    let config_value = config.get(5).copied().ok_or("configuration value missing")?;

    println!(
        "[xhci] slot {} device vid={:04x} pid={:04x} bcdUSB={:04x} dev={:04x} class={:02x}/{:02x}/{:02x} configs={}",
        slot_id,
        descriptor.vendor_id,
        descriptor.product_id,
        descriptor.usb_bcd,
        descriptor.device_bcd,
        descriptor.class,
        descriptor.subclass,
        descriptor.protocol,
        descriptor.configurations,
    );

    status.push(format!(
        "USB: port {} {} slot={} speed={} vid={:04x} pid={:04x} bcdUSB={:04x} dev={:04x} ep0_mps={} mps0_raw={} default_mps={}",
        port_num,
        proto.label,
        slot_id,
        port_speed_name(proto, speed_id),
        descriptor.vendor_id,
        descriptor.product_id,
        descriptor.usb_bcd,
        descriptor.device_bcd,
        descriptor.max_packet_size0,
        descriptor8[7],
        device.default_mps,
    ));

    if hid_interfaces.is_empty() {
        println!(
            "[xhci] slot {} config0 parsed but no boot HID interfaces were found",
            slot_id
        );
        status.push(format!(
            "USB: port {} slot={} no boot HID interfaces in config 0",
            port_num, slot_id,
        ));
        return Ok((status, devices));
    }

    if let Err(err) = set_configuration(
        info,
        event_ring,
        slot_id,
        &mut device.transfer_ring,
        config_value,
    ) {
        let _ = disable_slot(info, cmd_ring, event_ring, slot_id);
        return Err(err);
    }

    for hid in hid_interfaces {
        let configured = match activate_hid_interface(
            info,
            cmd_ring,
            event_ring,
            slot_id,
            port_num,
            speed_id,
            &hid,
            &mut device,
        ) {
            Ok(configured) => configured,
            Err(err) => {
                let _ = disable_slot(info, cmd_ring, event_ring, slot_id);
                return Err(err);
            }
        };
        println!(
            "[xhci] slot {} hid {} iface={} alt={} ep={:#04x} mps={} interval={} report_desc={}",
            slot_id,
            hid_protocol_name(configured.protocol),
            configured.interface_number,
            hid.alternate_setting,
            configured.endpoint_address,
            hid.max_packet_size,
            configured.interval,
            hid.report_descriptor_len,
        );
        status.push(format!(
            "USB: port {} slot={} HID {} iface={} alt={} ep={:#04x} mps={} interval={} report_desc={}",
            port_num,
            slot_id,
            hid_protocol_name(configured.protocol),
            configured.interface_number,
            hid.alternate_setting,
            configured.endpoint_address,
            hid.max_packet_size,
            configured.interval,
            hid.report_descriptor_len,
        ));
        devices.push(configured);
    }

    Ok((status, devices))
}

fn build_default_control_device(
    info: &XhciInfo,
    dcbaa_virt: u64,
    slot_id: u8,
    port_num: u8,
    speed_id: u8,
) -> Result<PrimedDevice, &'static str> {
    let (input_ctx_phys, input_ctx_virt) =
        alloc_zeroed_phys().ok_or("input context alloc failed")?;
    let (output_ctx_phys, output_ctx_virt) =
        alloc_zeroed_phys().ok_or("output context alloc failed")?;
    let (transfer_ring_phys, transfer_ring_virt) =
        alloc_zeroed_phys().ok_or("control ring alloc failed")?;
    let (descriptor_phys, descriptor_virt) =
        alloc_zeroed_phys().ok_or("descriptor buffer alloc failed")?;

    unsafe {
        init_link_trb(transfer_ring_phys, CONTROL_RING_TRBS, transfer_ring_phys);
        write_u64(dcbaa_virt + slot_id as u64 * 8, output_ctx_phys);
        // Input Control Context: evaluate Slot Context (A0) and Endpoint 0 Context (A1).
        write_u32(input_ctx_virt + 0x04, 0x0000_0003);
    }

    let slot_ctx = input_ctx_virt + info.context_size as u64;
    let ep0_ctx = input_ctx_virt + (info.context_size as u64 * 2);
    let default_mps = default_control_mps(speed_id);
    if default_mps == 0 {
        return Err("unsupported default control max packet size");
    }

    unsafe {
        // Slot Context
        write_u32(slot_ctx, ((speed_id as u32) << 20) | (1 << 27));
        write_u32(slot_ctx + 0x04, (port_num as u32) << 16);

        // Endpoint 0 Context
        write_u32(ep0_ctx, 0);
        write_u32(
            ep0_ctx + 0x04,
            ((default_mps as u32) << 16) | (4 << 3) | (3 << 1),
        );
        write_u64(ep0_ctx + 0x08, transfer_ring_phys | 1);
        write_u32(ep0_ctx + 0x10, 8);
    }

    Ok(PrimedDevice {
        default_mps,
        transfer_ring: TransferRingState {
            phys: transfer_ring_phys,
            virt: transfer_ring_virt,
            enqueue_idx: 0,
            cycle: true,
        },
        descriptor_phys,
        descriptor_virt,
        input_ctx_phys,
        input_ctx_virt,
        output_ctx_virt,
    })
}

fn enable_slot(
    info: &XhciInfo,
    cmd_ring: &mut CommandRingState,
    event_ring: &mut EventRingState,
    slot_type: u8,
) -> Result<u8, &'static str> {
    let trb_phys = push_command_trb(
        cmd_ring,
        0,
        0,
        (TRB_TYPE_ENABLE_SLOT_CMD << 10) | ((slot_type as u32) << 16),
    );
    ring_host_doorbell(info);

    let completion = wait_for_command_completion(info, event_ring, trb_phys)?;
    if completion.completion_code != COMPLETION_SUCCESS || completion.slot_id == 0 {
        println!(
            "[xhci] enable slot failed code={} ptr={:#x} slot={}",
            completion.completion_code, completion.ptr, completion.slot_id,
        );
        return Err("enable slot failed");
    }

    println!("[xhci] enabled slot {}", completion.slot_id);
    Ok(completion.slot_id)
}

fn disable_slot(
    info: &XhciInfo,
    cmd_ring: &mut CommandRingState,
    event_ring: &mut EventRingState,
    slot_id: u8,
) -> Result<(), &'static str> {
    let trb_phys = push_command_trb(
        cmd_ring,
        0,
        0,
        (TRB_TYPE_DISABLE_SLOT_CMD << 10) | ((slot_id as u32) << 24),
    );
    ring_host_doorbell(info);

    let completion = wait_for_command_completion(info, event_ring, trb_phys)?;
    if completion.completion_code != COMPLETION_SUCCESS {
        println!(
            "[xhci] disable slot {} failed code={} ptr={:#x}",
            slot_id, completion.completion_code, completion.ptr,
        );
        return Err("disable slot failed");
    }

    Ok(())
}

fn address_device(
    info: &XhciInfo,
    cmd_ring: &mut CommandRingState,
    event_ring: &mut EventRingState,
    slot_id: u8,
    input_ctx_phys: u64,
    bsr: bool,
) -> Result<(), &'static str> {
    let trb_phys = push_command_trb(
        cmd_ring,
        input_ctx_phys,
        0,
        (TRB_TYPE_ADDRESS_DEVICE_CMD << 10)
            | ((bsr as u32) << 9)
            | ((slot_id as u32) << 24),
    );
    ring_host_doorbell(info);

    let completion = wait_for_command_completion(info, event_ring, trb_phys)?;
    if completion.completion_code != COMPLETION_SUCCESS {
        println!(
            "[xhci] address device failed slot={} code={} ptr={:#x}",
            slot_id, completion.completion_code, completion.ptr,
        );
        return Err("address device failed");
    }

    println!(
        "[xhci] slot {} address device complete bsr={}",
        slot_id,
        bsr as u8,
    );
    Ok(())
}

fn update_ep0_max_packet_size(info: &XhciInfo, device: &PrimedDevice, max_packet_size0: u16) {
    if max_packet_size0 == 0 {
        return;
    }

    let ep0_ctx = device.input_ctx_virt + (info.context_size as u64 * 2);
    unsafe {
        let word = read_u32(ep0_ctx + 0x04);
        write_u32(
            ep0_ctx + 0x04,
            (word & 0x0000_FFFF) | ((max_packet_size0 as u32) << 16),
        );
        write_u32(device.input_ctx_virt, 0);
        write_u32(device.input_ctx_virt + 0x04, 0x0000_0003);
    }
}

fn read_device_descriptor_header(
    info: &XhciInfo,
    event_ring: &mut EventRingState,
    slot_id: u8,
    ring: &mut TransferRingState,
    buffer_phys: u64,
    buffer_virt: u64,
) -> Result<[u8; DEVICE_DESCRIPTOR_HEADER_LEN], &'static str> {
    let descriptor_bytes = read_descriptor(
        info,
        event_ring,
        slot_id,
        ring,
        buffer_phys,
        buffer_virt,
        DESCRIPTOR_TYPE_DEVICE,
        0,
        REQUEST_RECIPIENT_DEVICE,
        0,
        DEVICE_DESCRIPTOR_HEADER_LEN,
    )?;
    let mut bytes = [0u8; DEVICE_DESCRIPTOR_HEADER_LEN];
    for (idx, byte) in bytes.iter_mut().enumerate() {
        *byte = descriptor_bytes[idx];
    }

    println!(
        "[xhci] slot {} descriptor8 {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
        slot_id, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    );

    Ok(bytes)
}

fn read_device_descriptor(
    info: &XhciInfo,
    event_ring: &mut EventRingState,
    slot_id: u8,
    ring: &mut TransferRingState,
    buffer_phys: u64,
    buffer_virt: u64,
) -> Result<DeviceDescriptor, &'static str> {
    let bytes = read_descriptor(
        info,
        event_ring,
        slot_id,
        ring,
        buffer_phys,
        buffer_virt,
        DESCRIPTOR_TYPE_DEVICE,
        0,
        REQUEST_RECIPIENT_DEVICE,
        0,
        DEVICE_DESCRIPTOR_LEN,
    )?;

    if bytes.len() != DEVICE_DESCRIPTOR_LEN || bytes[1] != DESCRIPTOR_TYPE_DEVICE as u8 {
        return Err("malformed device descriptor");
    }

    Ok(DeviceDescriptor {
        usb_bcd: u16::from_le_bytes([bytes[2], bytes[3]]),
        class: bytes[4],
        subclass: bytes[5],
        protocol: bytes[6],
        max_packet_size0: max_packet_size0_from_descriptor(
            u16::from_le_bytes([bytes[2], bytes[3]]),
            bytes[7],
        ),
        vendor_id: u16::from_le_bytes([bytes[8], bytes[9]]),
        product_id: u16::from_le_bytes([bytes[10], bytes[11]]),
        device_bcd: u16::from_le_bytes([bytes[12], bytes[13]]),
        configurations: bytes[17],
    })
}

fn read_configuration_descriptor(
    info: &XhciInfo,
    event_ring: &mut EventRingState,
    slot_id: u8,
    ring: &mut TransferRingState,
    buffer_phys: u64,
    buffer_virt: u64,
) -> Result<Vec<u8>, &'static str> {
    let header = read_descriptor(
        info,
        event_ring,
        slot_id,
        ring,
        buffer_phys,
        buffer_virt,
        DESCRIPTOR_TYPE_CONFIGURATION,
        0,
        REQUEST_RECIPIENT_DEVICE,
        0,
        CONFIG_DESCRIPTOR_HEADER_LEN,
    )?;

    if header.len() != CONFIG_DESCRIPTOR_HEADER_LEN
        || header[1] != DESCRIPTOR_TYPE_CONFIGURATION as u8
    {
        return Err("malformed configuration descriptor header");
    }

    let total_len = u16::from_le_bytes([header[2], header[3]]) as usize;
    if !(CONFIG_DESCRIPTOR_HEADER_LEN..=DESCRIPTOR_BUFFER_BYTES).contains(&total_len) {
        return Err("configuration descriptor too large");
    }

    let config = read_descriptor(
        info,
        event_ring,
        slot_id,
        ring,
        buffer_phys,
        buffer_virt,
        DESCRIPTOR_TYPE_CONFIGURATION,
        0,
        REQUEST_RECIPIENT_DEVICE,
        0,
        total_len,
    )?;

    println!(
        "[xhci] slot {} config0 total_len={} interfaces={} max_power={}mA attrs={:#x}",
        slot_id,
        total_len,
        config[4],
        (config[8] as u16) * 2,
        config[7],
    );

    Ok(config)
}

fn set_configuration(
    info: &XhciInfo,
    event_ring: &mut EventRingState,
    slot_id: u8,
    ring: &mut TransferRingState,
    configuration_value: u8,
) -> Result<(), &'static str> {
    control_transfer_no_data(
        info,
        event_ring,
        slot_id,
        ring,
        REQUEST_TYPE_OUT | REQUEST_TYPE_STANDARD | REQUEST_RECIPIENT_DEVICE,
        SETUP_SET_CONFIGURATION,
        configuration_value as u16,
        0,
    )?;
    println!(
        "[xhci] slot {} set configuration {}",
        slot_id, configuration_value
    );
    Ok(())
}

fn set_boot_protocol(
    info: &XhciInfo,
    event_ring: &mut EventRingState,
    slot_id: u8,
    ring: &mut TransferRingState,
    interface_number: u8,
) -> Result<(), &'static str> {
    control_transfer_no_data(
        info,
        event_ring,
        slot_id,
        ring,
        REQUEST_TYPE_OUT | REQUEST_TYPE_CLASS | REQUEST_RECIPIENT_INTERFACE,
        SETUP_SET_PROTOCOL,
        0,
        interface_number as u16,
    )?;
    Ok(())
}

fn set_idle(
    info: &XhciInfo,
    event_ring: &mut EventRingState,
    slot_id: u8,
    ring: &mut TransferRingState,
    interface_number: u8,
) -> Result<(), &'static str> {
    control_transfer_no_data(
        info,
        event_ring,
        slot_id,
        ring,
        REQUEST_TYPE_OUT | REQUEST_TYPE_CLASS | REQUEST_RECIPIENT_INTERFACE,
        SETUP_SET_IDLE,
        0,
        interface_number as u16,
    )?;
    Ok(())
}

fn activate_hid_interface(
    info: &XhciInfo,
    cmd_ring: &mut CommandRingState,
    event_ring: &mut EventRingState,
    slot_id: u8,
    port_num: u8,
    speed_id: u8,
    hid: &HidInterface,
    device: &mut PrimedDevice,
) -> Result<HidDeviceState, &'static str> {
    set_boot_protocol(
        info,
        event_ring,
        slot_id,
        &mut device.transfer_ring,
        hid.number,
    )?;
    if hid.protocol == USB_HID_PROTOCOL_KEYBOARD {
        let _ = set_idle(
            info,
            event_ring,
            slot_id,
            &mut device.transfer_ring,
            hid.number,
        );
    }

    let endpoint_dci = endpoint_dci(hid.endpoint_address);
    if endpoint_dci <= CONTROL_ENDPOINT_DCI {
        return Err("invalid HID interrupt endpoint");
    }

    let (report_ring_phys, report_ring_virt) =
        alloc_zeroed_phys().ok_or("interrupt ring alloc failed")?;
    let (report_buffer_phys, report_buffer_virt) =
        alloc_zeroed_phys().ok_or("report buffer alloc failed")?;
    unsafe {
        init_link_trb(report_ring_phys, INTERRUPT_RING_TRBS, report_ring_phys);
    }

    configure_interrupt_endpoint(
        info,
        cmd_ring,
        event_ring,
        slot_id,
        speed_id,
        hid,
        endpoint_dci,
        report_ring_phys,
        device,
    )?;

    let mut hid_state = HidDeviceState {
        port_num,
        slot_id,
        protocol: hid.protocol,
        interface_number: hid.number,
        endpoint_address: hid.endpoint_address,
        endpoint_dci,
        report_request_len: interrupt_report_len(hid),
        report_ring: TransferRingState {
            phys: report_ring_phys,
            virt: report_ring_virt,
            enqueue_idx: 0,
            cycle: true,
        },
        report_buffer_phys,
        report_buffer_virt,
        report_trb_phys: 0,
        interval: hid.interval.max(1),
        report_count: 0,
        error_count: 0,
        last_report_len: 0,
        last_completion_code: 0,
    };
    queue_interrupt_transfer_by_base(info.db_base, &mut hid_state)?;

    Ok(hid_state)
}

fn configure_interrupt_endpoint(
    info: &XhciInfo,
    cmd_ring: &mut CommandRingState,
    event_ring: &mut EventRingState,
    slot_id: u8,
    speed_id: u8,
    hid: &HidInterface,
    endpoint_dci: u8,
    report_ring_phys: u64,
    device: &mut PrimedDevice,
) -> Result<(), &'static str> {
    unsafe {
        zero_page(device.input_ctx_virt);

        let output_slot_ctx = device.output_ctx_virt + info.context_size as u64;
        let input_slot_ctx = device.input_ctx_virt + info.context_size as u64;
        copy_context(output_slot_ctx, input_slot_ctx, info.context_size);

        let output_ep0_ctx = device.output_ctx_virt + (info.context_size as u64 * 2);
        let input_ep0_ctx = device.input_ctx_virt + (info.context_size as u64 * 2);
        copy_context(output_ep0_ctx, input_ep0_ctx, info.context_size);

        write_u32(device.input_ctx_virt, 0);
        write_u32(
            device.input_ctx_virt + 0x04,
            (1 << 0) | (1 << endpoint_dci),
        );

        let slot_ctx_entries = read_u32(input_slot_ctx) & !(0x1F << 27);
        write_u32(
            input_slot_ctx,
            slot_ctx_entries | ((endpoint_dci.max(1) as u32) << 27),
        );

        let ep_ctx = device.input_ctx_virt + ((endpoint_dci as u64 + 1) * info.context_size as u64);
        write_u32(
            ep_ctx,
            (interrupt_interval(speed_id, hid.interval) as u32) << 16,
        );
        write_u32(
            ep_ctx + 0x04,
            ((hid.max_packet_size as u32) << 16) | (7 << 3) | (3 << 1),
        );
        write_u64(ep_ctx + 0x08, report_ring_phys | 1);
        write_u32(
            ep_ctx + 0x10,
            interrupt_report_len(hid) as u32,
        );
    }

    let trb_phys = push_command_trb(
        cmd_ring,
        device.input_ctx_phys,
        0,
        (TRB_TYPE_CONFIGURE_ENDPOINT_CMD << 10) | ((slot_id as u32) << 24),
    );
    ring_host_doorbell(info);

    let completion = wait_for_command_completion(info, event_ring, trb_phys)?;
    if completion.completion_code != COMPLETION_SUCCESS {
        println!(
            "[xhci] configure endpoint failed slot={} code={} ptr={:#x}",
            slot_id, completion.completion_code, completion.ptr,
        );
        return Err("configure endpoint failed");
    }

    Ok(())
}

fn control_transfer_in(
    info: &XhciInfo,
    event_ring: &mut EventRingState,
    slot_id: u8,
    ring: &mut TransferRingState,
    buffer_phys: u64,
    buffer_virt: u64,
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    len: usize,
) -> Result<Vec<u8>, &'static str> {
    if len == 0 || len > DESCRIPTOR_BUFFER_BYTES || len > u16::MAX as usize {
        return Err("control transfer length unsupported");
    }

    let setup = usb_setup_packet(request_type, request, value, index, len as u16);

    let _setup_phys = push_transfer_trb(ring, setup, 8, (TRB_TYPE_SETUP_STAGE << 10) | TRB_IDT | TRB_TRT_IN);
    let _data_phys = push_transfer_trb(
        ring,
        buffer_phys,
        len as u32,
        (TRB_TYPE_DATA_STAGE << 10) | TRB_DIR_IN,
    );
    let status_phys = push_transfer_trb(ring, 0, 0, (TRB_TYPE_STATUS_STAGE << 10) | TRB_IOC);

    ring_device_doorbell(info, slot_id, CONTROL_ENDPOINT_DCI);

    let transfer =
        wait_for_transfer_completion(info, event_ring, slot_id, CONTROL_ENDPOINT_DCI, status_phys)?;
    if !completion_is_success_like(transfer.completion_code) {
        println!(
            "[xhci] control IN failed slot={} req={:#x} value={:#x} code={} ptr={:#x} residual={}",
            slot_id,
            request,
            value,
            transfer.completion_code,
            transfer.ptr,
            transfer.residual,
        );
        return Err("control IN transfer failed");
    }

    let mut bytes = Vec::with_capacity(len);
    bytes.resize(len, 0);
    for (idx, byte) in bytes.iter_mut().enumerate() {
        *byte = unsafe { core::ptr::read_volatile((buffer_virt + idx as u64) as *const u8) };
    }

    Ok(bytes)
}

fn control_transfer_no_data(
    info: &XhciInfo,
    event_ring: &mut EventRingState,
    slot_id: u8,
    ring: &mut TransferRingState,
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
) -> Result<(), &'static str> {
    let setup = usb_setup_packet(request_type, request, value, index, 0);
    let _setup_phys = push_transfer_trb(
        ring,
        setup,
        8,
        (TRB_TYPE_SETUP_STAGE << 10) | TRB_IDT | TRB_TRT_NONE,
    );
    let status_phys = push_transfer_trb(
        ring,
        0,
        0,
        (TRB_TYPE_STATUS_STAGE << 10) | TRB_IOC | TRB_DIR_IN,
    );

    ring_device_doorbell(info, slot_id, CONTROL_ENDPOINT_DCI);

    let transfer =
        wait_for_transfer_completion(info, event_ring, slot_id, CONTROL_ENDPOINT_DCI, status_phys)?;
    if !completion_is_success_like(transfer.completion_code) {
        println!(
            "[xhci] control no-data failed slot={} req={:#x} value={:#x} code={} ptr={:#x}",
            slot_id,
            request,
            value,
            transfer.completion_code,
            transfer.ptr,
        );
        return Err("control transfer failed");
    }

    Ok(())
}

fn read_descriptor(
    info: &XhciInfo,
    event_ring: &mut EventRingState,
    slot_id: u8,
    ring: &mut TransferRingState,
    buffer_phys: u64,
    buffer_virt: u64,
    descriptor_type: u16,
    descriptor_index: u8,
    recipient: u8,
    index: u16,
    len: usize,
) -> Result<Vec<u8>, &'static str> {
    control_transfer_in(
        info,
        event_ring,
        slot_id,
        ring,
        buffer_phys,
        buffer_virt,
        REQUEST_TYPE_IN | REQUEST_TYPE_STANDARD | recipient,
        SETUP_GET_DESCRIPTOR,
        (descriptor_type << 8) | descriptor_index as u16,
        index,
        len,
    )
}

fn parse_boot_hid_interfaces(config: &[u8]) -> Vec<HidInterface> {
    let mut interfaces = Vec::new();
    let mut current_number = 0u8;
    let mut current_alternate_setting = 0u8;
    let mut current_class = 0u8;
    let mut current_subclass = 0u8;
    let mut current_protocol = 0u8;
    let mut current_report_descriptor_len = 0u16;
    let mut offset = 0usize;

    while offset + 2 <= config.len() {
        let len = config[offset] as usize;
        if len < 2 || offset + len > config.len() {
            break;
        }

        match config[offset + 1] {
            USB_DESC_TYPE_INTERFACE if len >= 9 => {
                current_number = config[offset + 2];
                current_alternate_setting = config[offset + 3];
                current_class = config[offset + 5];
                current_subclass = config[offset + 6];
                current_protocol = config[offset + 7];
                current_report_descriptor_len = 0;
            }
            DESCRIPTOR_TYPE_HID if len >= 9 && current_class == USB_CLASS_HID => {
                let descriptor_count = config[offset + 5] as usize;
                let mut desc_off = offset + 6;
                for _ in 0..descriptor_count {
                    if desc_off + 3 > offset + len {
                        break;
                    }
                    let desc_type = config[desc_off];
                    let desc_len = u16::from_le_bytes([config[desc_off + 1], config[desc_off + 2]]);
                    if desc_type == DESCRIPTOR_TYPE_REPORT {
                        current_report_descriptor_len = desc_len;
                        break;
                    }
                    desc_off += 3;
                }
            }
            USB_DESC_TYPE_ENDPOINT
                if len >= 7
                    && current_class == USB_CLASS_HID
                    && current_subclass == USB_HID_SUBCLASS_BOOT
                    && (current_protocol == USB_HID_PROTOCOL_KEYBOARD
                        || current_protocol == USB_HID_PROTOCOL_MOUSE) =>
            {
                let endpoint_address = config[offset + 2];
                let attributes = config[offset + 3] & 0x03;
                if endpoint_address & 0x80 != 0 && attributes == USB_ENDPOINT_ATTR_INTERRUPT {
                    interfaces.push(HidInterface {
                        number: current_number,
                        alternate_setting: current_alternate_setting,
                        protocol: current_protocol,
                        endpoint_address,
                        max_packet_size: u16::from_le_bytes([
                            config[offset + 4],
                            config[offset + 5],
                        ]) & 0x07ff,
                        interval: config[offset + 6],
                        report_descriptor_len: current_report_descriptor_len,
                    });
                }
            }
            _ => {}
        }

        offset += len;
    }

    interfaces
}

fn usb_setup_packet(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> u64 {
    request_type as u64
        | ((request as u64) << 8)
        | ((value as u64) << 16)
        | ((index as u64) << 32)
        | ((length as u64) << 48)
}

fn max_packet_size0_from_descriptor(usb_bcd: u16, raw: u8) -> u16 {
    if usb_bcd >= 0x0300 {
        match raw {
            0..=15 => 1u16 << raw,
            _ => 0,
        }
    } else {
        raw as u16
    }
}

fn hid_protocol_name(protocol: u8) -> &'static str {
    match protocol {
        USB_HID_PROTOCOL_KEYBOARD => "keyboard",
        USB_HID_PROTOCOL_MOUSE => "mouse",
        _ => "unknown",
    }
}

fn interrupt_report_len(hid: &HidInterface) -> usize {
    match hid.protocol {
        USB_HID_PROTOCOL_KEYBOARD => BOOT_KEYBOARD_REPORT_BYTES,
        USB_HID_PROTOCOL_MOUSE => BOOT_MOUSE_REPORT_BYTES.min(hid.max_packet_size as usize).max(3),
        _ => hid.max_packet_size as usize,
    }
}

fn endpoint_dci(endpoint_address: u8) -> u8 {
    let ep_num = endpoint_address & 0x0f;
    if ep_num == 0 {
        CONTROL_ENDPOINT_DCI
    } else {
        ep_num * 2 + ((endpoint_address >> 7) & 0x1)
    }
}

fn interrupt_interval(speed_id: u8, interval: u8) -> u8 {
    let raw = interval.max(1);
    if speed_id >= 3 {
        raw.saturating_sub(1).min(15)
    } else {
        raw.saturating_sub(1).min(15)
    }
}

fn default_control_mps(speed_id: u8) -> u16 {
    match speed_id {
        1 | 2 => 8,
        3 => 64,
        4..=15 => 512,
        _ => 0,
    }
}

fn read_portsc(info: &XhciInfo, port_num: u8) -> u32 {
    read_portsc_by_op_base(info.op_base, port_num)
}

fn portsc_addr(info: &XhciInfo, port_num: u8) -> u64 {
    portsc_addr_by_op_base(info.op_base, port_num)
}

fn portsc_addr_by_op_base(op_base: u64, port_num: u8) -> u64 {
    op_base + OP_PORTSC_BASE + 0x10 * (port_num as u64 - 1)
}

fn read_portsc_by_op_base(op_base: u64, port_num: u8) -> u32 {
    unsafe { read_u32(portsc_addr_by_op_base(op_base, port_num)) }
}

fn port_speed_id(portsc: u32) -> u8 {
    ((portsc & PORTSC_SPEED_MASK) >> PORTSC_SPEED_SHIFT) as u8
}

fn clear_port_changes(info: &XhciInfo, port_num: u8) {
    clear_port_changes_by_op_base(info.op_base, port_num);
}

fn clear_port_changes_by_op_base(op_base: u64, port_num: u8) {
    let portsc = read_portsc_by_op_base(op_base, port_num);
    let change_bits = portsc & PORTSC_CHANGE_BITS;
    if change_bits == 0 {
        return;
    }

    unsafe {
        write_u32(
            portsc_addr_by_op_base(op_base, port_num),
            (portsc & PORTSC_PP) | change_bits,
        );
    }
}

fn reset_port(
    info: &XhciInfo,
    event_ring: &mut EventRingState,
    port_num: u8,
) -> Result<u32, &'static str> {
    let portsc = read_portsc(info, port_num);
    unsafe {
        write_u32(
            portsc_addr(info, port_num),
            (portsc & PORTSC_PP) | (portsc & PORTSC_CHANGE_BITS) | PORTSC_PR,
        );
    }

    let _ = wait_for_port_status_change(info, event_ring, port_num)?;
    clear_port_changes(info, port_num);
    if read_portsc(info, port_num) & PORTSC_PR != 0 {
        wait_until("port reset clear", || {
            read_portsc(info, port_num) & PORTSC_PR == 0
        })?;
    }

    Ok(read_portsc(info, port_num))
}

fn wait_for_port_status_change(
    info: &XhciInfo,
    event_ring: &mut EventRingState,
    port_num: u8,
) -> Result<u32, &'static str> {
    for _ in 0..SPIN_TIMEOUT {
        if let Some(event) = next_event(info, event_ring) {
            match event.trb_type() as u32 {
                TRB_TYPE_PORT_STATUS_CHANGE => {
                    if event.port_id() == port_num {
                        return Ok(read_portsc(info, port_num));
                    }
                    println!(
                        "[xhci] ignoring port change for port {} while waiting on port {}",
                        event.port_id(),
                        port_num,
                    );
                }
                TRB_TYPE_CMD_COMPLETION => {
                    println!(
                        "[xhci] unexpected command completion while waiting for port {} ptr={:#x} code={} slot={}",
                        port_num,
                        event.parameter & !0xFu64,
                        event.completion_code(),
                        event.slot_id(),
                    );
                }
                _ => {
                    println!(
                        "[xhci] unexpected event while waiting for port {} type={} code={} param={:#x}",
                        port_num,
                        event.trb_type(),
                        event.completion_code(),
                        event.parameter,
                    );
                }
            }
        }
        core::hint::spin_loop();
    }

    println!(
        "[xhci] timeout while waiting for port {} status change",
        port_num
    );
    Err("port change timeout")
}

fn wait_for_transfer_completion(
    info: &XhciInfo,
    event_ring: &mut EventRingState,
    slot_id: u8,
    endpoint_id: u8,
    expected_trb_phys: u64,
) -> Result<TransferCompletion, &'static str> {
    for _ in 0..SPIN_TIMEOUT {
        if let Some(event) = next_event(info, event_ring) {
            match event.trb_type() as u32 {
                TRB_TYPE_TRANSFER_EVENT => {
                    let completion = TransferCompletion {
                        ptr: event.parameter & !0xFu64,
                        completion_code: event.completion_code(),
                        slot_id: event.slot_id(),
                        endpoint_id: event.endpoint_id(),
                        residual: event.residual(),
                    };

                    if completion.slot_id == slot_id && completion.endpoint_id == endpoint_id {
                        if completion.ptr == expected_trb_phys
                            || !completion_is_success_like(completion.completion_code)
                        {
                            return Ok(completion);
                        }
                        println!(
                            "[xhci] transfer event slot={} ep={} ptr={:#x} code={} residual={}",
                            completion.slot_id,
                            completion.endpoint_id,
                            completion.ptr,
                            completion.completion_code,
                            completion.residual,
                        );
                    }
                }
                TRB_TYPE_PORT_STATUS_CHANGE => {
                    println!(
                        "[xhci] event: port {} changed during transfer",
                        event.port_id()
                    );
                }
                TRB_TYPE_CMD_COMPLETION => {
                    println!(
                        "[xhci] unexpected command completion during transfer ptr={:#x} code={} slot={}",
                        event.parameter & !0xFu64,
                        event.completion_code(),
                        event.slot_id(),
                    );
                }
                _ => {
                    println!(
                        "[xhci] unexpected event during transfer type={} code={} param={:#x} status={:#x}",
                        event.trb_type(),
                        event.completion_code(),
                        event.parameter,
                        event.status,
                    );
                }
            }
        }
        core::hint::spin_loop();
    }

    println!(
        "[xhci] timeout while waiting for transfer completion slot={} ep={} ptr={:#x}",
        slot_id, endpoint_id, expected_trb_phys,
    );
    Err("transfer completion timeout")
}

fn ring_host_doorbell(info: &XhciInfo) {
    fence(Ordering::SeqCst);
    unsafe {
        write_u32(info.db_base, 0);
    }
}

fn ring_device_doorbell(info: &XhciInfo, slot_id: u8, dci: u8) {
    fence(Ordering::SeqCst);
    unsafe {
        write_u32(info.db_base + slot_id as u64 * 4, dci as u32);
    }
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

fn push_transfer_trb(
    ring: &mut TransferRingState,
    parameter: u64,
    status: u32,
    control: u32,
) -> u64 {
    let trb_phys = ring.phys + ring.enqueue_idx as u64 * 16;
    let trb_virt = ring.virt + ring.enqueue_idx as u64 * 16;
    let control = control | if ring.cycle { TRB_CYCLE } else { 0 };

    unsafe {
        write_u64(trb_virt, parameter);
        write_u32(trb_virt + 8, status);
        write_u32(trb_virt + 12, control);
    }

    ring.enqueue_idx += 1;
    if ring.enqueue_idx == CONTROL_RING_TRBS - 1 {
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
    next_event_by_base(info.rt_base, ring)
}

fn next_event_by_base(rt_base: u64, ring: &mut EventRingState) -> Option<EventTrb> {
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
    advance_event_ring_by_base(rt_base, ring);
    Some(event)
}

fn advance_event_ring_by_base(rt_base: u64, ring: &mut EventRingState) {
    ring.dequeue_idx += 1;
    if ring.dequeue_idx == EVENT_RING_TRBS {
        ring.dequeue_idx = 0;
        ring.cycle = !ring.cycle;
    }

    let erdp = ring.phys + ring.dequeue_idx as u64 * 16;
    unsafe {
        write_u64(rt_base + RT_IR0 + IR0_ERDP, erdp | ERDP_EHB_CLEAR);
    }
}

fn handle_runtime_transfer_event(runtime: &mut ActiveState, event: EventTrb) {
    let completion = TransferCompletion {
        ptr: event.parameter & !0xFu64,
        completion_code: event.completion_code(),
        slot_id: event.slot_id(),
        endpoint_id: event.endpoint_id(),
        residual: event.residual(),
    };

    let Some(device) = runtime.devices.iter_mut().find(|device| {
        device.slot_id == completion.slot_id && device.endpoint_dci == completion.endpoint_id
    }) else {
        println!(
            "[xhci] runtime transfer for unknown device slot={} ep={} ptr={:#x} code={}",
            completion.slot_id,
            completion.endpoint_id,
            completion.ptr,
            completion.completion_code,
        );
        return;
    };

    if completion.ptr != device.report_trb_phys {
        device.error_count = device.error_count.saturating_add(1);
        device.last_completion_code = completion.completion_code;
        runtime.last_runtime_note = format!(
            "slot {} ep {} ptr mismatch",
            completion.slot_id,
            completion.endpoint_id,
        );
        println!(
            "[xhci] runtime transfer ptr mismatch slot={} ep={} got={:#x} expected={:#x}",
            completion.slot_id,
            completion.endpoint_id,
            completion.ptr,
            device.report_trb_phys,
        );
        return;
    }

    device.last_completion_code = completion.completion_code;
    if !completion_is_success_like(completion.completion_code) {
        device.error_count = device.error_count.saturating_add(1);
        runtime.last_runtime_note = format!(
            "slot {} ep {} code {}",
            completion.slot_id,
            completion.endpoint_id,
            completion.completion_code,
        );
        println!(
            "[xhci] HID transfer failed slot={} ep={} code={}",
            completion.slot_id,
            completion.endpoint_id,
            completion.completion_code,
        );
        return;
    }

    let actual_len = device
        .report_request_len
        .saturating_sub(completion.residual as usize);
    device.report_count = device.report_count.saturating_add(1);
    device.last_report_len = actual_len;
    runtime.last_runtime_note = format!(
        "slot {} {} {}B",
        device.slot_id,
        hid_protocol_name(device.protocol),
        actual_len,
    );
    dispatch_hid_report(device, actual_len);

    if let Err(err) = queue_interrupt_transfer_by_base(runtime.db_base, device) {
        device.error_count = device.error_count.saturating_add(1);
        runtime.last_runtime_note = format!(
            "slot {} ep {} requeue {}",
            device.slot_id,
            device.endpoint_dci,
            err,
        );
        println!(
            "[xhci] failed to requeue HID transfer slot={} ep={} err={}",
            device.slot_id, device.endpoint_dci, err
        );
    }
}

fn dispatch_hid_report(device: &mut HidDeviceState, actual_len: usize) {
    let mut report = [0u8; BOOT_KEYBOARD_REPORT_BYTES];
    let len = actual_len.min(device.report_request_len).min(report.len());
    for (idx, byte) in report.iter_mut().take(len).enumerate() {
        *byte = unsafe { core::ptr::read_volatile((device.report_buffer_virt + idx as u64) as *const u8) };
    }

    match device.protocol {
        USB_HID_PROTOCOL_KEYBOARD if len >= BOOT_KEYBOARD_REPORT_BYTES => {
            crate::keyboard::handle_usb_boot_report(&report);
        }
        USB_HID_PROTOCOL_MOUSE if len >= 3 => {
            crate::mouse::handle_usb_boot_report(&report[..len]);
        }
        _ => {}
    }
}

fn queue_interrupt_transfer_by_base(
    db_base: u64,
    device: &mut HidDeviceState,
) -> Result<(), &'static str> {
    if device.report_request_len == 0 {
        return Err("interrupt report length is zero");
    }

    device.report_trb_phys = push_transfer_trb(
        &mut device.report_ring,
        device.report_buffer_phys,
        device.report_request_len as u32,
        (TRB_TYPE_NORMAL << 10) | TRB_IOC | TRB_DIR_IN,
    );
    fence(Ordering::SeqCst);
    unsafe {
        write_u32(db_base + device.slot_id as u64 * 4, device.endpoint_dci as u32);
    }
    Ok(())
}

fn completion_is_success_like(code: u8) -> bool {
    code == COMPLETION_SUCCESS || code == COMPLETION_SHORT_PACKET
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

unsafe fn zero_page(addr: u64) {
    core::ptr::write_bytes(addr as *mut u8, 0, 4096);
}

unsafe fn copy_context(src: u64, dst: u64, len: usize) {
    core::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, len);
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

fn scan_ports(info: &XhciInfo) -> Vec<String> {
    let mut status = Vec::new();
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
            status.push(format!(
                "USB: port {} {} connected={} enabled={} speed={}",
                port_num,
                proto.label,
                connected as u8,
                enabled as u8,
                port_speed_name(proto, speed_id),
            ));
        } else {
            println!(
                "[xhci] port {} proto=? ccs={} ped={} speed_id={} portsc={:#x}",
                port_num, connected as u8, enabled as u8, speed_id, portsc,
            );
            status.push(format!(
                "USB: port {} connected={} enabled={} speed_id={}",
                port_num, connected as u8, enabled as u8, speed_id,
            ));
        }
    }

    if !any {
        println!("[xhci] no active root-hub ports reported");
        status.push(String::from("USB: no active root-hub ports reported"));
    }
    status
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
