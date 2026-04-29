extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

const MAX_PENDING: usize = 24;

#[derive(Clone)]
struct WritebackItem {
    kind: String,
    path: String,
    tick: u64,
}

struct WritebackState {
    pending: Vec<WritebackItem>,
    queued: u64,
    completed: u64,
    barriers: u64,
    failures: u64,
    last_error: Option<String>,
}

static STATE: Mutex<WritebackState> = Mutex::new(WritebackState {
    pending: Vec::new(),
    queued: 0,
    completed: 0,
    barriers: 0,
    failures: 0,
    last_error: None,
});

pub fn enqueue(kind: &str, path: &str) {
    if !crate::allocator::heap_ready() {
        return;
    }
    if crate::settings_state::loaded()
        && !crate::settings_state::snapshot().storage_writeback_enabled
    {
        return;
    }
    let mut state = STATE.lock();
    if state.pending.len() >= MAX_PENDING {
        state.pending.remove(0);
    }
    state.pending.push(WritebackItem {
        kind: String::from(kind),
        path: String::from(path),
        tick: crate::interrupts::ticks(),
    });
    state.queued = state.queued.saturating_add(1);
    crate::deferred::enqueue(crate::deferred::DeferredWork::FlushWriteback);
}

pub fn barrier() -> Result<(), &'static str> {
    if crate::settings_state::loaded()
        && !crate::settings_state::snapshot().storage_writeback_enabled
    {
        return Ok(());
    }
    {
        let mut state = STATE.lock();
        state.barriers = state.barriers.saturating_add(1);
    }
    match crate::fs_hardening::flush() {
        Ok(()) => Ok(()),
        Err(_) => {
            record_failure("writeback barrier failed");
            Err("writeback barrier failed")
        }
    }
}

pub fn drain(max_items: usize) -> usize {
    if crate::settings_state::loaded()
        && !crate::settings_state::snapshot().storage_writeback_enabled
    {
        return 0;
    }
    let mut drained = 0usize;
    for _ in 0..max_items {
        let item = {
            let mut state = STATE.lock();
            if state.pending.is_empty() {
                None
            } else {
                Some(state.pending.remove(0))
            }
        };
        let Some(item) = item else {
            break;
        };
        if crate::fs_hardening::flush().is_ok() {
            let mut state = STATE.lock();
            state.completed = state.completed.saturating_add(1);
            crate::profiler::record("writeback", &item.kind, &item.path);
        } else {
            record_failure("flush failed");
        }
        drained += 1;
    }
    drained
}

pub fn lines() -> Vec<String> {
    let state = STATE.lock();
    let mut lines = alloc::vec![format!(
        "enabled={} queued={} completed={} pending={} barriers={} failures={}",
        if crate::settings_state::snapshot().storage_writeback_enabled {
            "yes"
        } else {
            "no"
        },
        state.queued,
        state.completed,
        state.pending.len(),
        state.barriers,
        state.failures
    )];
    if let Some(err) = state.last_error.as_ref() {
        lines.push(format!("last_error={}", err));
    }
    for item in state.pending.iter().rev().take(8) {
        lines.push(format!(
            "pending {} {} tick={}",
            item.kind, item.path, item.tick
        ));
    }
    lines
}

fn record_failure(err: &str) {
    let mut state = STATE.lock();
    state.failures = state.failures.saturating_add(1);
    state.last_error = Some(String::from(err));
}
