extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

const MAX_NOTIFICATIONS: usize = 24;

#[derive(Clone)]
#[allow(dead_code)]
pub struct Notification {
    pub id: u64,
    pub tick: u64,
    pub title: String,
    pub body: String,
    pub unread: bool,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static NOTIFICATIONS: Mutex<Vec<Notification>> = Mutex::new(Vec::new());

pub fn push(title: &str, body: &str) {
    let notification = Notification {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        tick: crate::interrupts::ticks(),
        title: String::from(title),
        body: String::from(body),
        unread: true,
    };

    crate::klog::log_kv(title, body);
    let mut notifications = NOTIFICATIONS.lock();
    notifications.push(notification);
    if notifications.len() > MAX_NOTIFICATIONS {
        notifications.remove(0);
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
    notifications[start..].to_vec()
}

pub fn unread_count() -> usize {
    NOTIFICATIONS
        .lock()
        .iter()
        .filter(|notification| notification.unread)
        .count()
}

pub fn mark_all_read() {
    for notification in NOTIFICATIONS.lock().iter_mut() {
        notification.unread = false;
    }
}

#[allow(dead_code)]
pub fn clear() {
    NOTIFICATIONS.lock().clear();
    crate::wm::request_repaint();
}
