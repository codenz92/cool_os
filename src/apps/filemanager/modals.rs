use super::drawing::{blend, fmt_push_u};
use super::*;

impl FileManagerApp {
    pub(super) fn handle_modal_key(&mut self, c: char) -> bool {
        let changed = match self.modal.clone() {
            Some(ModalState::Name(_)) => match c {
                '\n' | '\r' => {
                    self.save_name_dialog();
                    true
                }
                '\u{001B}' => {
                    self.modal = None;
                    true
                }
                _ => match self.modal.as_mut() {
                    Some(ModalState::Name(state)) => Self::handle_name_dialog_key(state, c),
                    _ => false,
                },
            },
            Some(ModalState::TextEditor(_)) => match self.modal.as_mut() {
                Some(ModalState::TextEditor(state)) => Self::handle_text_editor_key(state, c),
                _ => false,
            },
            Some(ModalState::Confirm(_)) => self.handle_confirm_dialog_key(c),
            Some(ModalState::Properties(_)) => self.handle_properties_key(c),
            None => false,
        };
        if changed {
            self.sync_modal_state();
        }
        changed
    }

    pub(super) fn handle_modal_click(&mut self, lx: i32, ly: i32) -> bool {
        let handled = match self.modal.clone() {
            Some(ModalState::Name(_)) => self.handle_name_dialog_click(lx, ly),
            Some(ModalState::TextEditor(_)) => self.handle_text_editor_click(lx, ly),
            Some(ModalState::Confirm(_)) => self.handle_confirm_dialog_click(lx, ly),
            Some(ModalState::Properties(_)) => self.handle_properties_click(lx, ly),
            None => false,
        };
        if handled {
            self.sync_modal_state();
        }
        handled
    }

    pub(super) fn draw_modal(&mut self) {
        let Some(modal) = self.modal.clone() else {
            return;
        };
        match modal {
            ModalState::Name(state) => self.draw_name_dialog(&state),
            ModalState::TextEditor(state) => self.draw_text_editor(&state),
            ModalState::Confirm(state) => self.draw_confirm_dialog(&state),
            ModalState::Properties(state) => self.draw_properties_dialog(&state),
        }
    }

    pub(super) fn draw_name_dialog(&mut self, state: &NameDialogState) {
        let layout = self.layout();
        let rect = Self::centered_rect(layout, DIALOG_W, DIALOG_H);
        let input = self.name_dialog_input_rect(rect);
        let save = self.dialog_save_button_rect(rect);
        let cancel = self.dialog_cancel_button_rect(rect);
        let title = match state.mode {
            NameDialogMode::NewFile => "Create New File",
            NameDialogMode::NewFolder => "Create New Folder",
            NameDialogMode::Rename(_) => "Rename Item",
        };

        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL_ALT);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_BORDER);
        self.fill_rect(rect.x, rect.y, rect.w, 3, FM_SELECTION_GLOW);
        self.put_str(
            (rect.x + 14) as usize,
            (rect.y + 12) as usize,
            title,
            FM_TEXT,
        );
        self.put_str(
            (rect.x + 14) as usize,
            (rect.y + 28) as usize,
            "FAT names, long names allowed",
            FM_TEXT_MUTED,
        );

        self.fill_rect(input.x, input.y, input.w, input.h, FM_SEARCH);
        self.draw_rect_border(input.x, input.y, input.w, input.h, FM_BORDER_SOFT);
        let max_chars = ((input.w - 16).max(8) as usize) / CW;
        self.put_str(
            (input.x + 8) as usize,
            (input.y + 8) as usize,
            &Self::clip_text(&state.input, max_chars),
            FM_TEXT,
        );

        let cursor_char = state.cursor.min(state.input.chars().count());
        let cursor_x = input.x + 8 + (cursor_char as i32 * CW as i32).min(input.w - 14);
        self.fill_rect(cursor_x, input.y + 6, 2, input.h - 12, FM_SELECTION_GLOW);

        if let Some(error) = state.error.as_ref() {
            self.put_str(
                (rect.x + 14) as usize,
                (rect.y + 76) as usize,
                &Self::clip_text(error, ((rect.w - 28).max(0) as usize) / CW),
                blend(FM_ACCENT, WHITE, 72),
            );
        }

        self.draw_dialog_button(save, "Save");
        self.draw_dialog_button(cancel, "Cancel");
    }

    pub(super) fn draw_text_editor(&mut self, state: &TextEditorState) {
        let layout = self.layout();
        let rect = Self::centered_rect(layout, EDITOR_W, EDITOR_H);
        let text_rect = self.editor_text_rect(rect);
        let save = self.editor_save_button_rect(rect);
        let cancel = self.editor_cancel_button_rect(rect);
        let visible_lines = ((text_rect.h - 12).max(12) as usize) / 12;
        let (cursor_line, cursor_col) = Self::text_cursor_line_col(&state.text, state.cursor);
        let lines = Self::text_lines(&state.text);

        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL_ALT);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_BORDER);
        self.fill_rect(rect.x, rect.y, rect.w, 3, FM_SELECTION_GLOW);
        self.put_str(
            (rect.x + 14) as usize,
            (rect.y + 12) as usize,
            "Edit Text File",
            FM_TEXT,
        );
        self.put_str(
            (rect.x + 14) as usize,
            (rect.y + 28) as usize,
            &Self::clip_text(&state.path, ((rect.w - 28).max(0) as usize) / CW),
            FM_TEXT_MUTED,
        );

        self.fill_rect(
            text_rect.x,
            text_rect.y,
            text_rect.w,
            text_rect.h,
            FM_SEARCH,
        );
        self.draw_rect_border(
            text_rect.x,
            text_rect.y,
            text_rect.w,
            text_rect.h,
            FM_BORDER_SOFT,
        );
        let max_chars = ((text_rect.w - 18).max(8) as usize) / CW;
        for screen_line in 0..visible_lines {
            let doc_line = state.scroll_line + screen_line;
            let Some(line) = lines.get(doc_line) else {
                break;
            };
            self.put_str(
                (text_rect.x + 8) as usize,
                (text_rect.y + 8 + screen_line as i32 * 12) as usize,
                &Self::clip_text(line, max_chars),
                FM_TEXT,
            );
        }
        if cursor_line >= state.scroll_line && cursor_line < state.scroll_line + visible_lines {
            let cursor_x = text_rect.x + 8 + (cursor_col.min(max_chars) as i32 * CW as i32);
            let cursor_y = text_rect.y + 8 + ((cursor_line - state.scroll_line) as i32 * 12);
            self.fill_rect(cursor_x, cursor_y, 2, 10, FM_SELECTION_GLOW);
        }

        if let Some(error) = state.error.as_ref() {
            self.put_str(
                (rect.x + 14) as usize,
                (rect.y + rect.h - 52) as usize,
                &Self::clip_text(error, ((rect.w - 28).max(0) as usize) / CW),
                blend(FM_ACCENT, WHITE, 72),
            );
        }

        self.draw_dialog_button(save, "Save");
        self.draw_dialog_button(cancel, "Cancel");
    }

    pub(super) fn draw_confirm_dialog(&mut self, state: &ConfirmDialogState) {
        let layout = self.layout();
        let rect = Self::centered_rect(layout, CONFIRM_W, CONFIRM_H);
        let confirm = self.confirm_button_rect(rect);
        let cancel = self.confirm_cancel_button_rect(rect);

        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL_ALT);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_BORDER);
        self.fill_rect(rect.x, rect.y, rect.w, 3, FM_SELECTION_GLOW);
        self.put_str(
            (rect.x + 14) as usize,
            (rect.y + 14) as usize,
            &Self::clip_text(&state.title, ((rect.w - 28).max(0) as usize) / CW),
            FM_TEXT,
        );
        self.put_str(
            (rect.x + 14) as usize,
            (rect.y + 42) as usize,
            &Self::clip_text(&state.message, ((rect.w - 28).max(0) as usize) / CW),
            FM_TEXT_DIM,
        );
        let detail = match &state.action {
            ConfirmAction::Delete(_) => "Permanent delete cannot be undone.",
            ConfirmAction::Trash(_) => "Open Trash to restore or delete later.",
        };
        self.put_str(
            (rect.x + 14) as usize,
            (rect.y + 62) as usize,
            &Self::clip_text(detail, ((rect.w - 28).max(0) as usize) / CW),
            FM_TEXT_MUTED,
        );

        self.draw_dialog_button(confirm, &state.confirm_label);
        self.draw_dialog_button(cancel, &state.cancel_label);
    }

    pub(super) fn draw_properties_dialog(&mut self, state: &PropertiesState) {
        let layout = self.layout();
        let rect = Self::centered_rect(layout, PROPERTIES_W, PROPERTIES_H);
        let close = self.properties_close_button_rect(rect);

        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL_ALT);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_BORDER);
        self.fill_rect(rect.x, rect.y, rect.w, 3, FM_SELECTION_GLOW);
        self.put_str(
            (rect.x + 14) as usize,
            (rect.y + 12) as usize,
            "Properties",
            FM_TEXT,
        );
        self.put_str(
            (rect.x + 14) as usize,
            (rect.y + 28) as usize,
            &Self::clip_text(&state.name, ((rect.w - 28).max(0) as usize) / CW),
            FM_TEXT_DIM,
        );

        let size = state
            .recursive_size
            .map(Self::format_size_u64)
            .unwrap_or_else(|| Self::format_size(state.size));
        let mut child_count = String::from("-");
        if let Some(count) = state.child_count {
            child_count.clear();
            fmt_push_u(&mut child_count, count as u64);
        }
        let target_kind = if state.target.is_some() {
            "Item"
        } else {
            "Folder"
        };

        self.draw_property_row(rect.x + 14, rect.y + 54, "Path", &state.path, rect.w - 28);
        self.draw_property_row(rect.x + 14, rect.y + 86, "Kind", &state.kind, rect.w - 28);
        self.draw_property_row(rect.x + 14, rect.y + 118, "Size", &size, rect.w - 28);
        self.draw_property_row(
            rect.x + 14,
            rect.y + 150,
            "Contains",
            &child_count,
            rect.w - 28,
        );
        self.draw_property_row(
            rect.x + 14,
            rect.y + 182,
            target_kind,
            &state.note,
            rect.w - 28,
        );

        self.draw_dialog_button(close, "Close");
    }

    pub(super) fn draw_property_row(
        &mut self,
        x: i32,
        y: i32,
        label: &str,
        value: &str,
        width: i32,
    ) {
        self.put_str(x as usize, y as usize, label, FM_TEXT_MUTED);
        self.put_str(
            x as usize,
            (y + 13) as usize,
            &Self::clip_text(value, (width.max(0) as usize) / CW),
            FM_TEXT_DIM,
        );
        self.fill_rect(x, y + 24, width, 1, FM_BORDER_SOFT);
    }

    pub(super) fn draw_dialog_button(&mut self, rect: Rect, label: &str) {
        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_BORDER);
        let label_x = rect.x + ((rect.w - label.len() as i32 * CW as i32) / 2).max(6);
        self.put_str(label_x as usize, (rect.y + 7) as usize, label, FM_TEXT);
    }

    pub(super) fn centered_rect(layout: Layout, w: i32, h: i32) -> Rect {
        Rect {
            x: ((layout.width - w) / 2).max(14),
            y: ((layout.status_y - h) / 2).max(PATHBAR_H + 14),
            w,
            h,
        }
    }

    pub(super) fn name_dialog_input_rect(&self, rect: Rect) -> Rect {
        Rect {
            x: rect.x + 14,
            y: rect.y + 46,
            w: rect.w - 28,
            h: 26,
        }
    }

    pub(super) fn dialog_save_button_rect(&self, rect: Rect) -> Rect {
        Rect {
            x: rect.x + rect.w - 156,
            y: rect.y + rect.h - 36,
            w: 64,
            h: 24,
        }
    }

    pub(super) fn dialog_cancel_button_rect(&self, rect: Rect) -> Rect {
        Rect {
            x: rect.x + rect.w - 82,
            y: rect.y + rect.h - 36,
            w: 68,
            h: 24,
        }
    }

    pub(super) fn editor_text_rect(&self, rect: Rect) -> Rect {
        Rect {
            x: rect.x + 14,
            y: rect.y + 46,
            w: rect.w - 28,
            h: rect.h - 94,
        }
    }

    pub(super) fn editor_save_button_rect(&self, rect: Rect) -> Rect {
        self.dialog_save_button_rect(rect)
    }

    pub(super) fn editor_cancel_button_rect(&self, rect: Rect) -> Rect {
        self.dialog_cancel_button_rect(rect)
    }

    pub(super) fn confirm_button_rect(&self, rect: Rect) -> Rect {
        Rect {
            x: rect.x + rect.w - 166,
            y: rect.y + rect.h - 38,
            w: 74,
            h: 24,
        }
    }

    pub(super) fn confirm_cancel_button_rect(&self, rect: Rect) -> Rect {
        Rect {
            x: rect.x + rect.w - 82,
            y: rect.y + rect.h - 38,
            w: 68,
            h: 24,
        }
    }

    pub(super) fn properties_close_button_rect(&self, rect: Rect) -> Rect {
        Rect {
            x: rect.x + rect.w - 82,
            y: rect.y + rect.h - 36,
            w: 68,
            h: 24,
        }
    }

    pub(super) fn handle_name_dialog_key(state: &mut NameDialogState, c: char) -> bool {
        match c {
            '\u{0008}' => {
                if state.cursor > 0 {
                    state.cursor -= 1;
                    let byte = Self::char_to_byte_index(&state.input, state.cursor);
                    let next = Self::char_to_byte_index(&state.input, state.cursor + 1);
                    state.input.replace_range(byte..next, "");
                    state.error = None;
                    return true;
                }
            }
            '\u{F702}' => {
                if state.cursor > 0 {
                    state.cursor -= 1;
                    return true;
                }
            }
            '\u{F703}' => {
                let len = state.input.chars().count();
                if state.cursor < len {
                    state.cursor += 1;
                    return true;
                }
            }
            _ if !c.is_control() => {
                let byte = Self::char_to_byte_index(&state.input, state.cursor);
                state.input.insert(byte, c);
                state.cursor += 1;
                state.error = None;
                return true;
            }
            _ => {}
        }
        false
    }

    pub(super) fn handle_text_editor_key(state: &mut TextEditorState, c: char) -> bool {
        match c {
            '\u{0008}' => {
                if state.cursor > 0 {
                    state.cursor -= 1;
                    let byte = Self::char_to_byte_index(&state.text, state.cursor);
                    let next = Self::char_to_byte_index(&state.text, state.cursor + 1);
                    state.text.replace_range(byte..next, "");
                    state.error = None;
                    return true;
                }
            }
            '\n' => {
                let byte = Self::char_to_byte_index(&state.text, state.cursor);
                state.text.insert(byte, '\n');
                state.cursor += 1;
                state.error = None;
                return true;
            }
            '\t' => {
                for _ in 0..4 {
                    let byte = Self::char_to_byte_index(&state.text, state.cursor);
                    state.text.insert(byte, ' ');
                    state.cursor += 1;
                }
                state.error = None;
                return true;
            }
            '\u{F702}' => {
                if state.cursor > 0 {
                    state.cursor -= 1;
                    return true;
                }
            }
            '\u{F703}' => {
                let len = state.text.chars().count();
                if state.cursor < len {
                    state.cursor += 1;
                    return true;
                }
            }
            '\u{F700}' => {
                let (line, col) = Self::text_cursor_line_col(&state.text, state.cursor);
                if line > 0 {
                    state.cursor = Self::text_cursor_from_line_col(&state.text, line - 1, col);
                    return true;
                }
            }
            '\u{F701}' => {
                let (line, col) = Self::text_cursor_line_col(&state.text, state.cursor);
                state.cursor = Self::text_cursor_from_line_col(&state.text, line + 1, col);
                return true;
            }
            _ if !c.is_control() => {
                let byte = Self::char_to_byte_index(&state.text, state.cursor);
                state.text.insert(byte, c);
                state.cursor += 1;
                state.error = None;
                return true;
            }
            _ => {}
        }
        false
    }

    pub(super) fn handle_name_dialog_click(&mut self, lx: i32, ly: i32) -> bool {
        let layout = self.layout();
        let rect = Self::centered_rect(layout, DIALOG_W, DIALOG_H);
        let save = self.dialog_save_button_rect(rect);
        let cancel = self.dialog_cancel_button_rect(rect);
        let input = self.name_dialog_input_rect(rect);
        if save.hit(lx, ly) {
            self.save_name_dialog();
            return true;
        }
        if cancel.hit(lx, ly) {
            self.modal = None;
            return true;
        }
        if input.hit(lx, ly) {
            if let Some(ModalState::Name(state)) = self.modal.as_mut() {
                state.cursor = state.input.chars().count();
                state.error = None;
            }
            return true;
        }
        true
    }

    pub(super) fn handle_text_editor_click(&mut self, lx: i32, ly: i32) -> bool {
        let layout = self.layout();
        let rect = Self::centered_rect(layout, EDITOR_W, EDITOR_H);
        let save = self.editor_save_button_rect(rect);
        let cancel = self.editor_cancel_button_rect(rect);
        let text_rect = self.editor_text_rect(rect);
        if save.hit(lx, ly) {
            self.save_text_editor();
            return true;
        }
        if cancel.hit(lx, ly) {
            self.modal = None;
            return true;
        }
        if text_rect.hit(lx, ly) {
            if let Some(ModalState::TextEditor(state)) = self.modal.as_mut() {
                let max_chars = ((text_rect.w - 18).max(8) as usize) / CW;
                let line = state.scroll_line + ((ly - text_rect.y - 8).max(0) / 12) as usize;
                let col = ((lx - text_rect.x - 8).max(0) as usize / CW).min(max_chars);
                state.cursor = Self::text_cursor_from_line_col(&state.text, line, col);
                state.error = None;
            }
            return true;
        }
        true
    }

    pub(super) fn handle_confirm_dialog_key(&mut self, c: char) -> bool {
        match c {
            '\n' | '\r' => {
                self.accept_confirm_dialog();
                true
            }
            '\u{001B}' => {
                self.modal = None;
                true
            }
            _ => false,
        }
    }

    pub(super) fn handle_properties_key(&mut self, c: char) -> bool {
        match c {
            '\n' | '\r' | '\u{001B}' | ' ' => {
                self.modal = None;
                true
            }
            _ => false,
        }
    }

    pub(super) fn handle_confirm_dialog_click(&mut self, lx: i32, ly: i32) -> bool {
        let layout = self.layout();
        let rect = Self::centered_rect(layout, CONFIRM_W, CONFIRM_H);
        if self.confirm_button_rect(rect).hit(lx, ly) {
            self.accept_confirm_dialog();
            return true;
        }
        if self.confirm_cancel_button_rect(rect).hit(lx, ly) {
            self.modal = None;
            return true;
        }
        true
    }

    pub(super) fn handle_properties_click(&mut self, lx: i32, ly: i32) -> bool {
        let layout = self.layout();
        let rect = Self::centered_rect(layout, PROPERTIES_W, PROPERTIES_H);
        if self.properties_close_button_rect(rect).hit(lx, ly) {
            self.modal = None;
            return true;
        }
        true
    }

    pub(super) fn accept_confirm_dialog(&mut self) {
        let Some(ModalState::Confirm(state)) = self.modal.take() else {
            return;
        };
        match state.action {
            ConfirmAction::Trash(targets) => self.move_targets_to_trash(&targets),
            ConfirmAction::Delete(targets) => self.delete_entries(&targets),
        }
    }

    pub(super) fn save_name_dialog(&mut self) {
        let Some(ModalState::Name(mut state)) = self.modal.take() else {
            return;
        };
        let trimmed = state.input.trim();
        if trimmed.is_empty() {
            state.error = Some(String::from("name required"));
            self.modal = Some(ModalState::Name(state));
            return;
        }

        let result = match state.mode {
            NameDialogMode::NewFile => self.ensure_current_dir_exists().and_then(|_| {
                crate::fat32::create_file(&self.join_child_path(trimmed))
                    .map_err(|err| err.as_str())
            }),
            NameDialogMode::NewFolder => self.ensure_current_dir_exists().and_then(|_| {
                crate::fat32::create_dir(&self.join_child_path(trimmed)).map_err(|err| err.as_str())
            }),
            NameDialogMode::Rename(idx) => {
                crate::fat32::rename(&self.make_abs(idx), trimmed).map_err(|err| err.as_str())
            }
        };

        match result {
            Ok(()) => {
                let current = self.path.clone();
                self.modal = None;
                self.load_dir_with_state(&current, Some(trimmed), Some(self.offset));
                self.status_note = Some(String::from("saved changes"));
            }
            Err(err) => {
                state.error = Some(String::from(err));
                self.modal = Some(ModalState::Name(state));
            }
        }
    }

    pub(super) fn open_text_editor(&mut self, idx: usize) {
        let path = self.make_abs(idx);
        match crate::fat32::read_file(&path) {
            Some(bytes) => match core::str::from_utf8(&bytes) {
                Ok(text) => {
                    self.modal = Some(ModalState::TextEditor(TextEditorState {
                        entry_idx: idx,
                        path,
                        text: String::from(text),
                        cursor: text.chars().count(),
                        scroll_line: 0,
                        error: None,
                    }));
                }
                Err(_) => {
                    self.status_note = Some(String::from("file is not UTF-8 text"));
                }
            },
            None => {
                self.status_note = Some(String::from("file not found"));
            }
        }
    }

    pub(super) fn save_text_editor(&mut self) {
        let Some(ModalState::TextEditor(mut state)) = self.modal.take() else {
            return;
        };
        match crate::fat32::write_file(&state.path, state.text.as_bytes()) {
            Ok(()) => {
                let current = self.path.clone();
                let selected_name = self
                    .entries
                    .get(state.entry_idx)
                    .map(|entry| entry.name.clone());
                let selected_name_ref = selected_name.as_deref();
                self.modal = None;
                self.load_dir_with_state(&current, selected_name_ref, Some(self.offset));
                self.status_note = Some(String::from("text saved"));
            }
            Err(err) => {
                state.error = Some(String::from(err.as_str()));
                self.modal = Some(ModalState::TextEditor(state));
            }
        }
    }

    pub(super) fn sync_modal_state(&mut self) {
        let layout = self.layout();
        let editor_rect = Self::centered_rect(layout, EDITOR_W, EDITOR_H);
        let text_rect = self.editor_text_rect(editor_rect);
        let visible_lines = ((text_rect.h - 12).max(12) as usize) / 12;
        if let Some(ModalState::TextEditor(state)) = self.modal.as_mut() {
            let (cursor_line, _) = Self::text_cursor_line_col(&state.text, state.cursor);
            if cursor_line < state.scroll_line {
                state.scroll_line = cursor_line;
            } else if cursor_line >= state.scroll_line + visible_lines {
                state.scroll_line = cursor_line.saturating_sub(visible_lines.saturating_sub(1));
            }
        }
    }

    pub(super) fn text_lines(text: &str) -> Vec<String> {
        if text.is_empty() {
            return alloc::vec![String::new()];
        }
        text.split('\n').map(String::from).collect()
    }

    pub(super) fn text_cursor_line_col(text: &str, cursor: usize) -> (usize, usize) {
        let mut line = 0usize;
        let mut col = 0usize;
        for (idx, ch) in text.chars().enumerate() {
            if idx >= cursor {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    pub(super) fn text_cursor_from_line_col(
        text: &str,
        target_line: usize,
        target_col: usize,
    ) -> usize {
        let mut line = 0usize;
        let mut col = 0usize;
        let mut cursor = 0usize;
        for ch in text.chars() {
            if line == target_line && col == target_col {
                return cursor;
            }
            if ch == '\n' {
                if line == target_line {
                    return cursor;
                }
                line += 1;
                col = 0;
            } else if line == target_line {
                col += 1;
            }
            cursor += 1;
        }
        cursor
    }

    pub(super) fn char_to_byte_index(text: &str, char_idx: usize) -> usize {
        if char_idx == 0 {
            return 0;
        }
        text.char_indices()
            .nth(char_idx)
            .map(|(idx, _)| idx)
            .unwrap_or(text.len())
    }
}
