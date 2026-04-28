/// Window manager — public interface.
pub mod compositor;
pub mod window;

use core::sync::atomic::{AtomicBool, Ordering};

static REPAINT: AtomicBool = AtomicBool::new(false);

pub fn request_repaint() {
    REPAINT.store(true, Ordering::Relaxed);
}

pub fn compose_if_needed() {
    if REPAINT.swap(false, Ordering::Relaxed) {
        compositor::WM.lock().compose();
    }
}

pub fn init() {
    request_repaint();
}
