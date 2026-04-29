extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

const MAX_NOTIFICATIONS: usize = 24;
const HISTORY_PATH: &str = "/LOGS/NOTIFY.TXT";

#[derive(Clone)]
#[allow(dead_code)]
pub struct Notification {
    pub id: u64,
    pub tick: u64,
    pub title: String,
    pub body: String,
    pub unread: bool,
    pub dismissed: bool,
    pub action: Option<String>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static NOTIFICATIONS: Mutex<Vec<Notification>> = Mutex::new(Vec::new());

pub fn push(title: &str, body: &str) {
    push_inner(title, body, true);
}

pub fn push_transient(title: &str, body: &str) {
    push_inner(title, body, false);
}

fn push_inner(title: &str, body: &str, persist: bool) {
    let notification = Notification {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        tick: crate::interrupts::ticks(),
        title: String::from(title),
        body: String::from(body),
        unread: true,
        dismissed: false,
        action: None,
    };

    crate::klog::log_kv(title, body);
    let mut notifications = NOTIFICATIONS.lock();
    notifications.push(notification);
    if notifications.len() > MAX_NOTIFICATIONS {
        notifications.remove(0);
    }
    if persist {
        let _ = flush_history_locked(&notifications);
    }
    crate::wm::request_repaint();
}

#[allow(dead_code)]
pub fn list() -> Vec<Notification> {
    NOTIFICATIONS.lock().clone()
}

pub fn latest(limit: usize) -> Vec<Notification> {
    let notifications = NOTIFICATIONS.lock();
    let start = notifications.len().saturating_sub(limit);
    notifications[start..]
        .iter()
        .filter(|notification| !notification.dismissed)
        .cloned()
        .collect()
}

pub fn unread_count() -> usize {
    NOTIFICATIONS
        .lock()
        .iter()
        .filter(|notification| notification.unread && !notification.dismissed)
        .count()
}

pub fn mark_all_read() {
    for notification in NOTIFICATIONS.lock().iter_mut() {
        notification.unread = false;
    }
}

#[allow(dead_code)]
pub fn clear() {
    {
        NOTIFICATIONS.lock().clear();
    }
    let _ = crate::fat32::safe_write_file(HISTORY_PATH, b"");
    crate::wm::request_repaint();
}

pub fn dismiss(id: u64) -> bool {
    let mut notifications = NOTIFICATIONS.lock();
    if let Some(index) = notifications
        .iter()
        .position(|notification| notification.id == id)
    {
        notifications[index].dismissed = true;
        notifications[index].unread = false;
        let _ = flush_history_locked(&notifications);
        crate::wm::request_repaint();
        true
    } else {
        false
    }
}

pub fn dismiss_group(title: &str) -> usize {
    let mut count = 0usize;
    let mut notifications = NOTIFICATIONS.lock();
    for notification in notifications.iter_mut() {
        if notification.title.eq_ignore_ascii_case(title) {
            notification.dismissed = true;
            notification.unread = false;
            count += 1;
        }
    }
    let _ = flush_history_locked(&notifications);
    crate::wm::request_repaint();
    count
}

pub fn history_lines() -> Vec<String> {
    if let Some(bytes) = crate::fat32::read_file(HISTORY_PATH) {
        if let Ok(text) = core::str::from_utf8(&bytes) {
            return text.lines().map(String::from).collect();
        }
    }
    Vec::new()
}

fn flush_history_locked(notifications: &[Notification]) -> Result<(), crate::fat32::FsError> {
    let _ = crate::fat32::create_dir("/LOGS");
    let mut out = String::new();
    for notification in notifications.iter() {
        out.push('#');
        push_u64(&mut out, notification.id);
        out.push(' ');
        push_u64(&mut out, notification.tick);
        out.push(' ');
        out.push_str(&notification.title);
        out.push_str(": ");
        out.push_str(&notification.body);
        if notification.dismissed {
            out.push_str(" [dismissed]");
        }
        out.push('\n');
    }
    crate::fat32::safe_write_file(HISTORY_PATH, out.as_bytes())
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
