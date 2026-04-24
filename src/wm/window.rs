/// A single window managed by the compositor.
extern crate alloc;
use alloc::vec::Vec;

/// Height of the title bar in pixels.
pub const TITLE_H: i32 = 22;
/// Width of each window control button.
pub const WIN_BTN_W: i32 = 18;
/// Width of the scrollbar strip along the right edge of the content area.
pub const SCROLLBAR_W: i32 = 10;
/// Width/height of the resize grab corner (bottom-right).
pub const RESIZE_HANDLE: i32 = 8;

// ── Scroll state ──────────────────────────────────────────────────────────────

/// Tracks vertical scroll position for a window.
/// Set `content_h` from the app; the compositor draws the scrollbar automatically.
pub struct ScrollState {
    /// Current scroll offset in pixels (0 = top).
    pub offset: i32,
    /// Total logical content height set by the app.
    /// When 0 or ≤ viewport height, no scrollbar is drawn.
    pub content_h: i32,
}

impl ScrollState {
    pub fn new() -> Self {
        ScrollState {
            offset: 0,
            content_h: 0,
        }
    }

    /// True when content is taller than the visible viewport.
    #[inline]
    pub fn needs_scrollbar(&self, view_h: i32) -> bool {
        self.content_h > view_h && view_h > 0
    }

    /// Clamp offset so it never exceeds the scrollable range.
    pub fn clamp(&mut self, view_h: i32) {
        let max = (self.content_h - view_h).max(0);
        self.offset = self.offset.clamp(0, max);
    }

    /// Returns `(thumb_y, thumb_h)` in track-local coordinates (pixels from track top).
    pub fn thumb_rect(&self, view_h: i32, track_h: i32) -> (i32, i32) {
        if self.content_h <= 0 || view_h <= 0 || !self.needs_scrollbar(view_h) {
            return (0, track_h);
        }
        let thumb_h = ((track_h * view_h) / self.content_h).max(16).min(track_h);
        let travel = (track_h - thumb_h).max(1);
        let max_off = (self.content_h - view_h).max(1);
        let thumb_y = ((travel as i64 * self.offset as i64) / max_off as i64) as i32;
        (thumb_y.clamp(0, travel), thumb_h)
    }
}

// ── Window ────────────────────────────────────────────────────────────────────

pub struct Window {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub title: &'static str,
    /// Per-pixel content-area back-buffer (width × content_height u32 pixels).
    pub buf: Vec<u32>,
    /// Minimized windows are hidden from the desktop but stay in the taskbar.
    pub minimized: bool,
    /// Scroll state — set `scroll.content_h` from your app to enable the scrollbar.
    pub scroll: ScrollState,
    /// Saved geometry for restore after minimize / maximize.
    saved_x: i32,
    saved_y: i32,
    saved_width: i32,
    saved_height: i32,
}

impl Window {
    pub fn new(x: i32, y: i32, width: i32, height: i32, title: &'static str) -> Self {
        let content_h = (height - TITLE_H).max(0) as usize;
        let buf = alloc::vec![crate::framebuffer::DARK_GRAY; width as usize * content_h];
        Window {
            x,
            y,
            width,
            height,
            title,
            buf,
            minimized: false,
            scroll: ScrollState::new(),
            saved_x: x,
            saved_y: y,
            saved_width: width,
            saved_height: height,
        }
    }

    // ── Hit tests ─────────────────────────────────────────────────────────────

    pub fn hit(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }

    pub fn hit_title(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + TITLE_H
    }

    pub fn hit_close(&self, px: i32, py: i32) -> bool {
        px >= self.x + self.width - WIN_BTN_W
            && px < self.x + self.width
            && py >= self.y
            && py < self.y + TITLE_H
    }

    pub fn hit_minimize(&self, px: i32, py: i32) -> bool {
        px >= self.x + self.width - WIN_BTN_W * 3
            && px < self.x + self.width - WIN_BTN_W * 2
            && py >= self.y
            && py < self.y + TITLE_H
    }

    pub fn hit_maximize(&self, px: i32, py: i32) -> bool {
        px >= self.x + self.width - WIN_BTN_W * 2
            && px < self.x + self.width - WIN_BTN_W
            && py >= self.y
            && py < self.y + TITLE_H
    }

    /// True when `(px, py)` is over the bottom-right resize grip.
    pub fn hit_resize(&self, px: i32, py: i32) -> bool {
        px >= self.x + self.width - RESIZE_HANDLE
            && px < self.x + self.width
            && py >= self.y + self.height - RESIZE_HANDLE
            && py < self.y + self.height
    }

    /// True when `(px, py)` is inside the scrollbar strip (only when active).
    pub fn hit_scrollbar(&self, px: i32, py: i32) -> bool {
        let view_h = (self.height - TITLE_H).max(0);
        self.scroll.needs_scrollbar(view_h)
            && px >= self.x + self.width - SCROLLBAR_W
            && px < self.x + self.width
            && py >= self.y + TITLE_H
            && py < self.y + self.height
    }

    // ── Resize ────────────────────────────────────────────────────────────────

    /// Resize the window, clamping to sensible minimums, and reallocate the back-buffer.
    pub fn resize_to(&mut self, new_w: i32, new_h: i32) {
        const MIN_W: i32 = 120;
        const MIN_H: i32 = TITLE_H + 40;
        self.width = new_w.max(MIN_W);
        self.height = new_h.max(MIN_H);
        let content_h = (self.height - TITLE_H).max(0) as usize;
        self.buf = alloc::vec![crate::framebuffer::DARK_GRAY; self.width as usize * content_h];
        self.scroll.clamp(content_h as i32);
    }

    // ── Minimize / maximize / restore ─────────────────────────────────────────

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
            self.buf = alloc::vec![crate::framebuffer::DARK_GRAY; self.width as usize * content_h];
        }
    }

    pub fn maximize(&mut self, sw: i32, sh: i32) {
        if self.width == sw && self.height == sh {
            // toggle: restore saved geometry
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
        self.buf = alloc::vec![crate::framebuffer::DARK_GRAY; self.width as usize * content_h];
        self.scroll.clamp(content_h as i32);
    }
}
