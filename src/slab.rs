extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

const MAX_SUBSYSTEMS: usize = 16;

#[derive(Clone)]
struct SlabCounter {
    subsystem: &'static str,
    allocs: u64,
    frees: u64,
    live: u64,
    bytes_live: usize,
    high_water: usize,
}

static COUNTERS: Mutex<Vec<SlabCounter>> = Mutex::new(Vec::new());

pub fn record_alloc(subsystem: &'static str, bytes: usize) {
    update(subsystem, bytes, true);
}

pub fn record_free(subsystem: &'static str, bytes: usize) {
    update(subsystem, bytes, false);
}

pub fn lines() -> Vec<String> {
    let counters = COUNTERS.lock();
    if counters.is_empty() {
        return alloc::vec![String::from("no slab-tracked allocations yet")];
    }
    counters
        .iter()
        .map(|counter| {
            format!(
                "{} allocs={} frees={} live={} bytes_live={} high_water={}",
                counter.subsystem,
                counter.allocs,
                counter.frees,
                counter.live,
                counter.bytes_live,
                counter.high_water
            )
        })
        .collect()
}

fn update(subsystem: &'static str, bytes: usize, alloc: bool) {
    if !crate::allocator::heap_ready() {
        return;
    }
    let mut counters = COUNTERS.lock();
    if let Some(counter) = counters
        .iter_mut()
        .find(|counter| counter.subsystem == subsystem)
    {
        if alloc {
            counter.allocs = counter.allocs.saturating_add(1);
            counter.live = counter.live.saturating_add(1);
            counter.bytes_live = counter.bytes_live.saturating_add(bytes);
            counter.high_water = counter.high_water.max(counter.bytes_live);
        } else {
            counter.frees = counter.frees.saturating_add(1);
            counter.live = counter.live.saturating_sub(1);
            counter.bytes_live = counter.bytes_live.saturating_sub(bytes);
        }
        return;
    }
    if counters.len() >= MAX_SUBSYSTEMS {
        counters.remove(0);
    }
    counters.push(SlabCounter {
        subsystem,
        allocs: if alloc { 1 } else { 0 },
        frees: if alloc { 0 } else { 1 },
        live: if alloc { 1 } else { 0 },
        bytes_live: if alloc { bytes } else { 0 },
        high_water: if alloc { bytes } else { 0 },
    });
}
