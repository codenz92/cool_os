extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static RSDP_ADDR: AtomicU64 = AtomicU64::new(0);
static RSDP_VALID: AtomicBool = AtomicBool::new(false);
static XSDT_ADDR: AtomicU64 = AtomicU64::new(0);

pub fn init(rsdp_addr: Option<u64>, phys_offset: u64) {
    crate::device_registry::register_virtual("power manager", "ACPI", "probing tables");
    if let Some(addr) = rsdp_addr {
        RSDP_ADDR.store(addr, Ordering::Relaxed);
        if unsafe { parse_rsdp(addr, phys_offset) } {
            RSDP_VALID.store(true, Ordering::Relaxed);
            crate::device_registry::register_virtual("power manager", "ACPI", "tables detected");
            crate::klog::log_owned(format!("acpi: RSDP at {:#x}", addr));
            return;
        }
    }
    crate::device_registry::register_virtual("power manager", "ACPI", "legacy reboot only");
    crate::klog::log("acpi: RSDP unavailable; legacy reboot path active");
}

pub fn status_lines() -> Vec<String> {
    let rsdp = RSDP_ADDR.load(Ordering::Relaxed);
    let xsdt = XSDT_ADDR.load(Ordering::Relaxed);
    let mut lines = Vec::new();
    if RSDP_VALID.load(Ordering::Relaxed) {
        lines.push(format!("ACPI: RSDP valid at {:#x}", rsdp));
        if xsdt != 0 {
            lines.push(format!("XSDT/RSDT pointer {:#x}", xsdt));
        }
    } else {
        lines.push(String::from("ACPI: RSDP unavailable"));
    }
    lines.push(String::from(
        "shutdown: table plumbing present, S5 not armed",
    ));
    lines.push(String::from("sleep: S-state groundwork only"));
    lines.push(String::from("reboot: legacy controller reset available"));
    lines
}

pub fn shutdown() -> Result<(), &'static str> {
    Err("ACPI shutdown table plumbing present; S5 write path not armed")
}

pub fn sleep() -> Result<(), &'static str> {
    Err("ACPI sleep groundwork present; S-state transition not armed")
}

unsafe fn parse_rsdp(rsdp_phys: u64, phys_offset: u64) -> bool {
    let ptr = (phys_offset + rsdp_phys) as *const u8;
    let signature = core::slice::from_raw_parts(ptr, 8);
    if signature != b"RSD PTR " {
        return false;
    }
    let revision = ptr.add(15).read_volatile();
    let rsdt = u32::from_le_bytes([
        ptr.add(16).read_volatile(),
        ptr.add(17).read_volatile(),
        ptr.add(18).read_volatile(),
        ptr.add(19).read_volatile(),
    ]) as u64;
    let xsdt = if revision >= 2 {
        u64::from_le_bytes([
            ptr.add(24).read_volatile(),
            ptr.add(25).read_volatile(),
            ptr.add(26).read_volatile(),
            ptr.add(27).read_volatile(),
            ptr.add(28).read_volatile(),
            ptr.add(29).read_volatile(),
            ptr.add(30).read_volatile(),
            ptr.add(31).read_volatile(),
        ])
    } else {
        rsdt
    };
    XSDT_ADDR.store(xsdt, Ordering::Relaxed);
    true
}
