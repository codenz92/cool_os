extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::panic::PanicInfo;
use spin::Mutex;

const CRASH_PATH: &str = "/LOGS/CRASH.TXT";
const MAX_TASK_REPORTS: usize = 24;

#[derive(Clone)]
struct TaskCrashReport {
    pid: usize,
    app: String,
    reason: String,
    timestamp: String,
    tick: u64,
    registers: String,
    stack: String,
    restarts: u32,
}

static TASK_REPORTS: Mutex<Vec<TaskCrashReport>> = Mutex::new(Vec::new());

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
    let app = crate::scheduler::task_name(pid).unwrap_or("task");
    let mut reports = TASK_REPORTS.lock();
    let restarts = reports
        .iter()
        .rev()
        .find(|report| report.app.eq_ignore_ascii_case(app))
        .map(|report| report.restarts)
        .unwrap_or(0);
    reports.push(TaskCrashReport {
        pid,
        app: String::from(app),
        reason: String::from(reason),
        timestamp: timestamp_string(),
        tick: crate::interrupts::ticks(),
        registers: String::from("rip/rsp unavailable; user fault frame capture pending"),
        stack: String::from("stack trace unavailable; frame-pointer unwinder pending"),
        restarts,
    });
    if reports.len() > MAX_TASK_REPORTS {
        reports.remove(0);
    }
}

pub fn update_task_report_context(pid: usize, registers: String, stack: String) {
    let mut reports = TASK_REPORTS.lock();
    if let Some(report) = reports.iter_mut().rev().find(|report| report.pid == pid) {
        report.registers = registers;
        report.stack = stack;
    }
}

pub fn record_restart(app: &str) {
    let mut reports = TASK_REPORTS.lock();
    if let Some(report) = reports
        .iter_mut()
        .rev()
        .find(|report| report.app.eq_ignore_ascii_case(app))
    {
        report.restarts = report.restarts.saturating_add(1);
    }
}

pub fn lines() -> Vec<String> {
    compact_lines()
}

pub fn detailed_lines() -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(bytes) = crate::fat32::read_file(CRASH_PATH) {
        if let Ok(text) = core::str::from_utf8(&bytes) {
            lines.push(String::from("== panic dump =="));
            for line in text.lines().take(12) {
                lines.push(String::from(line));
            }
        }
    }

    let reports = TASK_REPORTS.lock();
    if reports.is_empty() {
        if lines.is_empty() {
            lines.push(String::from("no crash dump recorded"));
        }
        return lines;
    }

    let mut grouped_apps: Vec<String> = Vec::new();
    lines.push(String::from("== app crash reports =="));
    for report in reports.iter().rev() {
        if grouped_apps
            .iter()
            .any(|app| app.eq_ignore_ascii_case(&report.app))
        {
            continue;
        }
        grouped_apps.push(report.app.clone());
        let count = reports
            .iter()
            .filter(|item| item.app.eq_ignore_ascii_case(&report.app))
            .count();
        lines.push(format!(
            "app={} reports={} latest_pid={} restarts={}",
            report.app, count, report.pid, report.restarts
        ));
        lines.push(format!(
            "  timestamp={} tick={} reason={}",
            report.timestamp, report.tick, report.reason
        ));
        if crate::settings_state::snapshot().diagnostics_crash_details {
            lines.push(format!("  registers: {}", report.registers));
            lines.push(format!("  stack: {}", report.stack));
        }
    }

    lines
}

fn compact_lines() -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(bytes) = crate::fat32::read_file(CRASH_PATH) {
        if let Ok(text) = core::str::from_utf8(&bytes) {
            for line in text.lines().take(10) {
                lines.push(String::from(line));
            }
            return lines;
        }
    }
    let reports = TASK_REPORTS.lock();
    for report in reports.iter().rev().take(10) {
        lines.push(format!(
            "app={} pid={} time={} tick={} restarts={} reason={}",
            report.app, report.pid, report.timestamp, report.tick, report.restarts, report.reason
        ));
        lines.push(format!("  registers: {}", report.registers));
        lines.push(format!("  stack: {}", report.stack));
    }
    if !lines.is_empty() {
        return lines;
    }
    lines.push(String::from("no crash dump recorded"));
    lines
}

fn timestamp_string() -> String {
    if let Some(dt) = crate::rtc::read_datetime() {
        return format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            dt.year, dt.month, dt.day, dt.hour, dt.minute
        );
    }
    format!("tick {}", crate::interrupts::ticks())
}
