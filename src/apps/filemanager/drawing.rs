use super::*;

impl FileManagerApp {
    pub(super) fn render(&mut self) {
        let layout = self.layout();
        self.last_width = self.window.width;
        self.last_height = self.window.height;
        self.view_h = (layout.height - COMMAND_H - PATHBAR_H - STATUS_H).max(0);

        self.fill_background();
        self.draw_path_bar(layout);
        self.draw_sidebar(layout);
        self.draw_main_shell(layout);
        if self.path == "/" {
            self.window.scroll.content_h = 0;
            self.window.scroll.offset = 0;
            self.draw_root_overview(layout);
        } else {
            self.draw_directory_view(layout);
        }
        self.draw_detail_panel(layout);
        self.draw_status_bar(layout);
        self.draw_context_menu();
        self.draw_modal();
        self.window.mark_dirty_all();
    }

    pub(super) fn draw_path_bar(&mut self, layout: Layout) {
        let back = self.back_button_rect();
        let fwd = self.forward_button_rect();
        let crumb = self.breadcrumb_rect();
        let search = self.search_rect(layout);

        self.fill_rect(0, COMMAND_H, layout.width, PATHBAR_H, FM_PANEL_ALT);
        self.fill_rect(
            0,
            COMMAND_H + PATHBAR_H - 1,
            layout.width,
            1,
            FM_BORDER_SOFT,
        );

        self.draw_back_button(back);
        self.draw_forward_button(fwd);
        let mut tabs = String::from("tab ");
        fmt_push_u(&mut tabs, (self.active_tab + 1) as u64);
        tabs.push('/');
        fmt_push_u(&mut tabs, self.tabs.len() as u64);
        tabs.push_str("  T new  W close");
        if self.split_view {
            tabs.push_str("  split");
        }
        self.put_str(74, (COMMAND_H + 3) as usize, &tabs, FM_TEXT_MUTED);
        self.fill_rect(crumb.x, crumb.y, crumb.w, crumb.h, FM_PANEL);
        self.draw_rect_border(crumb.x, crumb.y, crumb.w, crumb.h, FM_BORDER);
        self.draw_breadcrumbs(crumb);

        let search_bg = if self.search_active {
            FM_SELECTION
        } else {
            FM_SEARCH
        };
        let search_border = if self.search_active {
            FM_SELECTION_GLOW
        } else {
            FM_BORDER_SOFT
        };
        self.fill_rect(search.x, search.y, search.w, search.h, search_bg);
        self.draw_rect_border(search.x, search.y, search.w, search.h, search_border);
        let search_text = if self.search_active && !self.search_filter.is_empty() {
            self.search_filter.clone()
        } else if self.search_active {
            String::from("_")
        } else {
            String::from("search")
        };
        let search_color = if self.search_active {
            FM_TEXT
        } else {
            FM_TEXT_MUTED
        };
        let max_chars = ((search.w - 20).max(0) as usize) / CW;
        self.put_str(
            (search.x + 10) as usize,
            (search.y + 7) as usize,
            &Self::clip_text(&search_text, max_chars),
            search_color,
        );
    }

    pub(super) fn draw_forward_button(&mut self, rect: Rect) {
        let enabled = !self.forward_stack.is_empty();
        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL);
        self.draw_rect_border(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if enabled { FM_BORDER } else { FM_BORDER_SOFT },
        );
        self.fill_rect(
            rect.x + 1,
            rect.y + 1,
            rect.w - 2,
            3,
            if enabled {
                FM_SELECTION_GLOW
            } else {
                FM_BORDER_SOFT
            },
        );
        self.put_str(
            (rect.x + 8) as usize,
            (rect.y + 7) as usize,
            ">",
            if enabled { FM_TEXT } else { FM_TEXT_MUTED },
        );
    }

    pub(super) fn draw_context_menu(&mut self) {
        if self.modal.is_some() {
            return;
        }
        let Some(menu) = self.context_menu.as_ref() else {
            return;
        };
        let items = self.context_menu_items(menu.target);
        let rect = self.context_menu_rect(menu);
        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_BORDER);
        self.fill_rect(rect.x, rect.y, rect.w, 3, FM_SELECTION_GLOW);
        for (idx, (label, _)) in items.iter().enumerate() {
            let row_y = rect.y + 3 + idx as i32 * MENU_ROW_H;
            self.fill_rect(rect.x + 1, row_y, rect.w - 2, MENU_ROW_H, FM_PANEL);
            self.put_str((rect.x + 10) as usize, (row_y + 6) as usize, label, FM_TEXT);
            if idx + 1 < items.len() {
                self.fill_rect(
                    rect.x + 8,
                    row_y + MENU_ROW_H - 1,
                    rect.w - 16,
                    1,
                    FM_BORDER_SOFT,
                );
            }
        }
    }

    pub(super) fn draw_sidebar(&mut self, layout: Layout) {
        self.fill_rect(
            0,
            COMMAND_H + PATHBAR_H,
            layout.sidebar_w,
            layout.status_y - COMMAND_H - PATHBAR_H,
            FM_PANEL_SOFT,
        );
        self.fill_rect(
            layout.sidebar_w,
            COMMAND_H + PATHBAR_H,
            1,
            layout.status_y - COMMAND_H - PATHBAR_H,
            FM_BORDER_SOFT,
        );

        let mut y = COMMAND_H + PATHBAR_H + 14;
        for item in self.sidebar_items() {
            match item.kind {
                SidebarItemKind::Section => {
                    self.put_str(18, y as usize, &item.label, FM_TEXT_MUTED);
                    y += 16;
                }
                SidebarItemKind::Link => {
                    self.draw_sidebar_item(
                        Rect {
                            x: 10,
                            y,
                            w: layout.sidebar_w - 20,
                            h: NAV_ROW_H,
                        },
                        &item,
                    );
                    y += NAV_ROW_H + 4;
                }
            }
        }
    }

    pub(super) fn sidebar_items(&self) -> Vec<SidebarItem> {
        let mut items = Vec::new();
        let root_names = Self::root_directory_names();
        items.push(SidebarItem {
            label: String::from("This PC"),
            path: Some(String::from("/")),
            active: self.path == "/",
            kind: SidebarItemKind::Link,
            indent: 0,
            icon: SidebarIcon::Computer,
        });
        for label in QUICK_ACCESS_FOLDERS {
            let path = Self::shell_link_path_with_roots(label, &root_names);
            items.push(SidebarItem {
                label: String::from(label),
                active: self.path_matches_or_contains(&path),
                path: Some(path),
                kind: SidebarItemKind::Link,
                indent: 0,
                icon: SidebarIcon::Folder,
            });
        }

        let path_links = self.current_path_links();
        if !path_links.is_empty() {
            items.push(SidebarItem {
                label: String::from("CURRENT PATH"),
                path: None,
                active: false,
                kind: SidebarItemKind::Section,
                indent: 0,
                icon: SidebarIcon::Folder,
            });
            for (depth, label, path) in path_links {
                items.push(SidebarItem {
                    label,
                    active: self.path.eq_ignore_ascii_case(&path),
                    path: Some(path),
                    kind: SidebarItemKind::Link,
                    indent: 10 + depth as i32 * 12,
                    icon: SidebarIcon::Folder,
                });
            }
        }

        items
    }

    pub(super) fn draw_sidebar_item(&mut self, rect: Rect, item: &SidebarItem) {
        if item.active {
            self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_SELECTION);
            self.fill_rect(rect.x, rect.y, 3, rect.h, FM_SELECTION_GLOW);
        }
        let icon_x = rect.x + 8 + item.indent;
        let icon_y = rect.y + 4;
        self.draw_sidebar_icon(icon_x, icon_y, item.icon, item.active);
        self.put_str(
            (icon_x + 16) as usize,
            (rect.y + 5) as usize,
            &item.label,
            if item.active { FM_TEXT } else { FM_TEXT_DIM },
        );
    }

    pub(super) fn draw_sidebar_icon(&mut self, x: i32, y: i32, icon: SidebarIcon, active: bool) {
        match icon {
            SidebarIcon::Computer => {
                self.fill_rect(x, y + 4, 10, 6, if active { FM_TEXT } else { FM_DRIVE });
                self.fill_rect(
                    x + 1,
                    y + 2,
                    8,
                    3,
                    if active {
                        FM_SELECTION_GLOW
                    } else {
                        FM_ACCENT_SOFT
                    },
                );
                self.fill_rect(x + 3, y + 11, 4, 1, FM_TEXT_MUTED);
            }
            SidebarIcon::Folder => {
                self.fill_rect(x + 1, y, 6, 2, if active { FM_TEXT } else { FM_FOLDER });
                self.fill_rect(x, y + 2, 10, 6, if active { FM_TEXT } else { FM_FOLDER });
                self.fill_rect(
                    x + 1,
                    y + 4,
                    8,
                    3,
                    if active {
                        FM_SELECTION_GLOW
                    } else {
                        FM_FOLDER_SHADE
                    },
                );
            }
        }
    }

    pub(super) fn draw_back_button(&mut self, rect: Rect) {
        let enabled = !self.back_stack.is_empty();
        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL);
        self.draw_rect_border(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if enabled { FM_BORDER } else { FM_BORDER_SOFT },
        );
        self.fill_rect(
            rect.x + 1,
            rect.y + 1,
            rect.w - 2,
            3,
            if enabled {
                FM_SELECTION_GLOW
            } else {
                FM_BORDER_SOFT
            },
        );
        self.put_str(
            (rect.x + 8) as usize,
            (rect.y + 7) as usize,
            "<",
            if enabled { FM_TEXT } else { FM_TEXT_MUTED },
        );
    }

    pub(super) fn draw_main_shell(&mut self, layout: Layout) {
        self.fill_rect(
            layout.main_x,
            COMMAND_H + PATHBAR_H,
            layout.main_w,
            layout.status_y - COMMAND_H - PATHBAR_H,
            FM_BG_BOT,
        );
        if let Some(detail) = self.detail_rect(layout) {
            self.fill_rect(detail.x - 7, detail.y, 1, detail.h, FM_BORDER_SOFT);
        }
    }

    pub(super) fn draw_summary_cards(
        &mut self,
        layout: Layout,
        y: i32,
        folder_count: usize,
        file_count: usize,
    ) {
        let row_x = self.content_left(layout);
        let available_w = self.content_width(layout);
        let cols = self.summary_card_cols(layout).max(1);
        let card_w = ((available_w - SUMMARY_CARD_GAP * (cols - 1)) / cols).max(72);

        let mut folder_value = String::new();
        fmt_push_u(&mut folder_value, folder_count as u64);
        let mut file_value = String::new();
        fmt_push_u(&mut file_value, file_count as u64);
        let mut sort_value = String::new();
        sort_value.push_str(self.sort_column.label());
        sort_value.push(' ');
        sort_value.push(if self.sort_desc { 'v' } else { '^' });
        let cards = [
            ("Folders", folder_value, FM_FOLDER),
            ("Files", file_value, FM_FILE),
            ("Sort", sort_value, FM_ACCENT),
        ];
        for (idx, (label, value, accent)) in cards.iter().enumerate() {
            let col = idx as i32 % cols;
            let row = idx as i32 / cols;
            self.draw_summary_card(
                Rect {
                    x: row_x + col * (card_w + SUMMARY_CARD_GAP),
                    y: y + row * (SUMMARY_CARD_H + SUMMARY_CARD_GAP),
                    w: card_w,
                    h: SUMMARY_CARD_H,
                },
                label,
                value,
                *accent,
            );
        }
    }

    pub(super) fn draw_summary_card(&mut self, rect: Rect, label: &str, value: &str, accent: u32) {
        let max_chars = ((rect.w - 20).max(0) as usize) / CW;
        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_BORDER);
        self.fill_rect(rect.x + 1, rect.y + 1, rect.w - 2, 3, accent);
        self.put_str(
            (rect.x + 10) as usize,
            (rect.y + 10) as usize,
            &Self::clip_text(label, max_chars),
            FM_TEXT_MUTED,
        );
        self.put_str(
            (rect.x + 10) as usize,
            (rect.y + 22) as usize,
            &Self::clip_text(value, max_chars),
            FM_TEXT,
        );
    }

    pub(super) fn draw_detail_panel(&mut self, layout: Layout) {
        let detail = match self.detail_rect(layout) {
            Some(detail) => detail,
            None => return,
        };
        self.fill_rect(detail.x, detail.y, detail.w, detail.h, FM_PANEL);
        self.draw_rect_border(detail.x, detail.y, detail.w, detail.h, FM_BORDER);
        self.fill_rect(detail.x, detail.y, detail.w, 3, FM_SELECTION_GLOW);
        self.put_str(
            (detail.x + 12) as usize,
            (detail.y + 10) as usize,
            "DETAILS",
            FM_TEXT_MUTED,
        );

        if let Some(idx) = self.focused.or_else(|| self.selected.first().copied()) {
            if let Some((name, is_dir, size)) = self
                .entries
                .get(idx)
                .map(|entry| (entry.name.clone(), entry.is_dir, entry.size))
            {
                let full_path = self.make_abs(idx);
                let detail_type = self
                    .entries
                    .get(idx)
                    .map(|entry| Self::type_label_for_kind(entry, self.entry_kind(idx)))
                    .unwrap_or("File");
                let size_text = if let Some(app) = self.desktop_app_for_idx(idx) {
                    let mut s = String::from("Launch ");
                    s.push_str(app);
                    s
                } else if is_dir {
                    String::from("Open container")
                } else {
                    Self::format_size(size)
                };

                self.draw_large_entry_icon(detail.x + 14, detail.y + 30, is_dir);
                self.put_str(
                    (detail.x + 56) as usize,
                    (detail.y + 34) as usize,
                    &Self::clip_text(&name, 14),
                    FM_TEXT,
                );
                self.put_str(
                    (detail.x + 56) as usize,
                    (detail.y + 48) as usize,
                    detail_type,
                    FM_TEXT_MUTED,
                );

                self.draw_detail_row(detail.x + 12, detail.y + 76, "Location", &full_path);
                self.draw_detail_row(detail.x + 12, detail.y + 108, "Size", &size_text);
                self.draw_detail_row(
                    detail.x + 12,
                    detail.y + 140,
                    "Action",
                    if self.desktop_app_for_idx(idx).is_some() {
                        "Launch application"
                    } else if is_dir {
                        "Open folder"
                    } else {
                        "Open or edit file"
                    },
                );

                let note = if self.desktop_app_for_idx(idx).is_some() {
                    "Desktop launchers mirror the shell icons."
                } else if is_dir {
                    "Folders stay in the shell view."
                } else {
                    "Files open with the default app."
                };
                self.put_str(
                    (detail.x + 12) as usize,
                    (detail.y + detail.h - 26) as usize,
                    &Self::clip_text(note, ((detail.w - 24).max(0) as usize) / CW),
                    FM_TEXT_MUTED,
                );
            }
        } else {
            let folders = self.folder_indices().len();
            let files = self.file_indices().len();
            let mut folder_text = String::new();
            fmt_push_u(&mut folder_text, folders as u64);
            folder_text.push_str(" folders");
            let mut file_text = String::new();
            fmt_push_u(&mut file_text, files as u64);
            file_text.push_str(" files");
            self.draw_large_entry_icon(detail.x + 14, detail.y + 30, true);
            self.put_str(
                (detail.x + 56) as usize,
                (detail.y + 34) as usize,
                if self.path == "/" {
                    "This PC"
                } else {
                    "Folder"
                },
                FM_TEXT,
            );
            self.put_str(
                (detail.x + 56) as usize,
                (detail.y + 48) as usize,
                &Self::clip_text(&self.path, 14),
                FM_TEXT_MUTED,
            );
            self.draw_detail_row(detail.x + 12, detail.y + 82, "Folders", &folder_text);
            self.draw_detail_row(detail.x + 12, detail.y + 114, "Files", &file_text);
            self.draw_detail_row(
                detail.x + 12,
                detail.y + 146,
                "Sort",
                self.sort_column.label(),
            );
        }
    }

    pub(super) fn draw_detail_row(&mut self, x: i32, y: i32, label: &str, value: &str) {
        self.put_str(x as usize, y as usize, label, FM_TEXT_MUTED);
        self.put_str(
            x as usize,
            (y + 13) as usize,
            &Self::clip_text(value, 18),
            FM_TEXT_DIM,
        );
        self.fill_rect(x, y + 24, 150, 1, FM_BORDER_SOFT);
    }

    pub(super) fn draw_large_entry_icon(&mut self, x: i32, y: i32, is_dir: bool) {
        if is_dir {
            self.fill_rect(x + 4, y, 14, 6, FM_FOLDER);
            self.fill_rect(x, y + 6, 26, 18, FM_FOLDER);
            self.fill_rect(x + 2, y + 11, 22, 9, FM_FOLDER_SHADE);
        } else {
            self.fill_rect(x + 2, y, 18, 24, FM_FILE);
            self.draw_rect_border(x + 2, y, 18, 24, blend(FM_FILE, BLACK, 120));
            self.fill_rect(x + 12, y, 8, 6, WHITE);
            self.fill_rect(x + 5, y + 10, 10, 2, blend(FM_FILE, WHITE, 110));
            self.fill_rect(x + 5, y + 14, 10, 2, blend(FM_FILE, WHITE, 110));
        }
    }

    pub(super) fn draw_file_header(&mut self, layout: Layout, y: i32) {
        let columns = self.file_columns(layout);
        self.fill_rect(columns.row_x, y, columns.row_w, FILE_HEADER_H, FM_PANEL);
        self.draw_rect_border(
            columns.row_x,
            y,
            columns.row_w,
            FILE_HEADER_H,
            FM_BORDER_SOFT,
        );
        self.draw_sort_header_label(columns.name_x, y + 6, "Name", SortColumn::Name);
        self.draw_sort_header_label(columns.type_x, y + 6, "Type", SortColumn::Type);
        self.draw_sort_header_label(columns.size_x, y + 6, "Size", SortColumn::Size);
    }

    pub(super) fn draw_sort_header_label(
        &mut self,
        x: i32,
        y: i32,
        label: &str,
        column: SortColumn,
    ) {
        let mut text = String::from(label);
        if self.sort_column == column {
            text.push(' ');
            text.push(if self.sort_desc { 'v' } else { '^' });
        }
        self.put_str(
            x as usize,
            y as usize,
            &text,
            if self.sort_column == column {
                FM_TEXT
            } else {
                FM_TEXT_MUTED
            },
        );
    }

    pub(super) fn draw_root_overview(&mut self, layout: Layout) {
        let top = self.title_y();
        let content_left = self.content_left(layout);
        self.put_str(content_left as usize, top as usize, "This PC", FM_TEXT);
        self.put_str(
            content_left as usize,
            (top + 14) as usize,
            "coolOS shell view",
            FM_TEXT_MUTED,
        );

        let folders = self.folder_indices();
        let files = self.file_indices();
        self.draw_summary_cards(layout, self.summary_cards_y(), folders.len(), files.len());
        let section_y = self.section_start_y();
        let tiles_h = self.draw_folder_section(layout, section_y, &folders, true);
        let drives_y = section_y + tiles_h + 20;
        self.draw_drive_section(
            layout,
            drives_y,
            if files.is_empty() { &folders } else { &files },
        );
    }

    pub(super) fn draw_directory_view(&mut self, layout: Layout) {
        let top = self.title_y();
        let content_left = self.content_left(layout);
        let title_chars = (self.content_width(layout).max(0) as usize) / CW;
        let title = if self.path == "/" {
            "This PC"
        } else {
            self.path.as_str()
        };
        self.put_str(
            content_left as usize,
            top as usize,
            &Self::clip_text(title, title_chars.max(8)),
            FM_TEXT,
        );
        self.put_str(
            content_left as usize,
            (top + 14) as usize,
            "folders first, files below",
            FM_TEXT_MUTED,
        );

        let folders = self.folder_indices();
        let files = self.file_indices();
        self.draw_summary_cards(layout, self.summary_cards_y(), folders.len(), files.len());
        let section_y = self.section_start_y();
        let folders_h = self.draw_folder_section(layout, section_y, &folders, false);
        let files_y = section_y + folders_h + 18;
        self.draw_file_list_section(layout, files_y, &files);
    }

    pub(super) fn draw_folder_section(
        &mut self,
        layout: Layout,
        y: i32,
        indices: &[usize],
        root_mode: bool,
    ) -> i32 {
        let title = if root_mode { "Folders" } else { "Subfolders" };
        let count = indices.len();
        let content_left = self.content_left(layout);
        let content_w = self.content_width(layout);
        let mut label = String::from(title);
        label.push(' ');
        label.push('(');
        fmt_push_u(&mut label, count as u64);
        label.push(')');

        self.put_str(content_left as usize, y as usize, &label, FM_TEXT_DIM);
        self.fill_rect(
            content_left,
            y + SECTION_HDR_H - 2,
            content_w,
            1,
            FM_BORDER_SOFT,
        );

        if indices.is_empty() {
            self.put_str(
                (content_left + 10) as usize,
                (y + 24) as usize,
                "(no folders)",
                FM_TEXT_MUTED,
            );
            return 40;
        }

        let tile_y = y + 22;
        let tile_w = ((content_w - 24 - TILE_GAP_X * 2) / 3).max(140);
        let cols = (content_w / (tile_w + TILE_GAP_X)).max(1) as usize;

        for (visual_idx, &entry_idx) in indices.iter().enumerate() {
            let col = (visual_idx % cols).min(2);
            let row = visual_idx / cols;
            let rect = Rect {
                x: content_left + col as i32 * (tile_w + TILE_GAP_X),
                y: tile_y + row as i32 * (TILE_H + TILE_GAP_Y),
                w: tile_w,
                h: TILE_H,
            };
            self.draw_folder_tile(rect, entry_idx);
        }

        let rows = ((indices.len() + cols - 1) / cols).max(1) as i32;
        22 + rows * TILE_H + (rows - 1) * TILE_GAP_Y
    }

    pub(super) fn draw_drive_section(&mut self, layout: Layout, y: i32, indices: &[usize]) {
        let content_left = self.content_left(layout);
        let content_w = self.content_width(layout);
        self.put_str(
            content_left as usize,
            y as usize,
            "Devices and drives",
            FM_TEXT_DIM,
        );
        self.fill_rect(
            content_left,
            y + SECTION_HDR_H - 2,
            content_w,
            1,
            FM_BORDER_SOFT,
        );

        if indices.is_empty() {
            self.put_str(
                (content_left + 10) as usize,
                (y + 24) as usize,
                "(no items)",
                FM_TEXT_MUTED,
            );
            return;
        }

        let card_w = ((content_w - 12) / 2).max(180);
        let cols = if content_w > 420 { 2 } else { 1 };
        let max_items = if cols == 2 { 4 } else { 3 };
        for (visual_idx, &entry_idx) in indices.iter().take(max_items).enumerate() {
            let col = (visual_idx % cols as usize) as i32;
            let row = (visual_idx / cols as usize) as i32;
            let rect = Rect {
                x: content_left + col * (card_w + 12),
                y: y + 22 + row * (DRIVE_H + DRIVE_GAP_Y),
                w: card_w,
                h: DRIVE_H,
            };
            self.draw_drive_card(rect, entry_idx);
        }

        if indices.len() > max_items {
            let mut more = String::from("+");
            fmt_push_u(&mut more, (indices.len() - max_items) as u64);
            more.push_str(" more");
            self.put_str(
                (content_left + content_w - (more.len() as i32 * CW as i32) - 4).max(content_left)
                    as usize,
                y as usize,
                &more,
                FM_TEXT_MUTED,
            );
        }
    }

    pub(super) fn draw_file_list_section(
        &mut self,
        layout: Layout,
        y: i32,
        file_indices: &[usize],
    ) {
        let content_left = self.content_left(layout);
        let content_w = self.content_width(layout);
        self.put_str(content_left as usize, y as usize, "Files", FM_TEXT_DIM);
        self.fill_rect(
            content_left,
            y + SECTION_HDR_H - 2,
            content_w,
            1,
            FM_BORDER_SOFT,
        );

        let header_y = y + 22;
        self.draw_file_header(layout, header_y);

        let list_y = header_y + FILE_HEADER_H;
        let list_h = (layout.status_y - list_y - 10).max(0);
        self.view_h = list_h;
        self.total_rows = (list_h / LIST_ROW_H).max(0) as usize;
        self.window.scroll.content_h = file_indices.len() as i32 * LIST_ROW_H;
        self.window.scroll.offset = self.offset as i32 * LIST_ROW_H;
        self.window.scroll.clamp(list_h);

        if file_indices.is_empty() {
            self.put_str(
                (content_left + 10) as usize,
                (list_y + 8) as usize,
                "(no files)",
                FM_TEXT_MUTED,
            );
            return;
        }

        let visible = self.total_rows.max(1);
        let max_offset = file_indices.len().saturating_sub(visible);
        self.offset = self.offset.min(max_offset);
        let columns = self.file_columns(layout);
        let name_w = (columns.name_w.max(0) as usize) / CW;

        for visual_row in 0..visible {
            let idx_in_files = self.offset + visual_row;
            if idx_in_files >= file_indices.len() {
                break;
            }
            let entry_idx = file_indices[idx_in_files];
            let (full_name, size) = match self.entries.get(entry_idx) {
                Some(entry) => (entry.name.clone(), entry.size),
                None => continue,
            };
            let row_y = list_y + visual_row as i32 * LIST_ROW_H;
            let selected = self.selected.contains(&entry_idx);
            let focused = self.focused == Some(entry_idx);
            self.fill_rect(
                columns.row_x,
                row_y,
                columns.row_w,
                LIST_ROW_H,
                if selected {
                    FM_SELECTION
                } else if visual_row % 2 == 0 {
                    FM_PANEL_SOFT
                } else {
                    FM_BG_BOT
                },
            );
            if selected {
                self.fill_rect(columns.row_x, row_y, 3, LIST_ROW_H, FM_SELECTION_GLOW);
            }
            if focused {
                self.draw_rect_border(
                    columns.row_x + 2,
                    row_y + 2,
                    columns.row_w - 4,
                    LIST_ROW_H - 4,
                    FM_TEXT,
                );
            }
            self.draw_file_icon((columns.row_x + 10) as usize, (row_y + 4) as usize);

            let name = Self::clip_text(&full_name, name_w);
            self.put_str(
                columns.name_x as usize,
                (row_y + 5) as usize,
                &name,
                if selected { FM_TEXT } else { FM_TEXT_DIM },
            );
            self.put_str(
                columns.type_x as usize,
                (row_y + 5) as usize,
                self.entries
                    .get(entry_idx)
                    .map(|entry| Self::type_label_for_kind(entry, self.entry_kind(entry_idx)))
                    .unwrap_or("File"),
                FM_TEXT_MUTED,
            );
            let size = Self::format_size(size);
            self.put_str(
                columns.size_x as usize,
                (row_y + 5) as usize,
                &size,
                if selected { FM_TEXT } else { FM_TEXT_DIM },
            );
            self.fill_rect(
                columns.row_x,
                row_y + LIST_ROW_H - 1,
                columns.row_w,
                1,
                FM_BORDER_SOFT,
            );
        }
    }

    pub(super) fn draw_status_bar(&mut self, layout: Layout) {
        self.fill_rect(0, layout.status_y, layout.width, STATUS_H, FM_SHELL);
        self.fill_rect(0, layout.status_y, layout.width, 1, FM_BORDER_SOFT);

        let folders = self.entries.iter().filter(|e| e.is_dir).count();
        let files = self.entries.len().saturating_sub(folders);
        let mut left = String::new();
        fmt_push_u(&mut left, folders as u64);
        left.push_str(" folders  ");
        fmt_push_u(&mut left, files as u64);
        left.push_str(" files");
        self.put_str(10, (layout.status_y + 6) as usize, &left, FM_TEXT_MUTED);

        if let Some(line) = self.active_file_operation_line() {
            let progress = self.active_file_operation_progress().unwrap_or(0);
            let line_chars = ((layout.width - 330).max(80) as usize) / CW;
            self.put_str(
                134,
                (layout.status_y + 6) as usize,
                &Self::clip_text(&line, line_chars),
                FM_TEXT,
            );

            let bar_x = (layout.width / 2).max(260);
            let bar_w = (layout.width - bar_x - 190).clamp(80, 220);
            self.fill_rect(bar_x, layout.status_y + 7, bar_w, 6, FM_PANEL);
            self.draw_rect_border(bar_x, layout.status_y + 7, bar_w, 6, FM_BORDER_SOFT);
            let fill_w = ((bar_w - 2).max(0) as usize * progress as usize / 100) as i32;
            self.fill_rect(bar_x + 1, layout.status_y + 8, fill_w, 4, FM_ACCENT);

            let toggle = self.file_op_toggle_rect(layout);
            let cancel = self.file_op_cancel_rect(layout);
            self.fill_rect(toggle.x, toggle.y, toggle.w, toggle.h, FM_PANEL_ALT);
            self.draw_rect_border(toggle.x, toggle.y, toggle.w, toggle.h, FM_BORDER);
            self.put_str(
                (toggle.x + 8) as usize,
                (toggle.y + 3) as usize,
                if self.active_file_operation_paused() {
                    "Resume"
                } else {
                    "Pause"
                },
                FM_TEXT,
            );
            self.fill_rect(cancel.x, cancel.y, cancel.w, cancel.h, FM_PANEL_ALT);
            self.draw_rect_border(cancel.x, cancel.y, cancel.w, cancel.h, FM_BORDER);
            self.put_str(
                (cancel.x + 8) as usize,
                (cancel.y + 3) as usize,
                "Cancel",
                FM_TEXT,
            );
            return;
        }

        let hint = self.status_note.clone().unwrap_or_else(|| {
            if self.search_active {
                let mut s = String::from("search: ");
                s.push_str(&self.search_filter);
                s.push_str("  (recursive, Esc clears)");
                s
            } else if self.selected.len() > 1 {
                let mut s = String::new();
                fmt_push_u(&mut s, self.selected.len() as u64);
                s.push_str(" selected  Space toggle  C/X/V clipboard");
                s
            } else if self.split_view {
                String::from("split view active  S toggles  recursive search in path box")
            } else if let Some(clipboard) = self.clipboard.as_ref() {
                let mut s = String::new();
                fmt_push_u(&mut s, clipboard.entries.len() as u64);
                s.push_str(if clipboard.cut { " cut" } else { " copied" });
                s.push_str("  V paste");
                s
            } else {
                String::new()
            }
        });
        let hint_x = ((layout.width as usize).saturating_sub(hint.len() * CW)) / 2;
        self.put_str(hint_x, (layout.status_y + 6) as usize, &hint, FM_TEXT_MUTED);

        if let Some(idx) = self.focused.or_else(|| self.selected.first().copied()) {
            if let Some(entry) = self.entries.get(idx) {
                let entry_name = entry.name.clone();
                let entry_is_dir = entry.is_dir;
                let entry_size = entry.size;
                let mut right = String::from(&entry_name);
                right.push_str("  ");
                right.push_str(Self::type_label_for_kind(entry, self.entry_kind(idx)));
                if !entry_is_dir && self.desktop_app_for_idx(idx).is_none() {
                    right.push_str("  ");
                    right.push_str(&Self::format_size(entry_size));
                }
                let right_x = (layout.width as usize)
                    .saturating_sub(right.len() * CW)
                    .saturating_sub(10);
                self.put_str(right_x, (layout.status_y + 6) as usize, &right, FM_TEXT_DIM);
            }
        }
    }

    pub(super) fn fill_background(&mut self) {
        let stride = self.window.width.max(0) as usize;
        let height = (self.window.height - TITLE_H).max(0) as usize;
        for row in 0..height {
            let t = (row as u32).saturating_mul(255) / height.max(1) as u32;
            let row_color = blend(FM_BG_TOP, FM_BG_BOT, t);
            let base = row * stride;
            for col in 0..stride {
                let idx = base + col;
                if idx < self.window.buf.len() {
                    self.window.buf[idx] = row_color;
                }
            }
        }
    }

    pub(super) fn draw_breadcrumbs(&mut self, rect: Rect) {
        let segments = self.breadcrumb_segment_rects(rect);
        for (idx, (seg, label, _path)) in segments.iter().enumerate() {
            let active = idx + 1 == segments.len();
            self.fill_rect(
                seg.x,
                seg.y,
                seg.w,
                seg.h,
                if active { FM_SELECTION } else { FM_PANEL_ALT },
            );
            self.draw_rect_border(
                seg.x,
                seg.y,
                seg.w,
                seg.h,
                if active {
                    FM_SELECTION_GLOW
                } else {
                    FM_BORDER_SOFT
                },
            );
            self.put_str(
                (seg.x + BREAD_SEG_PAD) as usize,
                (seg.y + 4) as usize,
                &Self::clip_text(label, ((seg.w - BREAD_SEG_PAD * 2).max(0) as usize) / CW),
                if active { FM_TEXT } else { FM_TEXT_DIM },
            );
            if idx + 1 < segments.len() {
                self.put_str(
                    (seg.x + seg.w + 2) as usize,
                    (seg.y + 4) as usize,
                    ">",
                    FM_TEXT_MUTED,
                );
            }
        }
    }

    pub(super) fn draw_folder_tile(&mut self, rect: Rect, entry_idx: usize) {
        let selected = self.selected.contains(&entry_idx);
        let focused = self.focused == Some(entry_idx);
        self.fill_rect(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if selected { FM_SELECTION } else { FM_PANEL },
        );
        self.draw_rect_border(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if selected {
                FM_SELECTION_GLOW
            } else {
                FM_BORDER
            },
        );
        if focused {
            self.draw_rect_border(rect.x + 2, rect.y + 2, rect.w - 4, rect.h - 4, FM_TEXT);
        }
        self.fill_rect(rect.x + 10, rect.y + 12, 18, 12, FM_FOLDER);
        self.fill_rect(rect.x + 8, rect.y + 16, 28, 18, FM_FOLDER);
        self.fill_rect(rect.x + 10, rect.y + 20, 24, 10, FM_FOLDER_SHADE);

        if let Some(entry) = self.entries.get(entry_idx) {
            let entry_name = entry.name.clone();
            let count = self.entry_child_counts.get(entry_idx).copied().unwrap_or(0);
            self.put_str(
                (rect.x + 46) as usize,
                (rect.y + 14) as usize,
                &Self::clip_text(&entry_name, ((rect.w - 56).max(8) as usize) / CW),
                if selected { FM_TEXT } else { FM_TEXT_DIM },
            );
            let mut count_label = String::new();
            fmt_push_u(&mut count_label, count as u64);
            count_label.push_str(if count == 1 { " item" } else { " items" });
            self.put_str(
                (rect.x + 46) as usize,
                (rect.y + 28) as usize,
                &count_label,
                FM_TEXT_MUTED,
            );
        }
    }

    pub(super) fn draw_drive_card(&mut self, rect: Rect, entry_idx: usize) {
        let selected = self.selected.contains(&entry_idx);
        let focused = self.focused == Some(entry_idx);
        self.fill_rect(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if selected { FM_SELECTION } else { FM_PANEL },
        );
        self.draw_rect_border(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if selected {
                FM_SELECTION_GLOW
            } else {
                FM_BORDER
            },
        );
        if focused {
            self.draw_rect_border(rect.x + 2, rect.y + 2, rect.w - 4, rect.h - 4, FM_TEXT);
        }

        self.draw_drive_icon((rect.x + 10) as usize, (rect.y + 12) as usize);
        if let Some(entry) = self.entries.get(entry_idx) {
            let entry_name = entry.name.clone();
            let entry_is_dir = entry.is_dir;
            let entry_size = entry.size;
            let label = Self::clip_text(&entry_name, ((rect.w - 62).max(8) as usize) / CW);
            self.put_str(
                (rect.x + 46) as usize,
                (rect.y + 10) as usize,
                &label,
                if selected { FM_TEXT } else { FM_TEXT_DIM },
            );
            let detail = if entry_is_dir {
                "Folder root"
            } else {
                self.entries
                    .get(entry_idx)
                    .map(|entry| Self::type_label_for_kind(entry, self.entry_kind(entry_idx)))
                    .unwrap_or("File")
            };
            self.put_str(
                (rect.x + 46) as usize,
                (rect.y + 22) as usize,
                detail,
                FM_TEXT_MUTED,
            );

            let usage = Self::usage_ratio(&DirEntryInfo {
                name: entry_name.clone(),
                is_dir: entry_is_dir,
                size: entry_size,
            });
            self.fill_rect(rect.x + 46, rect.y + 35, rect.w - 58, 8, FM_SEARCH);
            self.draw_rect_border(rect.x + 46, rect.y + 35, rect.w - 58, 8, FM_BORDER_SOFT);
            self.fill_rect(
                rect.x + 47,
                rect.y + 36,
                ((rect.w - 60) * usage / 100).max(6),
                6,
                FM_DRIVE_FILL,
            );

            let size = if entry_is_dir {
                String::from("shell view")
            } else {
                Self::format_size(entry_size)
            };
            self.put_str(
                (rect.x + rect.w - 10 - size.len() as i32 * CW as i32).max(rect.x + 46) as usize,
                (rect.y + 10) as usize,
                &size,
                FM_TEXT,
            );
        }
    }

    pub(super) fn draw_drive_icon(&mut self, x: usize, y: usize) {
        self.fill_rect(x as i32, y as i32 + 8, 22, 8, FM_DRIVE);
        self.fill_rect(
            x as i32 + 2,
            y as i32 + 4,
            18,
            6,
            blend(FM_DRIVE, WHITE, 90),
        );
        self.fill_rect(x as i32 + 5, y as i32 + 11, 12, 2, FM_ACCENT);
        self.draw_rect_border(x as i32, y as i32 + 8, 22, 8, blend(FM_DRIVE, BLACK, 120));
    }

    pub(super) fn draw_file_icon(&mut self, x: usize, y: usize) {
        self.fill_rect(x as i32 + 1, y as i32, 10, 12, FM_FILE);
        self.draw_rect_border(x as i32 + 1, y as i32, 10, 12, blend(FM_FILE, BLACK, 120));
        self.fill_rect(x as i32 + 6, y as i32, 4, 3, WHITE);
    }

    pub(super) fn usage_ratio(entry: &DirEntryInfo) -> i32 {
        if entry.is_dir {
            ((entry.name.len() as i32 * 11) % 62) + 20
        } else if entry.size == 0 {
            8
        } else {
            ((entry.size % 100) as i32).clamp(12, 92)
        }
    }

    pub(super) fn clip_text(text: &str, max_chars: usize) -> String {
        if max_chars == 0 {
            return String::new();
        }
        if text.len() <= max_chars {
            return String::from(text);
        }
        if max_chars == 1 {
            return String::from("~");
        }
        let mut clipped = String::new();
        for ch in text.chars().take(max_chars - 1) {
            clipped.push(ch);
        }
        clipped.push('~');
        clipped
    }

    pub(super) fn put_str(&mut self, px: usize, py: usize, s: &str, color: u32) {
        let stride = self.window.width.max(0) as usize;
        let max_chars = stride.saturating_sub(px) / CW;
        for (ci, ch) in s.chars().take(max_chars).enumerate() {
            let glyph = font8x8::BASIC_FONTS
                .get(ch)
                .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
            for (gi, &byte) in glyph.iter().enumerate() {
                for bit in 0..8 {
                    if byte & (1 << bit) == 0 {
                        continue;
                    }
                    let x = px + ci * CW + bit;
                    let y = py + gi;
                    let idx = y * stride + x;
                    if idx < self.window.buf.len() {
                        self.window.buf[idx] = color;
                    }
                }
            }
        }
    }

    pub(super) fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let stride = self.window.width.max(0) as usize;
        let max_h = if stride > 0 {
            self.window.buf.len() / stride
        } else {
            0
        };
        let start_x = x.max(0) as usize;
        let start_y = y.max(0) as usize;
        let end_x = (x + w).max(0) as usize;
        let end_y = (y + h).max(0) as usize;
        for row in start_y..end_y.min(max_h) {
            let base = row * stride;
            for col in start_x..end_x.min(stride) {
                let idx = base + col;
                if idx < self.window.buf.len() {
                    self.window.buf[idx] = color;
                }
            }
        }
    }

    pub(super) fn draw_rect_border(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.fill_rect(x, y, w, 1, color);
        self.fill_rect(x, y + h - 1, w, 1, color);
        self.fill_rect(x, y, 1, h, color);
        self.fill_rect(x + w - 1, y, 1, h, color);
    }
}

pub(super) fn blend(a: u32, b: u32, t: u32) -> u32 {
    let clamped = t.min(255);
    let inv = 255 - clamped;
    let r = (((a >> 16) & 0xFF) * inv + ((b >> 16) & 0xFF) * clamped) / 255;
    let g = (((a >> 8) & 0xFF) * inv + ((b >> 8) & 0xFF) * clamped) / 255;
    let bl = ((a & 0xFF) * inv + (b & 0xFF) * clamped) / 255;
    (r << 16) | (g << 8) | bl
}

pub(super) fn fmt_push_u(s: &mut String, mut n: u64) {
    if n == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20usize;
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    for &b in &buf[i..] {
        s.push(b as char);
    }
}
