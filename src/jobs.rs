extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

const MAX_JOBS: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    #[allow(dead_code)]
    Paused,
    Cancelled,
    Complete,
    Failed,
}

impl JobState {
    pub const fn label(self) -> &'static str {
        match self {
            JobState::Running => "running",
            JobState::Paused => "paused",
            JobState::Cancelled => "cancelled",
            JobState::Complete => "complete",
            JobState::Failed => "failed",
        }
    }
}

#[derive(Clone)]
pub struct Job {
    pub id: u64,
    pub tick: u64,
    pub title: String,
    pub detail: String,
    pub progress: u8,
    pub state: JobState,
}

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);
static JOBS: Mutex<Vec<Job>> = Mutex::new(Vec::new());

pub fn start(title: &str, detail: &str) -> u64 {
    let id = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
    let mut jobs = JOBS.lock();
    jobs.push(Job {
        id,
        tick: crate::interrupts::ticks(),
        title: String::from(title),
        detail: String::from(detail),
        progress: 0,
        state: JobState::Running,
    });
    if jobs.len() > MAX_JOBS {
        jobs.remove(0);
    }
    crate::event_bus::emit("jobs", "start", title);
    crate::wm::request_repaint();
    id
}

pub fn complete(id: u64, detail: &str) {
    update(id, 100, JobState::Complete, detail);
}

pub fn progress(id: u64, progress: u8, detail: &str) {
    update(id, progress, JobState::Running, detail);
}

pub fn cancel(id: u64) -> bool {
    set_state(id, JobState::Cancelled, "cancel requested")
}

pub fn resume(id: u64) -> bool {
    set_state(id, JobState::Running, "resume requested")
}

pub fn is_cancelled(id: u64) -> bool {
    JOBS.lock()
        .iter()
        .find(|job| job.id == id)
        .map(|job| job.state == JobState::Cancelled)
        .unwrap_or(false)
}

pub fn fail(id: u64, detail: &str) {
    update(id, 100, JobState::Failed, detail);
}

#[allow(dead_code)]
pub fn recent(limit: usize) -> Vec<Job> {
    let jobs = JOBS.lock();
    let start = jobs.len().saturating_sub(limit);
    jobs[start..].to_vec()
}

pub fn lines() -> Vec<String> {
    let jobs = JOBS.lock();
    if jobs.is_empty() {
        return alloc::vec![String::from("no background jobs")];
    }
    jobs.iter()
        .rev()
        .take(12)
        .map(|job| {
            format!(
                "#{} t={} {} {}% {} - {}",
                job.id,
                job.tick,
                job.state.label(),
                job.progress,
                job.title,
                job.detail
            )
        })
        .collect()
}

fn update(id: u64, progress: u8, state: JobState, detail: &str) {
    let mut jobs = JOBS.lock();
    if let Some(job) = jobs.iter_mut().find(|job| job.id == id) {
        job.progress = progress.min(100);
        job.state = state;
        job.detail.clear();
        job.detail.push_str(detail);
        crate::event_bus::emit("jobs", state.label(), &job.title);
    }
    crate::wm::request_repaint();
}

fn set_state(id: u64, state: JobState, detail: &str) -> bool {
    let mut jobs = JOBS.lock();
    let Some(job) = jobs.iter_mut().find(|job| job.id == id) else {
        return false;
    };
    job.state = state;
    job.detail.clear();
    job.detail.push_str(detail);
    crate::event_bus::emit("jobs", state.label(), &job.title);
    crate::wm::request_repaint();
    true
}
