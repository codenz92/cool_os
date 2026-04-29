extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

static RESULTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn run_boot_tests() {
    let mut ok = 0usize;
    let mut fail = 0usize;
    check(
        "path-normalize",
        crate::vfs::normalize_path("/A/./B/../C") == "/A/C",
        &mut ok,
        &mut fail,
    );
    check(
        "root-normalize",
        crate::vfs::normalize_path("/../") == "/",
        &mut ok,
        &mut fail,
    );
    check(
        "syscall-range",
        crate::syscall::validate_user_range_for_test(0x1000, 16, 4096, false),
        &mut ok,
        &mut fail,
    );
    check(
        "syscall-null",
        !crate::syscall::validate_user_range_for_test(0, 16, 4096, false),
        &mut ok,
        &mut fail,
    );
    check(
        "scheduler-lifecycle",
        crate::scheduler::SCHEDULER.lock().tasks.len() >= 1,
        &mut ok,
        &mut fail,
    );
    check(
        "fat32-mutation",
        fat32_mutation_roundtrip(),
        &mut ok,
        &mut fail,
    );
    crate::println!("[selftest] kernel unit checks ok={} fail={}", ok, fail);
    crate::klog::log_owned(format!("selftest: ok={} fail={}", ok, fail));
}

pub fn lines() -> Vec<String> {
    let results = RESULTS.lock();
    if results.is_empty() {
        return alloc::vec![String::from("selftests not run")];
    }
    results.clone()
}

fn check(name: &str, passed: bool, ok: &mut usize, fail: &mut usize) {
    if passed {
        *ok += 1;
    } else {
        *fail += 1;
    }
    RESULTS
        .lock()
        .push(format!("{} {}", if passed { "ok" } else { "fail" }, name));
}

fn fat32_mutation_roundtrip() -> bool {
    let _ = crate::fat32::create_dir("/TMP");
    let path = "/TMP/SELFTEST.TXT";
    match crate::fat32::create_file(path) {
        Ok(()) | Err(crate::fat32::FsError::AlreadyExists) => {}
        Err(_) => return false,
    }
    if crate::fat32::write_file(path, b"selftest\n").is_err() {
        return false;
    }
    crate::fat32::read_file(path)
        .map(|bytes| bytes.as_slice() == b"selftest\n")
        .unwrap_or(false)
}
