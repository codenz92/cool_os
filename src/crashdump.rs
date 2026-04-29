extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::panic::PanicInfo;

const CRASH_PATH: &str = "/LOGS/CRASH.TXT";

pub fn record_panic(info: &PanicInfo) {
    let _ = crate::fat32::create_dir("/LOGS");
    let mut out = String::new();
    out.push_str("coolOS crash dump\n");
    out.push_str("panic=");
    let _ = core::fmt::Write::write_fmt(&mut out, format_args!("{}", info));
    out.push('\n');
    out.push_str("registers=unavailable outside exception frame\n");
    out.push_str("last_log_lines:\n");
    for line in crate::klog::lines().iter() {
        out.push_str(line);
        out.push('\n');
    }
    let _ = crate::fat32::safe_write_file(CRASH_PATH, out.as_bytes());
}

pub fn record_task_report(pid: usize, reason: &str) {
    let _ = crate::fat32::create_dir("/LOGS");
    let path = format!("/LOGS/TASK{}.TXT", pid);
    let data = format!(
        "pid={}\nreason={}\ntick={}\n",
        pid,
        reason,
        crate::interrupts::ticks()
    );
    let _ = crate::fat32::safe_write_file(&path, data.as_bytes());
}

pub fn lines() -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(bytes) = crate::fat32::read_file(CRASH_PATH) {
        if let Ok(text) = core::str::from_utf8(&bytes) {
            for line in text.lines().take(10) {
                lines.push(String::from(line));
            }
            return lines;
        }
    }
    lines.push(String::from("no crash dump recorded"));
    lines
}
