extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

const MAX_INDEXED: usize = 160;
const MAX_DEPTH: usize = 5;

#[derive(Clone)]
pub struct SearchEntry {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub snippet: String,
}

static INDEX: Mutex<Vec<SearchEntry>> = Mutex::new(Vec::new());
static DIRTY_PATHS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn refresh() {
    let job = crate::jobs::start("Search index", "scanning FAT32 filenames and text snippets");
    let mut entries = Vec::new();
    scan_dir("/", 0, &mut entries);
    let count = entries.len();
    *INDEX.lock() = entries;
    crate::jobs::complete(job, "index ready");
    crate::event_bus::emit("search", "refresh", "desktop search index rebuilt");
    crate::klog::log_owned(format!("search index: {} item(s)", count));
}

pub fn record_change(path: &str) {
    if !crate::allocator::heap_ready() {
        return;
    }
    let path = crate::vfs::normalize_path(path);
    let mut dirty = DIRTY_PATHS.lock();
    if !dirty.iter().any(|entry| entry.eq_ignore_ascii_case(&path)) {
        if dirty.len() >= 64 {
            dirty.remove(0);
        }
        dirty.push(path);
    }
    crate::deferred::enqueue(crate::deferred::DeferredWork::UpdateSearchIndex);
}

pub fn drain_changes() {
    let dirty = {
        let mut dirty = DIRTY_PATHS.lock();
        if dirty.is_empty() {
            return;
        }
        core::mem::take(&mut *dirty)
    };
    if dirty.len() > 24 {
        refresh();
        return;
    }
    let mut index = INDEX.lock();
    for path in dirty {
        remove_path_prefix(&mut index, &path);
        if let Some(entry) = entry_for_path(&path) {
            index.push(entry);
        }
    }
    index.sort_by(|a, b| a.path.cmp(&b.path));
    crate::event_bus::emit("search", "incremental", "desktop search index updated");
}

pub fn search(query: &str, limit: usize) -> Vec<SearchEntry> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, SearchEntry)> = INDEX
        .lock()
        .iter()
        .filter_map(|entry| {
            let score = score_entry(entry, query)?;
            Some((score, entry.clone()))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry)
        .collect()
}

pub fn lines(query: Option<&str>) -> Vec<String> {
    if let Some(query) = query {
        let results = search(query, 12);
        if results.is_empty() {
            return alloc::vec![String::from("no matches")];
        }
        return results
            .iter()
            .map(|entry| format!("{}  {}  {}", entry.kind, entry.path, entry.snippet))
            .collect();
    }
    let index = INDEX.lock();
    alloc::vec![format!("indexed {} file/folder/app entries", index.len())]
}

fn scan_dir(path: &str, depth: usize, out: &mut Vec<SearchEntry>) {
    if depth > MAX_DEPTH || out.len() >= MAX_INDEXED {
        return;
    }
    let Some(mut entries) = crate::vfs::vfs_list_dir(path) else {
        return;
    };
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    for entry in entries {
        if out.len() >= MAX_INDEXED {
            break;
        }
        let child = join_path(path, &entry.name);
        let kind = if entry.is_dir { "dir" } else { "file" };
        let snippet = if entry.is_dir {
            String::new()
        } else {
            file_snippet(&child)
        };
        out.push(SearchEntry {
            path: child.clone(),
            name: entry.name.clone(),
            kind: String::from(kind),
            snippet,
        });
        if entry.is_dir && !is_skipped_dir(&child) {
            scan_dir(&child, depth + 1, out);
        }
    }
}

fn file_snippet(path: &str) -> String {
    let Some(bytes) = crate::vfs::vfs_read_file(path) else {
        return String::new();
    };
    let Ok(text) = core::str::from_utf8(&bytes) else {
        return String::from("binary");
    };
    let mut out = String::new();
    for c in text.chars().take(42) {
        if c == '\n' || c == '\r' || c == '\t' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

fn entry_for_path(path: &str) -> Option<SearchEntry> {
    if crate::vfs::vfs_list_dir(path).is_some() {
        return Some(SearchEntry {
            path: String::from(path),
            name: file_name(path),
            kind: String::from("dir"),
            snippet: String::new(),
        });
    }
    if crate::vfs::vfs_read_file(path).is_some() {
        return Some(SearchEntry {
            path: String::from(path),
            name: file_name(path),
            kind: String::from("file"),
            snippet: file_snippet(path),
        });
    }
    None
}

fn remove_path_prefix(index: &mut Vec<SearchEntry>, path: &str) {
    let mut prefix = String::from(path);
    if !prefix.ends_with('/') {
        prefix.push('/');
    }
    index.retain(|entry| {
        !entry.path.eq_ignore_ascii_case(path) && !starts_with_ignore_ascii(&entry.path, &prefix)
    });
}

fn starts_with_ignore_ascii(value: &str, prefix: &str) -> bool {
    value.len() >= prefix.len() && value[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn file_name(path: &str) -> String {
    String::from(path.rsplit('/').next().unwrap_or(path))
}

fn score_entry(entry: &SearchEntry, query: &str) -> Option<usize> {
    let mut score = fuzzy_score(&entry.name, query).unwrap_or(0);
    score = score.max(
        fuzzy_score(&entry.path, query)
            .unwrap_or(0)
            .saturating_sub(2),
    );
    score = score.max(
        fuzzy_score(&entry.snippet, query)
            .unwrap_or(0)
            .saturating_sub(4),
    );
    if score == 0 {
        None
    } else {
        Some(score)
    }
}

pub fn fuzzy_score(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(1);
    }
    let mut score = 0usize;
    let mut pos = 0usize;
    let hay = haystack.as_bytes();
    for nb in needle.bytes() {
        let mut found = None;
        for (idx, hb) in hay.iter().enumerate().skip(pos) {
            if hb.to_ascii_lowercase() == nb.to_ascii_lowercase() {
                found = Some(idx);
                break;
            }
        }
        let idx = found?;
        score += if idx == pos { 8 } else { 3 };
        pos = idx + 1;
    }
    if contains_ignore_ascii(haystack, needle) {
        score += 20;
    }
    Some(score)
}

fn contains_ignore_ascii(haystack: &str, needle: &str) -> bool {
    let hay = haystack.as_bytes();
    let nee = needle.as_bytes();
    if nee.is_empty() {
        return true;
    }
    if nee.len() > hay.len() {
        return false;
    }
    for start in 0..=hay.len() - nee.len() {
        let mut ok = true;
        for i in 0..nee.len() {
            if hay[start + i].to_ascii_lowercase() != nee[i].to_ascii_lowercase() {
                ok = false;
                break;
            }
        }
        if ok {
            return true;
        }
    }
    false
}

fn is_skipped_dir(path: &str) -> bool {
    let upper = path.to_ascii_uppercase();
    upper == "/DEV" || upper == "/LOGS"
}

fn join_path(parent: &str, name: &str) -> String {
    if parent == "/" {
        let mut out = String::from("/");
        out.push_str(name);
        out
    } else {
        let mut out = String::from(parent);
        out.push('/');
        out.push_str(name);
        out
    }
}
