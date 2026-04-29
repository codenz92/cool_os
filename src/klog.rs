extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

const LOG_DIR: &str = "/LOGS";
const LOG_PATH: &str = "/LOGS/KERNEL.TXT";
const MAX_LINES: usize = 96;

static LOG_LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn init() {
    log("kernel log initialized");
    let _ = flush_to_disk();
}

pub fn log(message: &str) {
    let ticks = crate::interrupts::ticks();
    let mut line = String::new();
    push_u64(&mut line, ticks);
    line.push_str("  ");
    line.push_str(message);

    let mut lines = LOG_LINES.lock();
    lines.push(line);
    if lines.len() > MAX_LINES {
        lines.remove(0);
    }
}

pub fn log_owned(message: String) {
    log(&message);
}

pub fn lines() -> Vec<String> {
    LOG_LINES.lock().clone()
}

pub fn flush_to_disk() -> Result<(), crate::fat32::FsError> {
    if crate::allocator::heap_ready()
        && crate::settings_state::loaded()
        && !crate::settings_state::snapshot().logs_persist_kernel
    {
        return Ok(());
    }
    let _ = crate::fat32::create_dir(LOG_DIR);
    match crate::fat32::create_file(LOG_PATH) {
        Ok(()) | Err(crate::fat32::FsError::AlreadyExists) => {}
        Err(err) => return Err(err),
    }

    let lines = LOG_LINES.lock();
    let mut out = String::new();
    for line in lines.iter() {
        out.push_str(line);
        out.push('\n');
    }
    crate::fat32::safe_write_file(LOG_PATH, out.as_bytes())
}

pub fn dump_to_console() {
    crate::println!("--- kernel log tail ---");
    for line in LOG_LINES.lock().iter() {
        crate::println!("{}", line);
    }
}

pub fn log_kv(key: &str, value: &str) {
    log_owned(format!("{}: {}", key, value));
}

fn push_u64(out: &mut String, mut value: u64) {
    if value == 0 {
        out.push('0');
        return;
    }
    let mut digits = [0u8; 20];
    let mut len = 0usize;
    while value > 0 {
        digits[len] = b'0' + (value % 10) as u8;
        value /= 10;
        len += 1;
    }
    for idx in (0..len).rev() {
        out.push(digits[idx] as char);
    }
}
