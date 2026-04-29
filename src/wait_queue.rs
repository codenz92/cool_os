extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

const MAX_QUEUES: usize = 16;

#[derive(Clone)]
struct WaitQueueInfo {
    name: String,
    waiters: usize,
    waits: u64,
    wakes: u64,
    last_task: usize,
    last_tick: u64,
}

static QUEUES: Mutex<Vec<WaitQueueInfo>> = Mutex::new(Vec::new());

pub fn wait(name: &str, task_id: usize) {
    update(name, task_id, true);
}

pub fn wake(name: &str, task_id: usize) {
    update(name, task_id, false);
}

pub fn lines() -> Vec<String> {
    let queues = QUEUES.lock();
    if queues.is_empty() {
        return alloc::vec![String::from("no wait queues observed")];
    }
    queues
        .iter()
        .map(|queue| {
            format!(
                "{} waiters={} waits={} wakes={} last_task={} tick={}",
                queue.name,
                queue.waiters,
                queue.waits,
                queue.wakes,
                queue.last_task,
                queue.last_tick
            )
        })
        .collect()
}

fn update(name: &str, task_id: usize, waiting: bool) {
    if !crate::allocator::heap_ready() {
        return;
    }
    let mut queues = QUEUES.lock();
    if let Some(queue) = queues.iter_mut().find(|queue| queue.name == name) {
        if waiting {
            queue.waiters = queue.waiters.saturating_add(1);
            queue.waits = queue.waits.saturating_add(1);
        } else {
            queue.waiters = queue.waiters.saturating_sub(1);
            queue.wakes = queue.wakes.saturating_add(1);
        }
        queue.last_task = task_id;
        queue.last_tick = crate::interrupts::ticks();
        return;
    }

    if queues.len() >= MAX_QUEUES {
        queues.remove(0);
    }
    queues.push(WaitQueueInfo {
        name: String::from(name),
        waiters: if waiting { 1 } else { 0 },
        waits: if waiting { 1 } else { 0 },
        wakes: if waiting { 0 } else { 1 },
        last_task: task_id,
        last_tick: crate::interrupts::ticks(),
    });
}
