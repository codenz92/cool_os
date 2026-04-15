/// A single window managed by the compositor.

extern crate alloc;
use alloc::vec::Vec;

/// Height of the title bar in pixels (scaled 4× from the original 10 px).
pub const TITLE_H: i32 = 20;
/// Width of the close button in pixels (scaled 4×).
pub const CLOSE_W: i32 = 20;

pub struct Window {
    pub x:      i32,
    pub y:      i32,
    pub width:  i32,
    pub height: i32,
    pub title:  &'static str,
    /// Per-pixel content-area back-buffer — width × (height − TITLE_H) u32 pixels.
    pub buf:    Vec<u32>,
}

impl Window {
    /// Create a new window.  `height` includes the title bar.
    pub fn new(x: i32, y: i32, width: i32, height: i32, title: &'static str) -> Self {
        let content_h = (height - TITLE_H).max(0) as usize;
        let buf = alloc::vec![crate::framebuffer::DARK_GRAY; width as usize * content_h];
        Window { x, y, width, height, title, buf }
    }

    pub fn hit_title(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width
            && py >= self.y && py < self.y + TITLE_H
    }

    pub fn hit_close(&self, px: i32, py: i32) -> bool {
        px >= self.x + self.width - CLOSE_W && px < self.x + self.width
            && py >= self.y && py < self.y + TITLE_H
    }

    pub fn hit(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width
            && py >= self.y && py < self.y + self.height
    }
}
