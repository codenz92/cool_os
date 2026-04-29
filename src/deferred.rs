extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

const MAX_QUEUE: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DeferredWork {
    FlushKernelLog,
    FlushFilesystemJournal,
    FlushWriteback,
    PersistTaskSnapshot,
    RefreshSearchIndex,
    UpdateSearchIndex,
}

impl DeferredWork {
    fn label(self) -> &'static str {
        match self {
            DeferredWork::FlushKernelLog => "flush-kernel-log",
            DeferredWork::FlushFilesystemJournal => "flush-fs-journal",
            DeferredWork::FlushWriteback => "flush-writeback",
            DeferredWork::PersistTaskSnapshot => "persist-task-table",
            DeferredWork::RefreshSearchIndex => "refresh-search-index",
            DeferredWork::UpdateSearchIndex => "update-search-index",
        }
    }
}

static QUEUE: Mutex<Vec<DeferredWork>> = Mutex::new(Vec::new());

pub fn enqueue(work: DeferredWork) {
    if !crate::allocator::heap_ready() {
        return;
    }
    let mut queue = QUEUE.lock();
    if queue.contains(&work) {
        return;
    }
    if queue.len() >= MAX_QUEUE {
        queue.remove(0);
    }
    queue.push(work);
    crate::profiler::record("deferred", work.label(), "queued");
}

pub fn drain_budget(max_items: usize) -> usize {
    let mut completed = 0usize;
    for _ in 0..max_items {
        let Some(work) = pop_work() else {
            break;
        };
        run(work);
        completed += 1;
    }
    completed
}

pub fn lines() -> Vec<String> {
    let queue = QUEUE.lock();
    if queue.is_empty() {
        return alloc::vec![String::from("deferred queue empty")];
    }
    queue
        .iter()
        .enumerate()
        .map(|(idx, work)| format!("{:02} {}", idx, work.label()))
        .collect()
}

fn pop_work() -> Option<DeferredWork> {
    let mut queue = QUEUE.lock();
    if queue.is_empty() {
        None
    } else {
        Some(queue.remove(0))
    }
}

fn run(work: DeferredWork) {
    match work {
        DeferredWork::FlushKernelLog => {
            let _ = crate::klog::flush_to_disk();
        }
        DeferredWork::FlushFilesystemJournal => {
            let _ = crate::fs_hardening::flush();
        }
        DeferredWork::FlushWriteback => {
            crate::writeback::drain(4);
        }
        DeferredWork::PersistTaskSnapshot => {
            if !crate::settings_state::loaded()
                || crate::settings_state::snapshot().diagnostics_task_snapshots
            {
                let _ = crate::task_snapshot::persist();
            }
        }
        DeferredWork::RefreshSearchIndex => crate::search_index::refresh(),
        DeferredWork::UpdateSearchIndex => crate::search_index::drain_changes(),
    }
    crate::profiler::record("deferred", work.label(), "ran");
}
