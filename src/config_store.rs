extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

pub const CONFIG_DIR: &str = "/CONFIG";

static READS: AtomicU64 = AtomicU64::new(0);
static WRITES: AtomicU64 = AtomicU64::new(0);
static RECOVERIES: AtomicU64 = AtomicU64::new(0);

pub fn ensure_dir() {
    let _ = crate::fat32::create_dir(CONFIG_DIR);
}

pub fn read(path: &str) -> Option<Vec<u8>> {
    READS.fetch_add(1, Ordering::Relaxed);
    crate::fat32::read_file(path)
}

pub fn safe_write(path: &str, data: &[u8]) -> Result<(), crate::fat32::FsError> {
    ensure_dir();
    WRITES.fetch_add(1, Ordering::Relaxed);
    crate::fat32::safe_write_file(path, data)
}

pub fn write_default(path: &str, data: &[u8]) -> Result<(), crate::fat32::FsError> {
    ensure_dir();
    match crate::fat32::create_file(path) {
        Ok(()) => safe_write(path, data),
        Err(crate::fat32::FsError::AlreadyExists) => Ok(()),
        Err(err) => Err(err),
    }
}

pub fn recover_corrupt(path: &str, backup_path: &str, data: &[u8]) {
    ensure_dir();
    RECOVERIES.fetch_add(1, Ordering::Relaxed);
    let _ = crate::fat32::safe_write_file(backup_path, data);
    crate::klog::log_owned(format!("recovered corrupt {}", path));
}

pub fn lines() -> Vec<String> {
    let mut lines = alloc::vec![
        format!(
            "config store: reads={} writes={} recoveries={}",
            READS.load(Ordering::Relaxed),
            WRITES.load(Ordering::Relaxed),
            RECOVERIES.load(Ordering::Relaxed)
        ),
        String::from("path /CONFIG uses temp-write + rename via safe_write_file"),
    ];
    for path in [
        "/CONFIG/DESK.CFG",
        "/CONFIG/ICONS.CFG",
        "/CONFIG/APPS.CFG",
        "/CONFIG/ACCESS.CFG",
        "/CONFIG/SHORTCUT.CFG",
        "/CONFIG/SESSION.CFG",
    ] {
        let status = if crate::fat32::read_file(path).is_some() {
            "present"
        } else {
            "default"
        };
        lines.push(format!("{} {}", path, status));
    }
    lines
}
