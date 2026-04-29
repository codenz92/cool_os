extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

const SETTINGS_PATH: &str = "/CONFIG/SYSTEM.CFG";

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SystemSettings {
    pub diagnostics_task_snapshots: bool,
    pub diagnostics_crash_details: bool,
    pub logs_include_profiler: bool,
    pub logs_persist_kernel: bool,
    pub network_dns_enabled: bool,
    pub network_http_enabled: bool,
    pub network_offline_api: bool,
    pub storage_writeback_enabled: bool,
    pub storage_fsck_on_boot: bool,
}

const DEFAULT_SETTINGS: SystemSettings = SystemSettings {
    diagnostics_task_snapshots: true,
    diagnostics_crash_details: true,
    logs_include_profiler: true,
    logs_persist_kernel: true,
    network_dns_enabled: true,
    network_http_enabled: true,
    network_offline_api: true,
    storage_writeback_enabled: true,
    storage_fsck_on_boot: false,
};

static LOADED: AtomicBool = AtomicBool::new(false);
static SETTINGS: Mutex<SystemSettings> = Mutex::new(DEFAULT_SETTINGS);

pub fn init() {
    load_from_disk();
    let _ = save_to_disk();
}

pub fn loaded() -> bool {
    LOADED.load(Ordering::Acquire)
}

pub fn snapshot() -> SystemSettings {
    ensure_loaded();
    *SETTINGS.lock()
}

pub fn set(key: &str, value: bool) -> bool {
    ensure_loaded();
    {
        let mut settings = SETTINGS.lock();
        match key {
            "diagnostics_task_snapshots" => settings.diagnostics_task_snapshots = value,
            "diagnostics_crash_details" => settings.diagnostics_crash_details = value,
            "logs_include_profiler" => settings.logs_include_profiler = value,
            "logs_persist_kernel" => settings.logs_persist_kernel = value,
            "network_dns_enabled" => settings.network_dns_enabled = value,
            "network_http_enabled" => settings.network_http_enabled = value,
            "network_offline_api" => settings.network_offline_api = value,
            "storage_writeback_enabled" => settings.storage_writeback_enabled = value,
            "storage_fsck_on_boot" => settings.storage_fsck_on_boot = value,
            _ => return false,
        }
    }
    let _ = save_to_disk();
    crate::event_bus::emit("settings", key, if value { "on" } else { "off" });
    crate::wm::request_repaint();
    true
}

pub fn lines() -> Vec<String> {
    let settings = snapshot();
    alloc::vec![
        line(
            "diagnostics.task_snapshots",
            settings.diagnostics_task_snapshots
        ),
        line(
            "diagnostics.crash_details",
            settings.diagnostics_crash_details
        ),
        line("logs.include_profiler", settings.logs_include_profiler),
        line("logs.persist_kernel", settings.logs_persist_kernel),
        line("network.dns", settings.network_dns_enabled),
        line("network.http", settings.network_http_enabled),
        line("network.offline_api", settings.network_offline_api),
        line("storage.writeback", settings.storage_writeback_enabled),
        line("storage.fsck_on_boot", settings.storage_fsck_on_boot),
    ]
}

fn ensure_loaded() {
    if !LOADED.load(Ordering::Acquire) {
        load_from_disk();
    }
}

fn load_from_disk() {
    if LOADED.swap(true, Ordering::AcqRel) {
        return;
    }
    let mut next = DEFAULT_SETTINGS;
    if let Some(bytes) = crate::config_store::read(SETTINGS_PATH) {
        if let Ok(text) = core::str::from_utf8(&bytes) {
            for raw in text.lines() {
                let Some((key, value)) = raw.split_once('=') else {
                    continue;
                };
                let Some(enabled) = parse_bool(value) else {
                    continue;
                };
                match key.trim() {
                    "diagnostics_task_snapshots" => next.diagnostics_task_snapshots = enabled,
                    "diagnostics_crash_details" => next.diagnostics_crash_details = enabled,
                    "logs_include_profiler" => next.logs_include_profiler = enabled,
                    "logs_persist_kernel" => next.logs_persist_kernel = enabled,
                    "network_dns_enabled" => next.network_dns_enabled = enabled,
                    "network_http_enabled" => next.network_http_enabled = enabled,
                    "network_offline_api" => next.network_offline_api = enabled,
                    "storage_writeback_enabled" => next.storage_writeback_enabled = enabled,
                    "storage_fsck_on_boot" => next.storage_fsck_on_boot = enabled,
                    _ => {}
                }
            }
        } else {
            crate::config_store::recover_corrupt(SETTINGS_PATH, "/CONFIG/SYSTEM.BAD", &bytes);
        }
    }
    *SETTINGS.lock() = next;
}

fn save_to_disk() -> Result<(), crate::fat32::FsError> {
    let settings = *SETTINGS.lock();
    let mut out = String::new();
    push_setting(
        &mut out,
        "diagnostics_task_snapshots",
        settings.diagnostics_task_snapshots,
    );
    push_setting(
        &mut out,
        "diagnostics_crash_details",
        settings.diagnostics_crash_details,
    );
    push_setting(
        &mut out,
        "logs_include_profiler",
        settings.logs_include_profiler,
    );
    push_setting(
        &mut out,
        "logs_persist_kernel",
        settings.logs_persist_kernel,
    );
    push_setting(
        &mut out,
        "network_dns_enabled",
        settings.network_dns_enabled,
    );
    push_setting(
        &mut out,
        "network_http_enabled",
        settings.network_http_enabled,
    );
    push_setting(
        &mut out,
        "network_offline_api",
        settings.network_offline_api,
    );
    push_setting(
        &mut out,
        "storage_writeback_enabled",
        settings.storage_writeback_enabled,
    );
    push_setting(
        &mut out,
        "storage_fsck_on_boot",
        settings.storage_fsck_on_boot,
    );
    crate::config_store::safe_write(SETTINGS_PATH, out.as_bytes())
}

fn push_setting(out: &mut String, key: &str, value: bool) {
    out.push_str(key);
    out.push('=');
    out.push_str(if value { "true" } else { "false" });
    out.push('\n');
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn line(label: &str, value: bool) -> String {
    format!("{}={}", label, if value { "on" } else { "off" })
}
