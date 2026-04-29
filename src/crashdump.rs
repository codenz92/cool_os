extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::panic::PanicInfo;
use spin::Mutex;

const CRASH_PATH: &str = "/LOGS/CRASH.TXT";
const MAX_TASK_REPORTS: usize = 24;

static TASK_REPORTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

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
    let report = format!(
        "task pid={} reason={} tick={}",
        pid,
        reason,
        crate::interrupts::ticks()
    );
    let mut reports = TASK_REPORTS.lock();
    reports.push(report);
    if reports.len() > MAX_TASK_REPORTS {
        reports.remove(0);
    }
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
    for line in TASK_REPORTS.lock().iter().rev().take(10) {
        lines.push(String::from(line));
    }
    if !lines.is_empty() {
        return lines;
    }
    lines.push(String::from("no crash dump recorded"));
    lines
}
