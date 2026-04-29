extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

const MAX_EVENTS: usize = 96;

#[derive(Clone)]
pub struct Event {
    pub id: u64,
    pub tick: u64,
    pub source: String,
    pub kind: String,
    pub detail: String,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());

pub fn emit(source: &str, kind: &str, detail: &str) {
    let event = Event {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        tick: crate::interrupts::ticks(),
        source: String::from(source),
        kind: String::from(kind),
        detail: String::from(detail),
    };
    crate::klog::log_owned(format!("event {} {} {}", source, kind, detail));
    let mut events = EVENTS.lock();
    events.push(event);
    if events.len() > MAX_EVENTS {
        events.remove(0);
    }
}

pub fn recent(limit: usize) -> Vec<Event> {
    let events = EVENTS.lock();
    let start = events.len().saturating_sub(limit);
    events[start..].to_vec()
}

pub fn lines(limit: usize) -> Vec<String> {
    recent(limit)
        .iter()
        .map(|event| {
            format!(
                "#{} t={} {}:{} {}",
                event.id, event.tick, event.source, event.kind, event.detail
            )
        })
        .collect()
}
