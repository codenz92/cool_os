extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static RSDP_ADDR: AtomicU64 = AtomicU64::new(0);
static RSDP_VALID: AtomicBool = AtomicBool::new(false);
static XSDT_ADDR: AtomicU64 = AtomicU64::new(0);
static PHYS_OFFSET: AtomicU64 = AtomicU64::new(0);
static FADT_ADDR: AtomicU64 = AtomicU64::new(0);
static PM1A_CNT_BLK: AtomicU64 = AtomicU64::new(0);
static RESET_REG_ADDR: AtomicU64 = AtomicU64::new(0);
static RESET_VALUE: AtomicU64 = AtomicU64::new(0);

pub fn init(rsdp_addr: Option<u64>, phys_offset: u64) {
    PHYS_OFFSET.store(phys_offset, Ordering::Relaxed);
    crate::device_registry::register_virtual("power manager", "ACPI", "probing tables");
    if let Some(addr) = rsdp_addr {
        RSDP_ADDR.store(addr, Ordering::Relaxed);
        if unsafe { parse_rsdp(addr, phys_offset) } {
            unsafe { discover_fadt(phys_offset) };
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
        let fadt = FADT_ADDR.load(Ordering::Relaxed);
        if fadt != 0 {
            lines.push(format!("FADT at {:#x}", fadt));
            lines.push(format!(
                "PM1a_CNT={:#x} reset_reg={:#x} reset_value={:#x}",
                PM1A_CNT_BLK.load(Ordering::Relaxed),
                RESET_REG_ADDR.load(Ordering::Relaxed),
                RESET_VALUE.load(Ordering::Relaxed)
            ));
        }
    } else {
        lines.push(String::from("ACPI: RSDP unavailable"));
    }
    lines.push(String::from(
        "shutdown: FADT parsed; S5 AML decode still guarded",
    ));
    lines.push(String::from("sleep: S-state groundwork only"));
    lines.push(String::from("reboot: legacy controller reset available"));
    lines
}

pub fn shutdown() -> Result<(), &'static str> {
    Err("FADT parsed; S5 requires AML _S5 decode before PM1 write")
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

unsafe fn discover_fadt(phys_offset: u64) {
    let root = XSDT_ADDR.load(Ordering::Relaxed);
    if root == 0 {
        return;
    }
    let root_ptr = (phys_offset + root) as *const u8;
    let is_xsdt = table_signature(root_ptr) == *b"XSDT";
    let is_rsdt = table_signature(root_ptr) == *b"RSDT";
    if !is_xsdt && !is_rsdt {
        return;
    }
    let len = u32::from_le_bytes([
        root_ptr.add(4).read_volatile(),
        root_ptr.add(5).read_volatile(),
        root_ptr.add(6).read_volatile(),
        root_ptr.add(7).read_volatile(),
    ]) as usize;
    let entry_size = if is_xsdt { 8 } else { 4 };
    let count = len.saturating_sub(36) / entry_size;
    for idx in 0..count.min(64) {
        let off = 36 + idx * entry_size;
        let table_phys = if is_xsdt {
            u64::from_le_bytes([
                root_ptr.add(off).read_volatile(),
                root_ptr.add(off + 1).read_volatile(),
                root_ptr.add(off + 2).read_volatile(),
                root_ptr.add(off + 3).read_volatile(),
                root_ptr.add(off + 4).read_volatile(),
                root_ptr.add(off + 5).read_volatile(),
                root_ptr.add(off + 6).read_volatile(),
                root_ptr.add(off + 7).read_volatile(),
            ])
        } else {
            u32::from_le_bytes([
                root_ptr.add(off).read_volatile(),
                root_ptr.add(off + 1).read_volatile(),
                root_ptr.add(off + 2).read_volatile(),
                root_ptr.add(off + 3).read_volatile(),
            ]) as u64
        };
        let table_ptr = (phys_offset + table_phys) as *const u8;
        let sig = table_signature(table_ptr);
        if sig == *b"FACP" {
            FADT_ADDR.store(table_phys, Ordering::Relaxed);
            parse_fadt(table_ptr);
            return;
        }
    }
}

unsafe fn parse_fadt(ptr: *const u8) {
    let pm1a = u32::from_le_bytes([
        ptr.add(64).read_volatile(),
        ptr.add(65).read_volatile(),
        ptr.add(66).read_volatile(),
        ptr.add(67).read_volatile(),
    ]) as u64;
    PM1A_CNT_BLK.store(pm1a, Ordering::Relaxed);
    let reset_addr = u64::from_le_bytes([
        ptr.add(120).read_volatile(),
        ptr.add(121).read_volatile(),
        ptr.add(122).read_volatile(),
        ptr.add(123).read_volatile(),
        ptr.add(124).read_volatile(),
        ptr.add(125).read_volatile(),
        ptr.add(126).read_volatile(),
        ptr.add(127).read_volatile(),
    ]);
    RESET_REG_ADDR.store(reset_addr, Ordering::Relaxed);
    RESET_VALUE.store(ptr.add(128).read_volatile() as u64, Ordering::Relaxed);
}

unsafe fn table_signature(ptr: *const u8) -> [u8; 4] {
    [
        ptr.add(0).read_volatile(),
        ptr.add(1).read_volatile(),
        ptr.add(2).read_volatile(),
        ptr.add(3).read_volatile(),
    ]
}
