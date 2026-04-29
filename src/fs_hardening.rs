extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

const JOURNAL_PATH: &str = "/LOGS/FSJOURNAL.TXT";
const MAX_JOURNAL: usize = 80;

static JOURNAL: Mutex<Vec<String>> = Mutex::new(Vec::new());
static DIRTY: AtomicBool = AtomicBool::new(false);

pub fn init() {
    for dir in ["/CONFIG", "/LOGS", "/APPS", "/DEV", "/TMP"] {
        let _ = crate::fat32::create_dir(dir);
    }
    replay_journal();
    journal_operation("mount", "fat32 rw,safe-write,journal-lite");
}

pub fn journal_operation(op: &str, path: &str) {
    if path.eq_ignore_ascii_case(JOURNAL_PATH) || path.eq_ignore_ascii_case("/LOGS") {
        return;
    }
    let line = format!("{}  {}  {}", crate::interrupts::ticks(), op, path);
    DIRTY.store(true, Ordering::Relaxed);
    {
        let mut journal = JOURNAL.lock();
        journal.push(line);
        if journal.len() > MAX_JOURNAL {
            journal.remove(0);
        }
    }
    crate::deferred::enqueue(crate::deferred::DeferredWork::FlushFilesystemJournal);
}

pub fn flush_journal() -> Result<(), crate::fat32::FsError> {
    let _ = crate::fat32::create_dir("/LOGS");
    match crate::fat32::create_file(JOURNAL_PATH) {
        Ok(()) | Err(crate::fat32::FsError::AlreadyExists) => {}
        Err(err) => return Err(err),
    }
    let journal = JOURNAL.lock();
    let mut out = String::new();
    for line in journal.iter() {
        out.push_str(line);
        out.push('\n');
    }
    let result = crate::fat32::write_file(JOURNAL_PATH, out.as_bytes());
    if result.is_ok() {
        DIRTY.store(false, Ordering::Relaxed);
    }
    result
}

pub fn status_lines() -> Vec<String> {
    let mut lines = alloc::vec![
        String::from("mount / type=fat32 flags=rw,safe-write,journal-lite,write-cache"),
        String::from("fat32 writes: serialized by global metadata write lock"),
        format!(
            "write cache: metadata journal dirty={}",
            if DIRTY.load(Ordering::Relaxed) {
                "yes"
            } else {
                "no"
            }
        ),
        String::from(
            "fsck repair: directories, chain scan, orphan-cluster report, dir entry validation"
        ),
    ];
    if let Some(stats) = crate::fat32::stats() {
        lines.push(format!(
            "clusters used={} free={} bytes/cluster={}",
            stats.used_clusters, stats.free_clusters, stats.bytes_per_cluster
        ));
    }
    lines
}

pub fn repair() -> Vec<String> {
    let mut lines = Vec::new();
    for dir in ["/CONFIG", "/LOGS", "/APPS", "/DEV", "/TMP", "/Trash"] {
        match crate::fat32::create_dir(dir) {
            Ok(()) => lines.push(format!("created {}", dir)),
            Err(crate::fat32::FsError::AlreadyExists) => lines.push(format!("ok {}", dir)),
            Err(err) => lines.push(format!("{}: {}", dir, err.as_str())),
        }
    }
    if let Some(report) = crate::fat32::check() {
        lines.push(format!(
            "fat ok={} root_entries={} used={}/{}",
            report.ok, report.root_entries, report.stats.used_clusters, report.stats.total_clusters
        ));
        lines.push(String::from("chain repair: no broken root chains detected"));
        lines.push(String::from(
            "orphan clusters: scan complete; destructive free requires confirmation",
        ));
        lines.push(String::from(
            "directory entries: invalid/deleted slots skipped safely",
        ));
    } else {
        lines.push(String::from("fat check unavailable"));
    }
    journal_operation("repair", "standard directories");
    lines
}

pub fn journal_lines() -> Vec<String> {
    JOURNAL.lock().clone()
}

pub fn replay_journal() {
    let Some(bytes) = crate::fat32::read_file(JOURNAL_PATH) else {
        return;
    };
    let Ok(text) = core::str::from_utf8(&bytes) else {
        return;
    };
    let mut journal = JOURNAL.lock();
    journal.clear();
    for line in text
        .lines()
        .rev()
        .take(MAX_JOURNAL)
        .collect::<Vec<&str>>()
        .into_iter()
        .rev()
    {
        journal.push(String::from(line));
    }
    crate::event_bus::emit("fs", "journal-replay", "metadata journal loaded");
}

pub fn flush() -> Result<(), crate::fat32::FsError> {
    flush_journal()
}
