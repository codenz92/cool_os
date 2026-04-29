/// Window manager — public interface.
pub mod compositor;
pub mod window;

extern crate alloc;

use alloc::string::String;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

static REPAINT: AtomicBool = AtomicBool::new(false);
static SCREENSHOT_REQUEST: Mutex<Option<String>> = Mutex::new(None);

pub fn request_repaint() {
    REPAINT.store(true, Ordering::Relaxed);
}

pub fn compose_if_needed() {
    if REPAINT.swap(false, Ordering::Relaxed) {
        compositor::WM.lock().compose();
    }
}

pub fn prepare() {
    drop(compositor::WM.lock());
}

pub fn init() {
    request_repaint();
}

pub fn request_focused_screenshot(path: &str) {
    *SCREENSHOT_REQUEST.lock() = Some(String::from(path));
    request_repaint();
}

pub(crate) fn take_screenshot_request() -> Option<String> {
    SCREENSHOT_REQUEST.lock().take()
}
