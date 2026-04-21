/// A single window managed by the compositor.

extern crate alloc;
use alloc::vec::Vec;

/// Height of the title bar in pixels.
pub const TITLE_H: i32 = 22;
/// Width of each window control button.
pub const WIN_BTN_W: i32 = 18;

pub struct Window {
    pub x:      i32,
    pub y:      i32,
    pub width:  i32,
    pub height: i32,
    pub title:  &'static str,
    /// Per-pixel content-area back-buffer.
    pub buf:    Vec<u32>,
    /// Minimized windows are hidden from desktop but stay in taskbar.
    pub minimized: bool,
    /// Saved position/size for restore after minimize or maximize.
    saved_x: i32,
    saved_y: i32,
    saved_width:  i32,
    saved_height: i32,
}

impl Window {
    pub fn new(x: i32, y: i32, width: i32, height: i32, title: &'static str) -> Self {
        let content_h = (height - TITLE_H).max(0) as usize;
        let buf = alloc::vec![crate::framebuffer::DARK_GRAY; width as usize * content_h];
        Window {
            x, y, width, height, title, buf,
            minimized: false,
            saved_x: x, saved_y: y, saved_width: width, saved_height: height,
        }
    }

    pub fn hit_title(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width
            && py >= self.y && py < self.y + TITLE_H
    }

    pub fn hit_close(&self, px: i32, py: i32) -> bool {
        px >= self.x + self.width - WIN_BTN_W && px < self.x + self.width
            && py >= self.y && py < self.y + TITLE_H
    }

    pub fn hit_minimize(&self, px: i32, py: i32) -> bool {
        px >= self.x + self.width - WIN_BTN_W * 2 && px < self.x + self.width - WIN_BTN_W
            && py >= self.y && py < self.y + TITLE_H
    }

    pub fn hit_maximize(&self, px: i32, py: i32) -> bool {
        px >= self.x + self.width - WIN_BTN_W * 3 && px < self.x + self.width - WIN_BTN_W * 2
            && py >= self.y && py < self.y + TITLE_H
    }

    pub fn hit(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width
            && py >= self.y && py < self.y + self.height
    }

    pub fn minimize(&mut self) {
        if !self.minimized {
            self.minimized = true;
            self.saved_x = self.x;
            self.saved_y = self.y;
            self.saved_width = self.width;
            self.saved_height = self.height;
        }
    }

    pub fn restore(&mut self) {
        if self.minimized {
            self.minimized = false;
            self.x = self.saved_x;
            self.y = self.saved_y;
            self.width = self.saved_width;
            self.height = self.saved_height;
            let content_h = (self.height - TITLE_H).max(0) as usize;
            let new_buf = alloc::vec![crate::framebuffer::DARK_GRAY; self.width as usize * content_h];
            self.buf = new_buf;
        }
    }

    pub fn maximize(&mut self, sw: i32, sh: i32) {
        if self.width == sw && self.height == sh {
            self.x = self.saved_x;
            self.y = self.saved_y;
            self.width = self.saved_width;
            self.height = self.saved_height;
        } else {
            self.saved_x = self.x;
            self.saved_y = self.y;
            self.saved_width = self.width;
            self.saved_height = self.height;
            self.x = 0;
            self.y = 0;
            self.width = sw;
            self.height = sh;
        }
        let content_h = (self.height - TITLE_H).max(0) as usize;
        let new_buf = alloc::vec![crate::framebuffer::DARK_GRAY; self.width as usize * content_h];
        self.buf = new_buf;
    }
}
