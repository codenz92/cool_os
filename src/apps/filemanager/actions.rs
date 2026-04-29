use super::drawing::fmt_push_u;
use super::*;

impl FileManagerApp {
    pub fn load_dir(&mut self, dir: &str) {
        self.load_dir_with_state(dir, None, None);
    }

    pub(super) fn load_dir_with_state(
        &mut self,
        dir: &str,
        selected_name: Option<&str>,
        preferred_offset: Option<usize>,
    ) {
        self.path.clear();
        self.path.push_str(dir);
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.clear();
            tab.push_str(dir);
        }
        self.search_filter.clear();
        self.search_active = false;
        let (mut new_entries, mut entry_kinds, mut entry_paths) = self.load_entries_for_path(dir);
        Self::sort_entries(
            &mut new_entries,
            &mut entry_kinds,
            &mut entry_paths,
            self.sort_column,
            self.sort_desc,
        );
        self.all_entries = new_entries.clone();
        self.all_entry_kinds = entry_kinds.clone();
        self.all_entry_paths = entry_paths.clone();
        self.entries = new_entries;
        self.entry_kinds = entry_kinds;
        self.entry_paths = entry_paths;
        self.entry_child_counts = self.compute_child_counts();
        self.selected.clear();
        if let Some(name) = selected_name {
            if let Some(pos) = self
                .entries
                .iter()
                .position(|entry| entry.name.eq_ignore_ascii_case(name))
            {
                self.selected.push(pos);
            }
        }
        if self.selected.is_empty() {
            if !self.entries.is_empty() {
                self.selected.push(0);
            }
        }
        self.focused = self.selected.first().copied();
        let visible_rows = self.visible_row_capacity();
        let max_offset = self.entries.len().saturating_sub(visible_rows.max(1));
        self.offset = preferred_offset.unwrap_or(0).min(max_offset);
        self.ensure_selected_visible();
        self.context_menu = None;
        self.status_note = None;
        self.render();
    }

    pub(super) fn set_selected_single(&mut self, idx: usize) {
        self.selected.clear();
        self.selected.push(idx);
        self.focused = Some(idx);
    }

    pub(super) fn navigate_to(&mut self, path: &str) {
        let old = self.path.clone();
        self.forward_stack.clear();
        self.back_stack.push(old);
        self.load_dir(path);
    }

    pub(super) fn navigate_back(&mut self) {
        if let Some(prev) = self.back_stack.pop() {
            let current = self.path.clone();
            self.forward_stack.push(current);
            self.load_dir(&prev);
        }
    }

    pub(super) fn navigate_forward(&mut self) {
        if let Some(next) = self.forward_stack.pop() {
            let current = self.path.clone();
            self.back_stack.push(current);
            self.load_dir(&next);
        }
    }

    pub fn handle_key(&mut self, c: char) {
        if self.handle_modal_key(c) {
            self.render();
            return;
        }
        if self.search_active {
            self.handle_search_key(c);
            return;
        }
        let changed = self.handle_nav_key(c);
        if changed {
            self.render();
        }
    }

    pub(super) fn handle_search_key(&mut self, c: char) {
        match c {
            '\u{001B}' => {
                self.search_active = false;
                self.search_filter.clear();
                self.apply_search_filter();
            }
            '\u{0008}' => {
                if !self.search_filter.is_empty() {
                    self.search_filter.pop();
                    self.apply_search_filter();
                }
            }
            '\n' | '\r' => {
                self.search_active = false;
            }
            _ if !c.is_control() => {
                self.search_filter.push(c.to_ascii_uppercase());
                self.apply_search_filter();
            }
            _ => {}
        }
        self.render();
    }

    pub(super) fn apply_search_filter(&mut self) {
        let selected_path = self
            .focused
            .and_then(|idx| self.entry_paths.get(idx).cloned());
        if self.search_filter.is_empty() {
            self.entries = self.all_entries.clone();
            self.entry_kinds = self.all_entry_kinds.clone();
            self.entry_paths = self.all_entry_paths.clone();
        } else {
            let filter = self.search_filter.to_ascii_uppercase();
            let mut filtered_entries = Vec::new();
            let mut filtered_kinds = Vec::new();
            let mut filtered_paths = Vec::new();
            self.collect_search_entries(
                &self.path.clone(),
                &filter,
                &mut filtered_entries,
                &mut filtered_kinds,
                &mut filtered_paths,
            );
            if filtered_entries.is_empty() {
                for ((entry, kind), path) in self
                    .all_entries
                    .iter()
                    .zip(self.all_entry_kinds.iter())
                    .zip(self.all_entry_paths.iter())
                {
                    if entry.name.to_ascii_uppercase().contains(&filter) {
                        filtered_entries.push(entry.clone());
                        filtered_kinds.push(*kind);
                        filtered_paths.push(path.clone());
                    }
                }
            }
            Self::sort_entries(
                &mut filtered_entries,
                &mut filtered_kinds,
                &mut filtered_paths,
                self.sort_column,
                self.sort_desc,
            );
            self.entries = filtered_entries;
            self.entry_kinds = filtered_kinds;
            self.entry_paths = filtered_paths;
        }
        self.selected.clear();
        if let Some(path) = selected_path {
            if let Some(pos) = self
                .entry_paths
                .iter()
                .position(|entry_path| entry_path.eq_ignore_ascii_case(&path))
            {
                self.selected.push(pos);
            }
        }
        if self.selected.is_empty() && !self.entries.is_empty() {
            self.selected.push(0);
        }
        self.focused = self.selected.first().copied();
        self.offset = 0;
        self.entry_child_counts = self.compute_child_counts();
    }

    pub(super) fn handle_nav_key(&mut self, c: char) -> bool {
        match c {
            '\u{F700}' => self.move_focus_by(-1),
            '\u{F701}' => self.move_focus_by(1),
            '\u{F704}' => self.move_focus_to(0),
            '\u{F705}' => self.move_focus_to(self.entries.len().saturating_sub(1)),
            '\u{F706}' => self.move_focus_by(-(self.page_step() as isize)),
            '\u{F707}' => self.move_focus_by(self.page_step() as isize),
            '\u{F702}' | '\u{0008}' => {
                self.navigate_parent();
                false
            }
            '\u{F703}' => {
                self.open_selected();
                false
            }
            '\u{F708}' => {
                self.rename_focused();
                false
            }
            '\n' | '\r' => {
                self.open_selected();
                false
            }
            '\u{001B}' => {
                self.context_menu = None;
                true
            }
            ' ' => self.toggle_focused_selection(),
            'a' | 'A' => self.select_all_entries(),
            'c' | 'C' => self.copy_selection(false),
            'x' | 'X' => self.copy_selection(true),
            'v' | 'V' => {
                self.paste_clipboard();
                false
            }
            'p' | 'P' => {
                self.open_properties_for_focus();
                false
            }
            'r' | 'R' => {
                self.refresh_current_dir();
                false
            }
            't' | 'T' => {
                self.open_new_tab();
                false
            }
            'w' | 'W' => {
                self.close_active_tab();
                false
            }
            's' | 'S' => {
                self.split_view = !self.split_view;
                self.status_note = Some(if self.split_view {
                    String::from("split view enabled")
                } else {
                    String::from("split view disabled")
                });
                true
            }
            '\u{007F}' => {
                let targets = self.selected.clone();
                if !targets.is_empty() {
                    self.confirm_delete_entries(&targets);
                }
                false
            }
            _ => false,
        }
    }

    pub fn rename_focused(&mut self) -> bool {
        let Some(idx) = self.focused else {
            return false;
        };
        if !self.entry_can_rename(idx) {
            self.status_note = Some(String::from("selected item cannot be renamed"));
            return false;
        }
        let Some(entry) = self.entries.get(idx) else {
            return false;
        };
        let name = entry.name.clone();
        self.modal = Some(ModalState::Name(NameDialogState {
            mode: NameDialogMode::Rename(idx),
            input: name.clone(),
            cursor: name.len(),
            error: None,
        }));
        true
    }

    pub fn handle_click(&mut self, lx: i32, ly: i32) {
        if self.handle_modal_click(lx, ly) {
            self.render();
            return;
        }

        if self.handle_context_menu_click(lx, ly) {
            self.render();
            return;
        }

        if let Some(path) = self.hit_navigation(lx, ly) {
            self.context_menu = None;
            self.navigate_to(&path);
            return;
        }

        if ly >= COMMAND_H && ly < COMMAND_H + PATHBAR_H {
            self.context_menu = None;
            if self.back_button_rect().hit(lx, ly) {
                self.navigate_back();
                return;
            }
            if self.forward_button_rect().hit(lx, ly) {
                self.navigate_forward();
                return;
            }
            if self.search_rect(self.layout()).hit(lx, ly) {
                self.search_active = true;
                self.render();
                return;
            }
            if let Some(path) = self.hit_breadcrumb(lx, ly) {
                self.navigate_to(&path);
            }
            return;
        }

        if let Some(column) = self.hit_file_header_column(lx, ly) {
            self.context_menu = None;
            self.change_sort(column);
            return;
        }

        self.context_menu = None;
        self.search_active = false;
        if let Some(idx) = self.hit_main_entry(lx, ly) {
            self.set_selected_single(idx);
            self.render();
        }
    }

    pub fn handle_secondary_click(&mut self, lx: i32, ly: i32) {
        if self.modal.is_some() {
            return;
        }
        let layout = self.layout();
        if lx < layout.main_x || ly < COMMAND_H + PATHBAR_H || ly >= layout.status_y {
            self.context_menu = None;
            self.render();
            return;
        }
        let target = self.hit_main_entry(lx, ly);
        if let Some(idx) = target {
            if !self.selected.contains(&idx) {
                self.set_selected_single(idx);
            }
        }
        self.context_menu = Some(self.clamp_context_menu(lx, ly, target));
        self.render();
    }

    pub fn handle_dbl_click(&mut self, lx: i32, ly: i32) {
        if self.modal.is_some() || self.context_menu.is_some() {
            return;
        }
        if let Some(idx) = self.hit_main_entry(lx, ly) {
            self.set_selected_single(idx);
            self.open_selected();
        }
    }

    pub fn drag_paths_at(&mut self, lx: i32, ly: i32) -> Option<Vec<String>> {
        if self.modal.is_some() || self.context_menu.is_some() {
            return None;
        }
        let idx = self.hit_main_entry(lx, ly)?;
        if !matches!(self.entry_kind(idx), EntryKind::Fs) {
            return None;
        }
        if !self.selected.contains(&idx) {
            self.set_selected_single(idx);
        }
        let mut paths = Vec::new();
        for &selected in self.selected.iter() {
            if matches!(self.entry_kind(selected), EntryKind::Fs) {
                if let Some(path) = self.entry_paths.get(selected) {
                    paths.push(path.clone());
                }
            }
        }
        if paths.is_empty() {
            None
        } else {
            Some(paths)
        }
    }

    pub fn drop_paths(&mut self, paths: Vec<String>, cut: bool) -> bool {
        if paths.is_empty() {
            return false;
        }
        let entries = paths
            .iter()
            .map(|path| FileTarget {
                path: path.clone(),
                name: Self::path_name(path),
                is_dir: crate::fat32::list_dir(path).is_some(),
            })
            .collect();
        self.clipboard = Some(ClipboardState { entries, cut });
        self.paste_clipboard();
        true
    }

    pub fn take_open_request(&mut self) -> Option<FileManagerOpenRequest> {
        self.pending_open.take()
    }

    pub fn handle_scroll(&mut self, delta: i32) {
        if self.path == "/" {
            return;
        }
        let max_offset = self.entries.len().saturating_sub(self.total_rows.max(1));
        let new_offset =
            (self.offset as i32 + delta.signum() * 3).clamp(0, max_offset as i32) as usize;
        if new_offset != self.offset {
            self.offset = new_offset;
            self.render();
        }
    }

    pub fn update(&mut self) {
        if self.window.width != self.last_width || self.window.height != self.last_height {
            self.last_width = self.window.width;
            self.last_height = self.window.height;
            self.render();
            return;
        }

        if self.path != "/" {
            let expected = self.offset as i32 * LIST_ROW_H;
            if self.window.scroll.offset != expected {
                let visible_rows = self.visible_row_capacity();
                let max_row = self.entries.len().saturating_sub(visible_rows.max(1));
                self.offset = ((self.window.scroll.offset / LIST_ROW_H) as usize).min(max_row);
                self.render();
            }
        }
    }

    pub fn refresh_current_dir(&mut self) {
        let path = self.path.clone();
        let selected = self.selected_name();
        let offset = self.offset;
        self.load_dir_with_state(&path, selected.as_deref(), Some(offset));
        self.status_note = Some(String::from("refreshed"));
        self.render();
    }

    pub(super) fn open_selected(&mut self) {
        let sel = match self.focused.or_else(|| self.selected.first().copied()) {
            Some(s) => s,
            None => return,
        };
        if sel >= self.entries.len() {
            return;
        }
        if let Some(app) = self.desktop_app_for_idx(sel) {
            self.pending_open = Some(FileManagerOpenRequest::App(app));
            return;
        }
        let abs = self.make_abs(sel);
        if self.is_dir_idx(sel) {
            self.navigate_to(&abs);
        } else {
            match crate::app_metadata::association_for(&abs, false) {
                crate::app_metadata::Association::Executable => {
                    crate::app_lifecycle::record_file(&abs);
                    self.pending_open = Some(FileManagerOpenRequest::Exec(abs));
                }
                crate::app_metadata::Association::AppShortcut(app) => {
                    self.pending_open = Some(FileManagerOpenRequest::App(app));
                }
                _ => {
                    crate::app_lifecycle::record_file(&abs);
                    self.pending_open = Some(FileManagerOpenRequest::File(abs));
                }
            }
        }
    }

    pub(super) fn ensure_selected_visible(&mut self) {
        if self.path == "/" {
            return;
        }
        let sel = match self.focused.or_else(|| self.selected.first().copied()) {
            Some(s) => s,
            None => return,
        };
        let files = self.file_indices();
        let sel = files
            .iter()
            .position(|&idx| idx == sel)
            .unwrap_or(self.offset);
        let visible_rows = self.visible_row_capacity().max(1);
        if sel < self.offset {
            self.offset = sel;
        } else if sel >= self.offset + visible_rows {
            self.offset = sel.saturating_sub(visible_rows - 1);
        }
    }

    pub(super) fn make_abs(&self, idx: usize) -> String {
        self.entry_paths
            .get(idx)
            .cloned()
            .unwrap_or_else(|| Self::join_path(&self.path, &self.entries[idx].name))
    }

    pub(super) fn is_dir_idx(&self, idx: usize) -> bool {
        self.entries.get(idx).map(|e| e.is_dir).unwrap_or(false)
    }

    pub(super) fn format_size(size: u32) -> String {
        Self::format_size_u64(size as u64)
    }

    pub(super) fn format_size_u64(size: u64) -> String {
        if size >= 1024 * 1024 {
            let mut s = Self::fmt_u64(size / (1024 * 1024));
            s.push_str(" MB");
            s
        } else if size >= 1024 {
            let mut s = Self::fmt_u64(size / 1024);
            s.push_str(" KB");
            s
        } else {
            let mut s = Self::fmt_u64(size);
            s.push_str(" B");
            s
        }
    }

    pub(super) fn fmt_u64(n: u64) -> String {
        if n == 0 {
            return String::from("0");
        }
        let mut digits = [0u8; 20];
        let mut len = 0usize;
        let mut v = n;
        while v > 0 {
            digits[len] = b'0' + (v % 10) as u8;
            v /= 10;
            len += 1;
        }
        let mut s = String::new();
        for i in (0..len).rev() {
            s.push(digits[i] as char);
        }
        s
    }

    pub(super) fn file_ext(name: &str) -> &str {
        match name.rfind('.') {
            Some(pos) if pos < name.len() - 1 => &name[pos + 1..],
            _ => "",
        }
    }

    pub(super) fn type_label(name: &str, is_dir: bool) -> &'static str {
        if is_dir {
            return "Folder";
        }
        let ext = Self::file_ext(name);
        if ext.eq_ignore_ascii_case("TXT")
            || ext.eq_ignore_ascii_case("MD")
            || ext.eq_ignore_ascii_case("LOG")
            || ext.eq_ignore_ascii_case("RST")
            || ext.eq_ignore_ascii_case("CSV")
        {
            "Text"
        } else if ext.eq_ignore_ascii_case("RS") {
            "Rust"
        } else if ext.eq_ignore_ascii_case("C") || ext.eq_ignore_ascii_case("H") {
            "C Source"
        } else if ext.eq_ignore_ascii_case("CPP")
            || ext.eq_ignore_ascii_case("HPP")
            || ext.eq_ignore_ascii_case("CC")
        {
            "C++"
        } else if ext.eq_ignore_ascii_case("ELF") || ext.eq_ignore_ascii_case("BIN") {
            "Binary"
        } else if ext.eq_ignore_ascii_case("JSON")
            || ext.eq_ignore_ascii_case("TOML")
            || ext.eq_ignore_ascii_case("YAML")
            || ext.eq_ignore_ascii_case("YML")
        {
            "Config"
        } else if ext.eq_ignore_ascii_case("SH") || ext.eq_ignore_ascii_case("BASH") {
            "Script"
        } else if ext.eq_ignore_ascii_case("PY") {
            "Python"
        } else if ext.eq_ignore_ascii_case("JS") || ext.eq_ignore_ascii_case("TS") {
            "JavaScript"
        } else if ext.eq_ignore_ascii_case("ASM") || ext.eq_ignore_ascii_case("S") {
            "Assembly"
        } else {
            "File"
        }
    }

    pub(super) fn type_label_for_kind(entry: &DirEntryInfo, kind: EntryKind) -> &'static str {
        match kind {
            EntryKind::Fs => Self::type_label(&entry.name, entry.is_dir),
            EntryKind::DesktopApp(_) => "Application",
        }
    }

    pub(super) fn is_editable_text_name(name: &str) -> bool {
        let ext = Self::file_ext(name);
        for editable in [
            "TXT", "MD", "LOG", "RST", "CSV", "RS", "TOML", "JSON", "YAML", "YML", "SH", "BASH",
            "C", "H", "PY", "JS", "TS", "ASM", "S", "INI", "CFG",
        ] {
            if ext.eq_ignore_ascii_case(editable) {
                return true;
            }
        }
        false
    }

    pub(super) fn selected_name(&self) -> Option<String> {
        self.selected
            .first()
            .and_then(|&idx| self.entries.get(idx))
            .map(|entry| entry.name.clone())
    }

    pub(super) fn visible_row_capacity(&self) -> usize {
        if self.path == "/" {
            return 0;
        }
        let layout = self.layout();
        let rows_y = self.file_rows_y(layout);
        let list_h = (layout.status_y - rows_y - 10).max(0);
        (list_h / LIST_ROW_H) as usize
    }

    pub(super) fn page_step(&self) -> usize {
        self.visible_row_capacity().max(6)
    }

    pub(super) fn move_focus_by(&mut self, delta: isize) -> bool {
        if self.entries.is_empty() {
            self.focused = None;
            self.selected.clear();
            return false;
        }
        let current = self
            .focused
            .unwrap_or_else(|| self.selected.first().copied().unwrap_or(0));
        let max = self.entries.len().saturating_sub(1) as isize;
        let next = (current as isize + delta).clamp(0, max) as usize;
        self.move_focus_to(next)
    }

    pub(super) fn move_focus_to(&mut self, idx: usize) -> bool {
        if self.entries.is_empty() {
            self.focused = None;
            self.selected.clear();
            return false;
        }
        let next = idx.min(self.entries.len() - 1);
        if self.focused == Some(next) && self.selected.len() == 1 && self.selected[0] == next {
            return false;
        }
        self.set_selected_single(next);
        self.ensure_selected_visible();
        true
    }

    pub(super) fn toggle_focused_selection(&mut self) -> bool {
        let Some(cursor) = self.focused.or_else(|| self.selected.first().copied()) else {
            return false;
        };
        if let Some(pos) = self.selected.iter().position(|&i| i == cursor) {
            if self.selected.len() > 1 {
                self.selected.remove(pos);
            }
        } else {
            self.selected.push(cursor);
            self.selected.sort_unstable();
        }
        self.focused = Some(cursor);
        true
    }

    pub(super) fn select_all_entries(&mut self) -> bool {
        self.selected.clear();
        for idx in 0..self.entries.len() {
            self.selected.push(idx);
        }
        self.focused = self.selected.first().copied();
        !self.selected.is_empty()
    }

    pub(super) fn navigate_parent(&mut self) {
        if self.path == "/" {
            return;
        }
        let parent = Self::parent_path(&self.path);
        self.navigate_to(&parent);
    }

    pub(super) fn open_new_tab(&mut self) {
        if self.tabs.len() < 6 {
            self.tabs.push(self.path.clone());
            self.active_tab = self.tabs.len() - 1;
            self.status_note = Some(String::from("new tab opened"));
            self.render();
        }
    }

    pub(super) fn close_active_tab(&mut self) {
        if self.tabs.len() <= 1 {
            self.status_note = Some(String::from("last tab kept open"));
            self.render();
            return;
        }
        self.tabs.remove(self.active_tab);
        self.active_tab = self.active_tab.saturating_sub(1).min(self.tabs.len() - 1);
        let path = self.tabs[self.active_tab].clone();
        self.load_dir(&path);
    }

    pub(super) fn sort_entries(
        entries: &mut [DirEntryInfo],
        entry_kinds: &mut [EntryKind],
        entry_paths: &mut [String],
        sort_column: SortColumn,
        sort_desc: bool,
    ) {
        let mut order: Vec<usize> = (0..entries.len()).collect();
        order.sort_by(|&a_idx, &b_idx| {
            let a = &entries[a_idx];
            let b = &entries[b_idx];
            if a.is_dir != b.is_dir {
                return if a.is_dir {
                    core::cmp::Ordering::Less
                } else {
                    core::cmp::Ordering::Greater
                };
            }

            let base = match sort_column {
                SortColumn::Name => a
                    .name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase()),
                SortColumn::Size => a.size.cmp(&b.size).then_with(|| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                }),
                SortColumn::Type => Self::type_label_for_kind(a, entry_kinds[a_idx])
                    .cmp(Self::type_label_for_kind(b, entry_kinds[b_idx]))
                    .then_with(|| {
                        a.name
                            .to_ascii_lowercase()
                            .cmp(&b.name.to_ascii_lowercase())
                    }),
            };

            if sort_desc {
                base.reverse()
            } else {
                base
            }
        });

        let old_entries = entries.to_vec();
        let old_kinds = entry_kinds.to_vec();
        let old_paths = entry_paths.to_vec();
        for (dst, src) in order.into_iter().enumerate() {
            entries[dst] = old_entries[src].clone();
            entry_kinds[dst] = old_kinds[src];
            entry_paths[dst] = old_paths[src].clone();
        }
    }

    pub(super) fn resort_entries(&mut self, note: &str) {
        let selected = self.selected_name();
        Self::sort_entries(
            &mut self.entries,
            &mut self.entry_kinds,
            &mut self.entry_paths,
            self.sort_column,
            self.sort_desc,
        );
        self.selected.clear();
        if let Some(name) = selected {
            if let Some(pos) = self
                .entries
                .iter()
                .position(|entry| entry.name.eq_ignore_ascii_case(&name))
            {
                self.selected.push(pos);
            }
        }
        if self.selected.is_empty() {
            if !self.entries.is_empty() {
                self.selected.push(0);
            }
        }
        self.focused = self.selected.first().copied();
        self.entry_child_counts = self.compute_child_counts();
        self.ensure_selected_visible();
        self.status_note = Some(String::from(note));
        self.render();
    }

    pub(super) fn change_sort(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_desc = !self.sort_desc;
        } else {
            self.sort_column = column;
            self.sort_desc = false;
        }
        let mut note = String::from("sort ");
        note.push_str(column.label());
        note.push(' ');
        note.push_str(if self.sort_desc { "desc" } else { "asc" });
        self.resort_entries(&note);
    }

    pub(super) fn load_entries_for_path(
        &self,
        dir: &str,
    ) -> (Vec<DirEntryInfo>, Vec<EntryKind>, Vec<String>) {
        let mut entries = crate::fat32::list_dir(dir).unwrap_or_default();
        let mut entry_kinds = alloc::vec![EntryKind::Fs; entries.len()];
        let mut entry_paths: Vec<String> = entries
            .iter()
            .map(|entry| Self::join_path(dir, &entry.name))
            .collect();

        if dir.eq_ignore_ascii_case("/Desktop") {
            for (label, app) in DESKTOP_APP_LINKS {
                if entries
                    .iter()
                    .any(|entry| entry.name.eq_ignore_ascii_case(label))
                {
                    continue;
                }
                entries.push(DirEntryInfo {
                    name: String::from(label),
                    is_dir: false,
                    size: 0,
                });
                entry_kinds.push(EntryKind::DesktopApp(app));
                entry_paths.push(Self::join_path(dir, label));
            }
        }

        (entries, entry_kinds, entry_paths)
    }

    pub(super) fn desktop_app_for_idx(&self, idx: usize) -> Option<&'static str> {
        match self.entry_kinds.get(idx).copied() {
            Some(EntryKind::DesktopApp(app)) => Some(app),
            _ => None,
        }
    }

    pub(super) fn entry_kind(&self, idx: usize) -> EntryKind {
        self.entry_kinds.get(idx).copied().unwrap_or(EntryKind::Fs)
    }

    pub(super) fn entry_can_rename(&self, idx: usize) -> bool {
        matches!(self.entry_kind(idx), EntryKind::Fs)
    }

    pub(super) fn entry_can_edit_text(&self, idx: usize) -> bool {
        matches!(self.entry_kind(idx), EntryKind::Fs)
            && self
                .entries
                .get(idx)
                .map(|entry| !entry.is_dir && Self::is_editable_text_name(&entry.name))
                .unwrap_or(false)
    }

    pub(super) fn ensure_current_dir_exists(&self) -> Result<(), &'static str> {
        if self.path == "/" || crate::fat32::list_dir(&self.path).is_some() {
            return Ok(());
        }
        crate::fat32::create_dir(&self.path).map_err(|err| err.as_str())
    }

    pub(super) fn context_menu_items(
        &self,
        target: Option<usize>,
    ) -> Vec<(&'static str, ContextAction)> {
        let mut items = Vec::new();
        if let Some(idx) = target {
            items.push(("Open", ContextAction::Open));
            if matches!(self.entry_kind(idx), EntryKind::Fs) {
                items.push(("Copy", ContextAction::Copy));
                items.push(("Cut", ContextAction::Cut));
            }
            if self.entry_can_edit_text(idx) {
                items.push(("Edit Text", ContextAction::EditText));
            }
            if self.entry_can_rename(idx) {
                items.push(("Rename", ContextAction::Rename));
            }
            if matches!(self.entry_kind(idx), EntryKind::Fs) {
                items.push(("Duplicate", ContextAction::Duplicate));
            }
            if matches!(self.entry_kind(idx), EntryKind::Fs) {
                items.push(("Delete", ContextAction::Delete));
            }
            items.push(("Properties", ContextAction::Properties));
        }
        if self.clipboard.is_some() {
            items.push(("Paste", ContextAction::Paste));
        }
        items.push(("New File", ContextAction::NewFile));
        items.push(("New Folder", ContextAction::NewFolder));
        items.push(("Refresh", ContextAction::Refresh));
        items
    }

    pub(super) fn handle_context_menu_click(&mut self, lx: i32, ly: i32) -> bool {
        let Some(menu) = self.context_menu.as_ref() else {
            return false;
        };
        let items = self.context_menu_items(menu.target);
        let rect = self.context_menu_rect(menu);
        if !rect.hit(lx, ly) {
            self.context_menu = None;
            return true;
        }

        let rel_y = ly - rect.y - 3;
        if rel_y < 0 {
            return true;
        }
        let idx = (rel_y / MENU_ROW_H) as usize;
        if let Some((_, action)) = items.get(idx).copied() {
            let target = menu.target;
            self.context_menu = None;
            self.run_context_action(action, target);
        }
        true
    }

    pub(super) fn run_context_action(&mut self, action: ContextAction, target: Option<usize>) {
        match action {
            ContextAction::Open => {
                if let Some(idx) = target {
                    self.set_selected_single(idx);
                    self.open_selected();
                }
            }
            ContextAction::Copy => {
                self.copy_target_or_selection(target, false);
            }
            ContextAction::Cut => {
                self.copy_target_or_selection(target, true);
            }
            ContextAction::Paste => self.paste_clipboard(),
            ContextAction::NewFile => {
                self.modal = Some(ModalState::Name(NameDialogState {
                    mode: NameDialogMode::NewFile,
                    input: String::from("NEWFILE.TXT"),
                    cursor: "NEWFILE.TXT".len(),
                    error: None,
                }));
            }
            ContextAction::NewFolder => {
                self.modal = Some(ModalState::Name(NameDialogState {
                    mode: NameDialogMode::NewFolder,
                    input: String::from("NEWDIR"),
                    cursor: "NEWDIR".len(),
                    error: None,
                }));
            }
            ContextAction::Rename => {
                if let Some(idx) = target {
                    if let Some(entry) = self.entries.get(idx) {
                        let name = entry.name.clone();
                        self.modal = Some(ModalState::Name(NameDialogState {
                            mode: NameDialogMode::Rename(idx),
                            input: name.clone(),
                            cursor: name.len(),
                            error: None,
                        }));
                    }
                }
            }
            ContextAction::EditText => {
                if let Some(idx) = target {
                    self.open_text_editor(idx);
                }
            }
            ContextAction::Delete => {
                let targets: Vec<usize> = if let Some(idx) = target {
                    if self.selected.contains(&idx) {
                        self.selected.clone()
                    } else {
                        alloc::vec![idx]
                    }
                } else {
                    self.selected.clone()
                };
                self.confirm_delete_entries(&targets);
            }
            ContextAction::Duplicate => {
                if let Some(idx) = target {
                    self.duplicate_entry(idx);
                }
            }
            ContextAction::Properties => {
                if let Some(idx) = target {
                    self.set_selected_single(idx);
                    self.open_properties(Some(idx));
                }
            }
            ContextAction::Refresh => self.refresh_current_dir(),
        }
    }

    pub(super) fn confirm_delete_entries(&mut self, indices: &[usize]) {
        let targets = self.targets_from_indices(indices);
        if targets.is_empty() {
            return;
        }
        let permanent = targets
            .iter()
            .all(|target| Self::path_is_trash_or_inside(&target.path));
        let mut message = String::new();
        fmt_push_u(&mut message, targets.len() as u64);
        message.push_str(if targets.len() == 1 {
            " item"
        } else {
            " items"
        });
        message.push_str(if permanent {
            " will be deleted permanently."
        } else {
            " will move to Trash."
        });
        self.modal = Some(ModalState::Confirm(ConfirmDialogState {
            title: String::from(if permanent {
                "Delete Item"
            } else {
                "Move to Trash"
            }),
            message,
            confirm_label: String::from(if permanent { "Delete" } else { "Trash" }),
            cancel_label: String::from("Cancel"),
            action: if permanent {
                ConfirmAction::Delete(targets)
            } else {
                ConfirmAction::Trash(targets)
            },
        }));
        self.render();
    }

    pub(super) fn delete_entries(&mut self, targets: &[FileTarget]) {
        let mut last_err: Option<String> = None;
        let mut deleted = 0usize;
        for target in targets {
            match self.delete_path_recursive(&target.path, target.is_dir) {
                Ok(()) => deleted += 1,
                Err(err) => {
                    last_err = Some(String::from(err.as_str()));
                }
            }
        }
        let current = self.path.clone();
        self.load_dir_with_state(&current, None, Some(self.offset));
        if let Some(err) = last_err {
            self.status_note = Some(err);
        } else {
            let mut note = String::new();
            fmt_push_u(&mut note, deleted as u64);
            note.push_str(" deleted");
            self.status_note = Some(note);
        }
        self.render();
    }

    pub(super) fn move_targets_to_trash(&mut self, targets: &[FileTarget]) {
        let mut last_err: Option<String> = None;
        let mut moved = 0usize;
        if let Err(err) = self.ensure_trash_dir() {
            self.status_note = Some(String::from(err.as_str()));
            self.render();
            return;
        }

        for target in targets {
            if Self::path_is_trash_or_inside(&target.path) {
                match self.delete_path_recursive(&target.path, target.is_dir) {
                    Ok(()) => moved += 1,
                    Err(err) => last_err = Some(String::from(err.as_str())),
                }
                continue;
            }
            let dst = self.unique_child_path(TRASH_PATH, &target.name);
            match self.copy_path_recursive(&target.path, &dst, target.is_dir) {
                Ok(()) => match self.delete_path_recursive(&target.path, target.is_dir) {
                    Ok(()) => moved += 1,
                    Err(err) => last_err = Some(String::from(err.as_str())),
                },
                Err(err) => last_err = Some(String::from(err.as_str())),
            }
        }

        let current = self.path.clone();
        self.load_dir_with_state(&current, None, Some(self.offset));
        if let Some(err) = last_err {
            self.status_note = Some(err);
        } else {
            let mut note = String::new();
            fmt_push_u(&mut note, moved as u64);
            note.push_str(" moved to Trash");
            self.status_note = Some(note);
        }
        self.render();
    }

    pub(super) fn duplicate_entry(&mut self, idx: usize) {
        let entry = match self.entries.get(idx) {
            Some(e) => e.clone(),
            None => return,
        };
        let src = self.make_abs(idx);
        let copy_name = self.unique_copy_name(&self.path, &entry.name);
        let dst = Self::join_path(&self.path, &copy_name);
        match self.copy_path_recursive(&src, &dst, entry.is_dir) {
            Ok(()) => {
                let current = self.path.clone();
                self.load_dir_with_state(&current, Some(&copy_name), Some(self.offset));
                self.status_note = Some(String::from("duplicated"));
                self.render();
            }
            Err(err) => {
                self.status_note = Some(String::from(err.as_str()));
                self.render();
            }
        }
    }

    pub(super) fn copy_target_or_selection(&mut self, target: Option<usize>, cut: bool) {
        let indices = if let Some(idx) = target {
            if self.selected.contains(&idx) {
                self.selected.clone()
            } else {
                alloc::vec![idx]
            }
        } else {
            self.selected.clone()
        };
        self.copy_indices_to_clipboard(&indices, cut);
    }

    pub(super) fn copy_selection(&mut self, cut: bool) -> bool {
        let indices = self.selected.clone();
        self.copy_indices_to_clipboard(&indices, cut)
    }

    pub(super) fn copy_indices_to_clipboard(&mut self, indices: &[usize], cut: bool) -> bool {
        let targets = self.targets_from_indices(indices);
        if targets.is_empty() {
            self.status_note = Some(String::from("nothing to copy"));
            self.render();
            return false;
        }
        let count = targets.len();
        let shared_paths = targets.iter().map(|target| target.path.clone()).collect();
        self.clipboard = Some(ClipboardState {
            entries: targets,
            cut,
        });
        crate::clipboard::set_paths(shared_paths, cut);
        let mut note = String::new();
        fmt_push_u(&mut note, count as u64);
        note.push_str(if count == 1 { " item " } else { " items " });
        note.push_str(if cut { "cut" } else { "copied" });
        self.status_note = Some(note);
        self.render();
        true
    }

    pub(super) fn paste_clipboard(&mut self) {
        self.paste_clipboard_with_policy(None);
    }

    pub(super) fn paste_clipboard_with_policy(&mut self, conflict_policy: Option<ConflictPolicy>) {
        let clipboard = if let Some(clipboard) = self.clipboard.clone() {
            clipboard
        } else if let Some((paths, cut)) = crate::clipboard::get_paths() {
            let entries = paths
                .iter()
                .map(|path| FileTarget {
                    path: path.clone(),
                    name: Self::path_name(path),
                    is_dir: crate::fat32::list_dir(path).is_some(),
                })
                .collect();
            ClipboardState { entries, cut }
        } else {
            self.status_note = Some(String::from("clipboard empty"));
            self.render();
            return;
        };
        if let Err(err) = self.ensure_current_dir_exists() {
            self.status_note = Some(String::from(err));
            self.render();
            return;
        }

        let job = crate::jobs::start(
            if clipboard.cut {
                "Move files"
            } else {
                "Copy files"
            },
            &self.path,
        );
        let mut pasted = 0usize;
        let mut last_err: Option<String> = None;
        let mut selected_name: Option<String> = None;
        let total = clipboard.entries.len().max(1);
        for (target_idx, target) in clipboard.entries.iter().enumerate() {
            if crate::jobs::is_cancelled(job) {
                last_err = Some(String::from("operation cancelled"));
                break;
            }
            if target.is_dir && Self::path_contains(&target.path, &self.path) {
                last_err = Some(String::from("cannot paste folder into itself"));
                continue;
            }
            let exists = self.child_name_exists(&self.path, &target.name);
            if exists && conflict_policy.is_none() {
                self.modal = Some(ModalState::Conflict(ConflictDialogState {
                    clipboard: clipboard.clone(),
                    name: target.name.clone(),
                }));
                crate::jobs::fail(job, "file conflict");
                self.status_note = Some(String::from("file conflict needs a choice"));
                self.render();
                return;
            }
            if exists && conflict_policy == Some(ConflictPolicy::Skip) {
                continue;
            }
            let dest_name = if exists && conflict_policy == Some(ConflictPolicy::Rename) {
                self.unique_child_name(&self.path, &target.name)
            } else {
                target.name.clone()
            };
            let dest_path = Self::join_path(&self.path, &dest_name);
            if exists && conflict_policy == Some(ConflictPolicy::Replace) {
                let existing_is_dir = crate::fat32::list_dir(&dest_path).is_some();
                if let Err(err) = self.delete_path_recursive(&dest_path, existing_is_dir) {
                    last_err = Some(String::from(err.as_str()));
                    continue;
                }
            }
            match self.copy_path_recursive(&target.path, &dest_path, target.is_dir) {
                Ok(()) => {
                    if clipboard.cut {
                        if let Err(err) = self.delete_path_recursive(&target.path, target.is_dir) {
                            last_err = Some(String::from(err.as_str()));
                            continue;
                        }
                    }
                    pasted += 1;
                    if selected_name.is_none() {
                        selected_name = Some(dest_name);
                    }
                }
                Err(err) => last_err = Some(String::from(err.as_str())),
            }
            let progress = (((target_idx + 1) * 100) / total).min(99) as u8;
            crate::jobs::progress(job, progress, &target.name);
        }

        if clipboard.cut && last_err.is_none() {
            self.clipboard = None;
        }
        let current = self.path.clone();
        self.load_dir_with_state(&current, selected_name.as_deref(), Some(self.offset));
        if let Some(err) = last_err {
            crate::jobs::fail(job, &err);
            self.status_note = Some(err);
        } else {
            let mut note = String::new();
            fmt_push_u(&mut note, pasted as u64);
            note.push_str(if clipboard.cut { " moved" } else { " pasted" });
            crate::jobs::complete(job, &note);
            self.status_note = Some(note);
        }
        self.render();
    }

    pub(super) fn target_for_idx(&self, idx: usize) -> Option<FileTarget> {
        let entry = self.entries.get(idx)?;
        if !matches!(self.entry_kind(idx), EntryKind::Fs) {
            return None;
        }
        let path = self.make_abs(idx);
        Some(FileTarget {
            path,
            name: entry.name.clone(),
            is_dir: entry.is_dir,
        })
    }

    pub(super) fn targets_from_indices(&self, indices: &[usize]) -> Vec<FileTarget> {
        let mut sorted = indices.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        let mut raw_targets = Vec::new();
        for idx in sorted {
            if let Some(target) = self.target_for_idx(idx) {
                raw_targets.push(target);
            }
        }
        raw_targets.sort_by(|a, b| a.path.len().cmp(&b.path.len()));
        let mut targets: Vec<FileTarget> = Vec::new();
        for target in raw_targets {
            if targets.iter().any(|existing| {
                existing.is_dir && Self::path_contains(&existing.path, &target.path)
            }) {
                continue;
            }
            targets.push(target);
        }
        targets
    }

    pub(super) fn ensure_trash_dir(&self) -> Result<(), crate::fat32::FsError> {
        if crate::fat32::list_dir(TRASH_PATH).is_some() {
            return Ok(());
        }
        crate::fat32::create_dir(TRASH_PATH)
    }

    pub(super) fn copy_path_recursive(
        &self,
        src: &str,
        dst: &str,
        is_dir: bool,
    ) -> Result<(), crate::fat32::FsError> {
        if is_dir {
            crate::vfs::vfs_create_dir(dst)?;
            let children = crate::vfs::vfs_list_dir(src).ok_or(crate::fat32::FsError::NotFound)?;
            for child in children {
                let child_src = Self::join_path(src, &child.name);
                let child_dst = Self::join_path(dst, &child.name);
                self.copy_path_recursive(&child_src, &child_dst, child.is_dir)?;
            }
            Ok(())
        } else {
            crate::vfs::vfs_copy_file(src, dst)
        }
    }

    pub(super) fn delete_path_recursive(
        &self,
        path: &str,
        is_dir: bool,
    ) -> Result<(), crate::fat32::FsError> {
        if path == "/" || path.eq_ignore_ascii_case(TRASH_PATH) {
            return Err(crate::fat32::FsError::InvalidPath);
        }
        if crate::security::is_protected_path(path) {
            return Err(crate::fat32::FsError::PermissionDenied);
        }
        if is_dir {
            let children = crate::vfs::vfs_list_dir(path).ok_or(crate::fat32::FsError::NotFound)?;
            for child in children {
                let child_path = Self::join_path(path, &child.name);
                self.delete_path_recursive(&child_path, child.is_dir)?;
            }
        }
        crate::vfs::vfs_delete(path)
    }

    pub(super) fn unique_copy_name(&self, parent: &str, name: &str) -> String {
        let base = Self::copy_name_candidate(name);
        self.unique_child_name(parent, &base)
    }

    pub(super) fn copy_name_candidate(name: &str) -> String {
        let ext = Self::file_ext(name);
        let stem = if ext.is_empty() {
            name
        } else {
            &name[..name.len() - ext.len() - 1]
        };
        let mut out = String::from(stem);
        out.push_str("_CPY");
        if !ext.is_empty() {
            out.push('.');
            out.push_str(ext);
        }
        out
    }

    pub(super) fn unique_child_path(&self, parent: &str, name: &str) -> String {
        Self::join_path(parent, &self.unique_child_name(parent, name))
    }

    pub(super) fn unique_child_name(&self, parent: &str, name: &str) -> String {
        if !self.child_name_exists(parent, name) {
            return String::from(name);
        }
        let ext = Self::file_ext(name);
        let stem = if ext.is_empty() {
            name
        } else {
            &name[..name.len() - ext.len() - 1]
        };
        for n in 1..10_000usize {
            let mut candidate = String::from(stem);
            candidate.push('_');
            fmt_push_u(&mut candidate, n as u64);
            if !ext.is_empty() {
                candidate.push('.');
                candidate.push_str(ext);
            }
            if !self.child_name_exists(parent, &candidate) {
                return candidate;
            }
        }
        let mut fallback = String::from(stem);
        fallback.push_str("_COPY");
        fallback
    }

    pub(super) fn child_name_exists(&self, parent: &str, name: &str) -> bool {
        crate::fat32::list_dir(parent)
            .unwrap_or_default()
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    pub(super) fn path_contains(parent: &str, child: &str) -> bool {
        let parent_upper = parent.to_ascii_uppercase();
        let child_upper = child.to_ascii_uppercase();
        child_upper == parent_upper
            || child_upper
                .strip_prefix(&parent_upper)
                .map(|suffix| suffix.starts_with('/'))
                .unwrap_or(false)
    }

    pub(super) fn open_properties_for_focus(&mut self) {
        self.open_properties(self.focused.or_else(|| self.selected.first().copied()));
    }

    pub(super) fn open_properties(&mut self, idx: Option<usize>) {
        let state = if let Some(idx) = idx {
            let Some(entry) = self.entries.get(idx) else {
                return;
            };
            let path = self.make_abs(idx);
            let target = self.target_for_idx(idx);
            let (recursive_size, child_count) = if entry.is_dir {
                let (size, count) = self.recursive_stats(&path, true);
                (Some(size), Some(count))
            } else {
                (None, None)
            };
            PropertiesState {
                target,
                path,
                name: entry.name.clone(),
                kind: String::from(Self::type_label_for_kind(entry, self.entry_kind(idx))),
                size: entry.size,
                recursive_size,
                child_count,
                note: if entry.is_dir {
                    String::from("Folder totals include nested contents.")
                } else if Self::file_ext(&entry.name).eq_ignore_ascii_case("ELF") {
                    String::from("Executable opens as a user process.")
                } else {
                    String::from("File opens with its default handler.")
                },
            }
        } else {
            let (size, count) = self.recursive_stats(&self.path, true);
            PropertiesState {
                target: None,
                path: self.path.clone(),
                name: Self::path_name(&self.path),
                kind: String::from("Folder"),
                size: 0,
                recursive_size: Some(size),
                child_count: Some(count),
                note: String::from("Current folder properties."),
            }
        };
        self.modal = Some(ModalState::Properties(state));
        self.render();
    }

    pub(super) fn recursive_stats(&self, path: &str, is_dir: bool) -> (u64, usize) {
        if !is_dir {
            return (
                crate::fat32::read_file(path)
                    .map(|b| b.len() as u64)
                    .unwrap_or(0),
                0,
            );
        }
        let mut total = 0u64;
        let mut count = 0usize;
        if let Some(children) = crate::fat32::list_dir(path) {
            for child in children {
                count += 1;
                let child_path = Self::join_path(path, &child.name);
                if child.is_dir {
                    let (child_total, child_count) = self.recursive_stats(&child_path, true);
                    total += child_total;
                    count += child_count;
                } else {
                    total += child.size as u64;
                }
            }
        }
        (total, count)
    }

    pub(super) fn join_child_path(&self, name: &str) -> String {
        Self::join_path(&self.path, name)
    }

    pub(super) fn join_path(parent: &str, name: &str) -> String {
        let mut path = String::from(parent);
        if !path.ends_with('/') {
            path.push('/');
        }
        path.push_str(name);
        path
    }

    pub(super) fn parent_path(path: &str) -> String {
        if path == "/" {
            return String::from("/");
        }
        let trimmed = path.trim_end_matches('/');
        match trimmed.rfind('/') {
            Some(0) | None => String::from("/"),
            Some(pos) => String::from(&trimmed[..pos]),
        }
    }

    pub(super) fn path_name(path: &str) -> String {
        let trimmed = path.trim_end_matches('/');
        match trimmed.rfind('/') {
            Some(pos) if pos + 1 < trimmed.len() => String::from(&trimmed[pos + 1..]),
            _ => String::from(trimmed),
        }
    }

    pub(super) fn path_is_trash_or_inside(path: &str) -> bool {
        path.eq_ignore_ascii_case(TRASH_PATH)
            || path
                .strip_prefix(TRASH_PATH)
                .map(|suffix| suffix.starts_with('/'))
                .unwrap_or(false)
    }

    pub(super) fn compute_child_counts(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                if entry.is_dir {
                    self.entry_paths
                        .get(idx)
                        .and_then(|path| crate::fat32::list_dir(path))
                        .map(|children| children.len())
                        .unwrap_or(0)
                } else {
                    0
                }
            })
            .collect()
    }

    pub(super) fn collect_search_entries(
        &self,
        dir: &str,
        filter: &str,
        entries: &mut Vec<DirEntryInfo>,
        kinds: &mut Vec<EntryKind>,
        paths: &mut Vec<String>,
    ) {
        let Some(children) = crate::fat32::list_dir(dir) else {
            return;
        };
        for entry in children {
            let child_path = Self::join_path(dir, &entry.name);
            if entry.name.to_ascii_uppercase().contains(filter) {
                entries.push(entry.clone());
                kinds.push(EntryKind::Fs);
                paths.push(child_path.clone());
            }
            if entry.is_dir {
                self.collect_search_entries(&child_path, filter, entries, kinds, paths);
            }
        }
    }
}
