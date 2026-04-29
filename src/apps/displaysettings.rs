extern crate alloc;

use alloc::string::String;
use font8x8::UnicodeFonts;

use crate::desktop_settings::{self, DesktopSettings, DesktopSortMode};
use crate::framebuffer::WHITE;
use crate::wm::window::{Window, TITLE_H};

pub const DISPLAY_SETTINGS_W: i32 = 440;
pub const DISPLAY_SETTINGS_H: i32 = 388;

const BG_A: u32 = 0x00_03_07_16;
const BG_B: u32 = 0x00_01_03_0B;
const PANEL: u32 = 0x00_00_0A_1E;
const PANEL_ALT: u32 = 0x00_00_0F_28;
const BORDER: u32 = 0x00_00_44_88;
const ACCENT: u32 = 0x00_00_BB_FF;
const ACCENT_DIM: u32 = 0x00_00_55_88;
const LABEL: u32 = 0x00_66_AA_DD;
const MUTED: u32 = 0x00_55_7A_92;
const GOOD: u32 = 0x00_00_FF_AA;
const TAB_X: usize = 14;
const TAB_Y: usize = 46;
const TAB_W: usize = 64;
const TAB_H: usize = 22;
const TAB_STEP: usize = 68;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsPage {
    Desktop,
    Accessibility,
    Diagnostics,
    Logs,
    Network,
    Storage,
}

const SETTINGS_PAGES: [(SettingsPage, &str); 6] = [
    (SettingsPage::Desktop, "Desktop"),
    (SettingsPage::Accessibility, "Access"),
    (SettingsPage::Diagnostics, "Diag"),
    (SettingsPage::Logs, "Logs"),
    (SettingsPage::Network, "Net"),
    (SettingsPage::Storage, "Storage"),
];

pub struct DisplaySettingsApp {
    pub window: Window,
    last_width: i32,
    last_height: i32,
    last_settings: DesktopSettings,
    page: SettingsPage,
    last_page: SettingsPage,
}

impl DisplaySettingsApp {
    pub fn new(x: i32, y: i32) -> Self {
        Self::with_page(x, y, "desktop")
    }

    pub fn with_page(x: i32, y: i32, page_name: &str) -> Self {
        let page = page_from_name(page_name);
        let mut app = DisplaySettingsApp {
            window: Window::new(
                x,
                y,
                DISPLAY_SETTINGS_W,
                DISPLAY_SETTINGS_H,
                "Display Settings",
            ),
            last_width: DISPLAY_SETTINGS_W,
            last_height: DISPLAY_SETTINGS_H,
            last_settings: desktop_settings::snapshot(),
            page,
            last_page: page,
        };
        app.render();
        app
    }

    pub fn handle_click(&mut self, lx: i32, ly: i32) {
        let settings = desktop_settings::snapshot();

        if let Some(page) = self.hit_page_tab(lx, ly) {
            self.page = page;
        } else if self.page == SettingsPage::Desktop && self.hit_toggle(lx, ly, 120) {
            desktop_settings::set_show_icons(!settings.show_icons);
        } else if self.page == SettingsPage::Desktop && self.hit_toggle(lx, ly, 152) {
            desktop_settings::set_compact_spacing(!settings.compact_spacing);
        } else if self.page == SettingsPage::Desktop && self.hit_toggle(lx, ly, 184) {
            let prefs = crate::app_lifecycle::start_menu_prefs();
            crate::app_lifecycle::set_start_menu_compact(!prefs.compact);
        } else if self.page == SettingsPage::Desktop {
            if let Some(mode) = self.hit_sort_button(lx, ly) {
                desktop_settings::set_sort_mode(mode);
            } else {
                return;
            }
        } else if self.page == SettingsPage::Accessibility && self.hit_toggle(lx, ly, 104) {
            let access = crate::accessibility::snapshot();
            crate::accessibility::set("keyboard_nav", !access.keyboard_nav);
        } else if self.page == SettingsPage::Accessibility && self.hit_toggle(lx, ly, 136) {
            let access = crate::accessibility::snapshot();
            crate::accessibility::set("focus_rings", !access.focus_rings);
        } else if self.page == SettingsPage::Accessibility && self.hit_toggle(lx, ly, 168) {
            let access = crate::accessibility::snapshot();
            crate::accessibility::set("large_text", !access.large_text);
        } else if self.page == SettingsPage::Accessibility && self.hit_toggle(lx, ly, 200) {
            let access = crate::accessibility::snapshot();
            crate::accessibility::set("reduced_motion", !access.reduced_motion);
        } else {
            return;
        }

        crate::wm::request_repaint();
        self.render();
    }

    pub fn update(&mut self) {
        let settings = desktop_settings::snapshot();
        if self.window.width != self.last_width
            || self.window.height != self.last_height
            || settings != self.last_settings
            || self.page != self.last_page
        {
            self.render();
        }
    }

    fn render(&mut self) {
        let settings = desktop_settings::snapshot();
        let access = crate::accessibility::snapshot();
        self.last_width = self.window.width;
        self.last_height = self.window.height;
        self.last_settings = settings;
        self.last_page = self.page;

        let stride = self.window.width.max(0) as usize;
        let content_h = (self.window.height - TITLE_H).max(0) as usize;
        self.fill_background(stride);
        self.window.scroll.content_h = 0;
        self.window.scroll.offset = 0;

        self.fill_rect(stride, 0, 0, stride, 36, PANEL_ALT);
        self.fill_rect(stride, 0, 35, stride, 1, BORDER);
        self.put_str(stride, 18, 12, "SETTINGS", LABEL);
        self.put_str(
            stride,
            18,
            24,
            "desktop, diagnostics, logs, network, storage, accessibility",
            MUTED,
        );
        self.draw_page_tabs(stride);

        match self.page {
            SettingsPage::Desktop => self.render_desktop_page(stride, content_h, settings),
            SettingsPage::Accessibility => self.render_accessibility_page(stride, access),
            SettingsPage::Diagnostics => self.render_diagnostics_page(stride),
            SettingsPage::Logs => self.render_lines_page(
                stride,
                "LOGS + PROFILER",
                crate::klog::lines()
                    .into_iter()
                    .chain(crate::profiler::lines().into_iter())
                    .take(12)
                    .collect(),
            ),
            SettingsPage::Network => self.render_lines_page(
                stride,
                "NETWORK",
                crate::net::status_lines()
                    .into_iter()
                    .chain(crate::net::protocol_lines().into_iter())
                    .collect(),
            ),
            SettingsPage::Storage => {
                let mut lines = crate::vfs::mount_lines();
                lines.extend(crate::writeback::lines());
                lines.extend(crate::fs_hardening::status_lines());
                self.render_lines_page(stride, "STORAGE", lines);
            }
        }
        self.window.mark_dirty_all();
    }

    fn render_desktop_page(&mut self, stride: usize, content_h: usize, settings: DesktopSettings) {
        let panel_w = (self.window.width.max(0) as usize).saturating_sub(32);
        self.draw_panel(stride, 16, 78, panel_w, 54);
        self.draw_panel(stride, 16, 140, panel_w, 84);
        self.draw_panel(stride, 16, 232, panel_w, content_h.saturating_sub(248));

        self.put_str(stride, 28, 92, "CURRENT MODE", LABEL);
        self.put_resolution_line(stride, 28, 108);
        self.put_str(stride, 250, 108, "live shell controls", GOOD);

        self.draw_toggle_row(
            stride,
            28,
            120,
            panel_w.saturating_sub(24),
            "Show desktop icons",
            settings.show_icons,
        );
        self.draw_toggle_row(
            stride,
            28,
            152,
            panel_w.saturating_sub(24),
            "Compact icon spacing",
            settings.compact_spacing,
        );
        self.draw_toggle_row(
            stride,
            28,
            184,
            panel_w.saturating_sub(24),
            "Compact Start menu layout",
            crate::app_lifecycle::start_menu_prefs().compact,
        );

        self.put_str(stride, 28, 244, "SORT ORDER", LABEL);
        self.put_str(stride, 28, 258, "controls desktop icon layout", MUTED);
        self.draw_sort_buttons(stride, 170, 240, settings.sort_mode);
    }

    fn render_accessibility_page(
        &mut self,
        stride: usize,
        access: crate::accessibility::AccessibilitySettings,
    ) {
        let panel_w = (self.window.width.max(0) as usize).saturating_sub(32);
        self.draw_panel(stride, 16, 82, panel_w, 156);
        self.put_str(stride, 28, 94, "ACCESSIBILITY", LABEL);
        self.draw_toggle_row(
            stride,
            28,
            104,
            panel_w.saturating_sub(24),
            "Keyboard-only navigation",
            access.keyboard_nav,
        );
        self.draw_toggle_row(
            stride,
            28,
            136,
            panel_w.saturating_sub(24),
            "Focus rings",
            access.focus_rings,
        );
        self.draw_toggle_row(
            stride,
            28,
            168,
            panel_w.saturating_sub(24),
            "Large text across shell/apps",
            access.large_text,
        );
        self.draw_toggle_row(
            stride,
            28,
            200,
            panel_w.saturating_sub(24),
            "Reduced motion / calmer UI",
            access.reduced_motion,
        );
    }

    fn render_diagnostics_page(&mut self, stride: usize) {
        let panel_w = (self.window.width.max(0) as usize).saturating_sub(32);
        let card_w = panel_w.saturating_sub(12) / 2;
        let stats = crate::wm::compositor::compositor_stats();
        let service_count = crate::services::lines().len();
        let config_count = crate::config_store::lines().len().saturating_sub(1);
        let crash_count = crate::crashdump::lines()
            .iter()
            .filter(|line| !line.contains("no crash"))
            .count();

        self.draw_panel(stride, 16, 82, card_w, 76);
        self.draw_panel(stride, 28 + card_w, 82, card_w, 76);
        self.draw_panel(stride, 16, 168, card_w, 86);
        self.draw_panel(stride, 28 + card_w, 168, card_w, 86);
        self.draw_panel(stride, 16, 264, panel_w, 96);

        self.put_str(stride, 28, 96, "HEALTH", LABEL);
        self.put_str(stride, 28, 112, "selftests active at boot", WHITE);
        let mut crash_line = String::from("crash reports ");
        push_number(&mut crash_line, crash_count);
        self.put_str(
            stride,
            28,
            128,
            &crash_line,
            if crash_count == 0 { GOOD } else { WHITE },
        );
        let mut service_line = String::from("services tracked ");
        push_number(&mut service_line, service_count);
        self.put_str(stride, 28, 142, &service_line, MUTED);

        let right_x = 40 + card_w;
        self.put_str(stride, right_x, 96, "COMPOSITOR", LABEL);
        let mut fps_line = String::from("fps ");
        push_number(&mut fps_line, stats.fps as usize);
        fps_line.push_str("  frames ");
        push_number(&mut fps_line, stats.frames as usize);
        self.put_str(stride, right_x, 112, &fps_line, WHITE);
        let mut damage_line = String::from("damage rows ");
        push_number(&mut damage_line, stats.damage_rows as usize);
        self.put_str(stride, right_x, 128, &damage_line, MUTED);
        let mut pixels_line = String::from("pixels ");
        push_number(&mut pixels_line, stats.damage_pixels as usize);
        self.put_str(stride, right_x, 142, &pixels_line, MUTED);

        self.put_str(stride, 28, 182, "CONFIG", LABEL);
        self.put_str(stride, 28, 198, "/CONFIG validation", WHITE);
        let mut config_line = String::from("known files ");
        push_number(&mut config_line, config_count);
        self.put_str(stride, 28, 214, &config_line, MUTED);
        self.put_str(stride, 28, 230, "recovery uses temp-write", GOOD);

        self.put_str(stride, right_x, 182, "PACKAGES", LABEL);
        let manifests = crate::app_metadata::installed_app_manifests();
        let mut manifest_line = String::from("app manifests ");
        push_number(&mut manifest_line, manifests.len());
        self.put_str(stride, right_x, 198, &manifest_line, WHITE);
        let validation = if crate::app_metadata::validate_installed_manifests().is_ok() {
            "manifest validation ok"
        } else {
            "manifest validation failed"
        };
        self.put_str(stride, right_x, 214, validation, GOOD);
        self.put_str(stride, right_x, 230, "/APPS/*/APP.CFG", MUTED);

        self.put_str(stride, 28, 278, "RECENT EVENTS", LABEL);
        let mut y = 294usize;
        for line in crate::event_bus::lines(4).iter() {
            self.put_str(stride, 28, y, line, WHITE);
            y += 14;
        }
    }

    fn render_lines_page(&mut self, stride: usize, title: &str, lines: alloc::vec::Vec<String>) {
        let panel_w = (self.window.width.max(0) as usize).saturating_sub(32);
        self.draw_panel(stride, 16, 82, panel_w, 276);
        self.put_str(stride, 28, 96, title, LABEL);
        let mut y = 116usize;
        for line in lines.iter().take(15) {
            self.put_str(stride, 28, y, line, WHITE);
            y += 14;
        }
    }

    fn draw_page_tabs(&mut self, stride: usize) {
        for (idx, (page, label)) in SETTINGS_PAGES.iter().enumerate() {
            let x = TAB_X + idx * TAB_STEP;
            let active = *page == self.page;
            self.fill_rect(
                stride,
                x,
                TAB_Y,
                TAB_W,
                TAB_H,
                if active { ACCENT } else { PANEL },
            );
            self.draw_rect_border(
                stride,
                x,
                TAB_Y,
                TAB_W,
                TAB_H,
                if active { WHITE } else { BORDER },
            );
            self.put_str(
                stride,
                x + 4,
                TAB_Y + 7,
                label,
                if active { 0x00_00_09_18 } else { LABEL },
            );
        }
    }

    fn hit_page_tab(&self, lx: i32, ly: i32) -> Option<SettingsPage> {
        let tab_y = TAB_Y as i32;
        let tab_h = TAB_H as i32;
        if !(tab_y..tab_y + tab_h).contains(&ly) {
            return None;
        }
        for (idx, (page, _)) in SETTINGS_PAGES.iter().enumerate() {
            let x = TAB_X as i32 + idx as i32 * TAB_STEP as i32;
            if lx >= x && lx < x + TAB_W as i32 {
                return Some(*page);
            }
        }
        None
    }

    fn put_resolution_line(&mut self, stride: usize, x: usize, y: usize) {
        let mut line = String::from("Resolution ");
        push_number(&mut line, crate::framebuffer::width());
        line.push('x');
        push_number(&mut line, crate::framebuffer::height());
        line.push_str("    Sort ");
        line.push_str(desktop_settings::snapshot().sort_mode.label());
        self.put_str(stride, x, y, &line, WHITE);
    }

    fn draw_toggle_row(
        &mut self,
        stride: usize,
        x: usize,
        y: usize,
        w: usize,
        label: &str,
        active: bool,
    ) {
        self.fill_rect(stride, x, y, w, 22, PANEL);
        self.draw_rect_border(stride, x, y, w, 22, BORDER);
        self.put_str(stride, x + 12, y + 7, label, WHITE);
        let pill_x = x + w.saturating_sub(62);
        let pill_bg = if active { ACCENT } else { ACCENT_DIM };
        self.fill_rect(stride, pill_x, y + 4, 46, 14, pill_bg);
        self.draw_rect_border(stride, pill_x, y + 4, 46, 14, WHITE);
        self.put_str(
            stride,
            pill_x + 11,
            y + 7,
            if active { "ON" } else { "OFF" },
            if active { 0x00_00_09_18 } else { WHITE },
        );
    }

    fn draw_sort_buttons(&mut self, stride: usize, x: usize, y: usize, current: DesktopSortMode) {
        let button_w = 72usize;
        for (idx, mode) in [
            DesktopSortMode::Default,
            DesktopSortMode::Name,
            DesktopSortMode::Type,
        ]
        .iter()
        .enumerate()
        {
            let bx = x + idx * (button_w + 10);
            let active = *mode == current;
            self.fill_rect(
                stride,
                bx,
                y,
                button_w,
                20,
                if active { ACCENT } else { PANEL },
            );
            self.draw_rect_border(
                stride,
                bx,
                y,
                button_w,
                20,
                if active { WHITE } else { BORDER },
            );
            self.put_str(
                stride,
                bx + (button_w.saturating_sub(mode.label().len() * 8)) / 2,
                y + 6,
                mode.label(),
                if active { 0x00_00_09_18 } else { WHITE },
            );
        }
    }

    fn hit_toggle(&self, lx: i32, ly: i32, y: i32) -> bool {
        let panel_w = self.window.width.max(0) - 32;
        lx >= 28 && lx < 28 + panel_w - 24 && ly >= y && ly < y + 22
    }

    fn hit_sort_button(&self, lx: i32, ly: i32) -> Option<DesktopSortMode> {
        if ly < 240 || ly >= 260 {
            return None;
        }
        let button_w = 72i32;
        let start_x = 170i32;
        for (idx, mode) in [
            DesktopSortMode::Default,
            DesktopSortMode::Name,
            DesktopSortMode::Type,
        ]
        .iter()
        .enumerate()
        {
            let bx = start_x + idx as i32 * (button_w + 10);
            if lx >= bx && lx < bx + button_w {
                return Some(*mode);
            }
        }
        None
    }

    fn draw_panel(&mut self, stride: usize, x: usize, y: usize, w: usize, h: usize) {
        if w == 0 || h == 0 {
            return;
        }
        self.fill_rect(stride, x, y, w, h, PANEL);
        self.draw_rect_border(stride, x, y, w, h, BORDER);
        if h > 2 && w > 2 {
            self.draw_rect_border(stride, x + 1, y + 1, w - 2, h - 2, 0x00_00_18_30);
        }
    }

    fn fill_background(&mut self, stride: usize) {
        for (idx, pixel) in self.window.buf.iter_mut().enumerate() {
            let py = idx / stride;
            *pixel = if py % 10 < 5 { BG_A } else { BG_B };
        }
    }

    fn fill_rect(&mut self, stride: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let content_h = (self.window.height - TITLE_H).max(0) as usize;
        let width = self.window.width.max(0) as usize;
        for row in y..(y + h).min(content_h) {
            let base = row * stride;
            for col in x..(x + w).min(width) {
                let idx = base + col;
                if idx < self.window.buf.len() {
                    self.window.buf[idx] = color;
                }
            }
        }
    }

    fn draw_rect_border(
        &mut self,
        stride: usize,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        color: u32,
    ) {
        if w == 0 || h == 0 {
            return;
        }
        self.fill_rect(stride, x, y, w, 1, color);
        self.fill_rect(stride, x, y + h - 1, w, 1, color);
        self.fill_rect(stride, x, y, 1, h, color);
        self.fill_rect(stride, x + w - 1, y, 1, h, color);
    }

    fn put_str(&mut self, stride: usize, x: usize, y: usize, s: &str, color: u32) {
        for (i, ch) in s.chars().enumerate() {
            if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
                for (gy, &byte) in glyph.iter().enumerate() {
                    for gx in 0..8 {
                        if (byte >> gx) & 1 == 1 {
                            let px = x + i * 8 + gx;
                            let py = y + gy;
                            let idx = py * stride + px;
                            if idx < self.window.buf.len() {
                                self.window.buf[idx] = color;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn page_from_name(name: &str) -> SettingsPage {
    match name.to_ascii_lowercase().as_str() {
        "access" | "accessibility" => SettingsPage::Accessibility,
        "diag" | "diagnostics" | "health" => SettingsPage::Diagnostics,
        "logs" | "log" | "profiler" => SettingsPage::Logs,
        "net" | "network" => SettingsPage::Network,
        "storage" | "disk" => SettingsPage::Storage,
        _ => SettingsPage::Desktop,
    }
}

fn push_number(out: &mut String, mut value: usize) {
    if value == 0 {
        out.push('0');
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
        out.push(digits[idx] as char);
    }
}
