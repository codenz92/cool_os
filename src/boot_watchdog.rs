extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

static LAST_TICK: AtomicU64 = AtomicU64::new(0);
static LAST_COMPLETED: AtomicUsize = AtomicUsize::new(0);
static COMPLETE: AtomicBool = AtomicBool::new(false);
static WARNED: AtomicBool = AtomicBool::new(false);

const STUCK_MS: u64 = 2500;

pub fn record(_stage: &str, completed: usize) {
    LAST_COMPLETED.store(completed, Ordering::Relaxed);
    LAST_TICK.store(crate::interrupts::ticks(), Ordering::Relaxed);
    WARNED.store(false, Ordering::Relaxed);
}

pub fn complete() {
    COMPLETE.store(true, Ordering::Release);
}

pub fn tick_from_irq(now: u64) {
    if COMPLETE.load(Ordering::Acquire) {
        return;
    }
    let last = LAST_TICK.load(Ordering::Relaxed);
    if last == 0 {
        return;
    }
    let timeout = crate::interrupts::ticks_for_millis(STUCK_MS);
    if now.wrapping_sub(last) < timeout {
        return;
    }
    if WARNED.swap(true, Ordering::AcqRel) {
        return;
    }

    debug_write("[watchdog] boot splash stalled after milestone ");
    debug_usize(LAST_COMPLETED.load(Ordering::Relaxed));
    debug_write("\n");
}

pub fn lines() -> Vec<String> {
    let complete = COMPLETE.load(Ordering::Acquire);
    let last = LAST_TICK.load(Ordering::Relaxed);
    let completed = LAST_COMPLETED.load(Ordering::Relaxed);
    alloc::vec![
        format!("boot_complete={}", if complete { "yes" } else { "no" }),
        format!("last_milestone={}", completed),
        format!("last_tick={}", last),
        format!("stuck_timeout_ms={}", STUCK_MS),
    ]
}

fn debug_write(text: &str) {
    for b in text.bytes() {
        unsafe { x86_64::instructions::port::Port::<u8>::new(0xE9).write(b) };
    }
}

fn debug_usize(mut value: usize) {
    if value == 0 {
        debug_write("0");
        return;
    }
    let mut digits = [0u8; 20];
    let mut len = 0usize;
    while value > 0 {
        digits[len] = b'0' + (value % 10) as u8;
        value /= 10;
        len += 1;
    }
    for idx in (0..len).rev() {
        unsafe { x86_64::instructions::port::Port::<u8>::new(0xE9).write(digits[idx]) };
    }
}
