extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

const MAX_EVENTS: usize = 160;

#[derive(Clone)]
struct ProfileEvent {
    tick: u64,
    kind: String,
    name: String,
    detail: String,
    duration_ticks: u64,
}

static EVENTS: Mutex<Vec<ProfileEvent>> = Mutex::new(Vec::new());
static LAST_BOOT_TICK: AtomicU64 = AtomicU64::new(0);

pub fn record(kind: &str, name: &str, detail: &str) {
    record_with_duration(kind, name, detail, 0);
}

pub fn record_boot_stage(stage: &str, completed: usize) {
    if !crate::allocator::heap_ready() {
        return;
    }
    let now = crate::interrupts::ticks();
    let prev = LAST_BOOT_TICK.swap(now, Ordering::AcqRel);
    let duration = if prev == 0 { 0 } else { now.wrapping_sub(prev) };
    record_with_duration(
        "boot",
        stage,
        &format!(
            "milestone={}/{}",
            completed,
            crate::boot_splash::BOOT_PROGRESS_TOTAL
        ),
        duration,
    );
}

pub fn record_service(name: &str, state: &str) {
    record("service", name, state);
}

pub fn record_task(task_id: usize, name: &str, detail: &str) {
    record("task", name, &format!("pid={} {}", task_id, detail));
}

pub fn lines() -> Vec<String> {
    let events = EVENTS.lock();
    if events.is_empty() {
        return alloc::vec![String::from("no profiler events recorded")];
    }
    events
        .iter()
        .rev()
        .map(|event| {
            if event.duration_ticks > 0 {
                format!(
                    "{:>6}  {:<8} {:<24} +{}t  {}",
                    event.tick, event.kind, event.name, event.duration_ticks, event.detail
                )
            } else {
                format!(
                    "{:>6}  {:<8} {:<24} {}",
                    event.tick, event.kind, event.name, event.detail
                )
            }
        })
        .collect()
}

pub fn slow_lines() -> Vec<String> {
    let threshold = crate::interrupts::ticks_for_millis(300);
    let mut out = Vec::new();
    for event in EVENTS.lock().iter() {
        if event.duration_ticks >= threshold {
            out.push(format!(
                "{} {} took {} ticks",
                event.kind, event.name, event.duration_ticks
            ));
        }
    }
    if out.is_empty() {
        out.push(String::from("no slow phases above 300ms"));
    }
    out
}

fn record_with_duration(kind: &str, name: &str, detail: &str, duration_ticks: u64) {
    if !crate::allocator::heap_ready() {
        return;
    }
    let mut events = EVENTS.lock();
    events.push(ProfileEvent {
        tick: crate::interrupts::ticks(),
        kind: String::from(kind),
        name: String::from(name),
        detail: String::from(detail),
        duration_ticks,
    });
    if events.len() > MAX_EVENTS {
        events.remove(0);
    }
}
