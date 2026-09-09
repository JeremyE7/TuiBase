use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_textarea::{CursorMove, Input, TextArea};

use super::{
    SelectionRange,
    buffer::{char_len, char_slice, ordered_positions, selected_text, selection_range, split_text},
    commands::{Operator, VisualMode},
    registers::{RegisterBank, RegisterKind, RegisterValue},
    undo::{Snapshot, UndoManager},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    Normal,
    Insert,
}

impl std::fmt::Display for VimMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
        };
        f.write_str(label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorCommand {
    None,
    Save,
    ExecuteSelection,
    CopyToClipboard(String),
    PasteFromClipboard,
    Close,
}

pub struct VimEditor {
    pub textarea: TextArea<'static>,
    pub mode: VimMode,
    key_buffer: String,
    count_buffer: String,
    dirty: bool,
    pub scroll: (u16, u16),
    visual_anchor: Option<(usize, usize)>,
    visual_mode: VisualMode,
    pending_operator: Option<(Operator, usize, (usize, usize))>,
    registers: RegisterBank,
    undo: UndoManager,
    insert_snapshot: Option<Snapshot>,
}

impl VimEditor {
    pub fn new(text: impl Into<String>) -> Self {
        let mut textarea: TextArea<'static> = TextArea::from(split_text(&text.into()));
        Self::configure_textarea(&mut textarea);
        Self {
            textarea,
            mode: VimMode::Normal,
            key_buffer: String::new(),
            count_buffer: String::new(),
            dirty: false,
            scroll: (0, 0),
            visual_anchor: None,
            visual_mode: VisualMode::Character,
            pending_operator: None,
            registers: RegisterBank::default(),
            undo: UndoManager::default(),
            insert_snapshot: None,
        }
    }

    fn configure_textarea(textarea: &mut TextArea<'static>) {
        textarea.set_line_number_style(ratatui::style::Style::default());
        textarea.set_cursor_line_style(ratatui::style::Style::default());
    }

    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_clean(&mut self) {
        self.finish_insert_change();
        self.dirty = false;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn restore_state(&mut self, cursor: (usize, usize), scroll: (u16, u16), dirty: bool) {
        self.mode = VimMode::Normal;
        self.visual_anchor = None;
        self.textarea.cancel_selection();
        self.clear_pending();
        self.insert_snapshot = None;

        let row = cursor.0.min(self.textarea.lines().len().saturating_sub(1));
        let column = cursor.1.min(char_len(&self.textarea.lines()[row]));
        self.textarea.move_cursor(CursorMove::Jump(
            row.min(u16::MAX as usize) as u16,
            column.min(u16::MAX as usize) as u16,
        ));
        self.scroll = scroll;
        self.dirty = dirty;
    }

    pub fn has_visual_selection(&self) -> bool {
        self.visual_anchor.is_some()
    }

    pub fn paste_external_text(&mut self, text: &str) {
        if self.mode == VimMode::Insert {
            if self.textarea.insert_str(text) {
                self.dirty = true;
            }
        } else {
            self.with_change(|editor| {
                editor.textarea.insert_str(text);
            });
        }
    }

    pub fn visual_mode(&self) -> VisualMode {
        self.visual_mode
    }

    pub fn selection_for_render(&self) -> Option<SelectionRange> {
        let anchor = self.visual_anchor?;
        Some(selection_range(
            self.textarea.lines(),
            anchor,
            self.cursor_position(),
            self.visual_mode,
        ))
    }

    pub fn selected_text(&self) -> Option<String> {
        let anchor = self.visual_anchor?;
        Some(selected_text(
            self.textarea.lines(),
            anchor,
            self.cursor_position(),
            self.visual_mode,
        ))
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> EditorCommand {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        if ctrl && key.code == KeyCode::Char('s') {
            self.finish_insert_change();
            self.clear_pending();
            return EditorCommand::Save;
        }
        if ctrl && key.code == KeyCode::Enter {
            self.finish_insert_change();
            self.clear_pending();
            return EditorCommand::ExecuteSelection;
        }
        if ctrl && key.code == KeyCode::Char('a') {
            self.finish_insert_change();
            self.select_all();
            return EditorCommand::None;
        }
        if ctrl && key.code == KeyCode::Char('v') {
            if self.mode == VimMode::Insert {
                return EditorCommand::PasteFromClipboard;
            }
            if self.has_visual_selection() {
                self.visual_mode = if self.visual_mode == VisualMode::Block {
                    VisualMode::Character
                } else {
                    VisualMode::Block
                };
            } else {
                self.start_visual(VisualMode::Block);
            }
            return EditorCommand::None;
        }
        if ctrl && key.code == KeyCode::Char('c') && self.has_visual_selection() {
            return self.yank_visual();
        }

        if self.has_visual_selection() {
            return self.handle_visual(key);
        }

        match self.mode {
            VimMode::Insert => self.handle_insert(key),
            VimMode::Normal => self.handle_normal(key),
        }
    }

    fn handle_insert(&mut self, key: KeyEvent) -> EditorCommand {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            self.textarea.move_cursor(CursorMove::End);
            self.textarea.insert_newline();
            self.dirty = true;
            return EditorCommand::None;
        }

        match key.code {
            KeyCode::Esc => {
                self.finish_insert_change();
                self.mode = VimMode::Normal;
                self.clear_pending();
            }
            KeyCode::Char('w')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.textarea.delete_word();
                self.dirty = true;
            }
            KeyCode::Char('u')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.textarea.delete_line_by_head();
                self.dirty = true;
            }
            _ => {
                let input: Input = key.into();
                if self.textarea.input(input) {
                    self.dirty = true;
                }
            }
        }
        EditorCommand::None
    }

    fn handle_normal(&mut self, key: KeyEvent) -> EditorCommand {
        if let Some((operator, operator_count, anchor)) = self.pending_operator {
            return self.handle_pending_operator(key, operator, operator_count, anchor);
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
            self.finish_insert_change();
            if let Some(snapshot) = self.undo.redo(self.snapshot()) {
                self.restore_snapshot(snapshot);
                self.dirty = true;
            }
            self.clear_pending();
            return EditorCommand::None;
        }

        if key.code == KeyCode::Esc {
            self.clear_pending();
            return EditorCommand::None;
        }

        if key.code == KeyCode::Char('q') && key.modifiers.is_empty() {
            self.clear_pending();
            return EditorCommand::Close;
        }

        if self.is_count_key(key) {
            self.count_buffer.push(key_char(key));
            return EditorCommand::None;
        }

        if self.key_buffer == "g" {
            if key.code == KeyCode::Char('g') {
                let count = self.take_count().unwrap_or(1);
                if count == 1 {
                    self.textarea.move_cursor(CursorMove::Top);
                } else {
                    self.textarea.move_cursor(CursorMove::Jump(
                        count.saturating_sub(1).min(u16::MAX as usize) as u16,
                        0,
                    ));
                }
            }
            self.key_buffer.clear();
            return EditorCommand::None;
        }

        match key.code {
            KeyCode::Char('g') => {
                self.key_buffer = "g".to_owned();
            }
            KeyCode::Char('i') => self.enter_insert(),
            KeyCode::Char('a') => {
                self.textarea.move_cursor(CursorMove::Forward);
                self.enter_insert();
            }
            KeyCode::Char('A') => {
                self.textarea.move_cursor(CursorMove::End);
                self.enter_insert();
            }
            KeyCode::Char('I') => {
                self.move_to_first_non_blank();
                self.enter_insert();
            }
            KeyCode::Char('o') => {
                self.enter_insert();
                self.textarea.move_cursor(CursorMove::End);
                self.textarea.insert_newline();
                self.dirty = true;
            }
            KeyCode::Char('O') => {
                self.enter_insert();
                self.textarea.move_cursor(CursorMove::Head);
                self.textarea.insert_newline();
                self.textarea.move_cursor(CursorMove::Up);
                self.dirty = true;
            }
            KeyCode::Char('h') | KeyCode::Left => self.move_motion(CursorMove::Back),
            KeyCode::Char('j') | KeyCode::Down => self.move_motion(CursorMove::Down),
            KeyCode::Char('k') | KeyCode::Up => self.move_motion(CursorMove::Up),
            KeyCode::Char('l') | KeyCode::Right => self.move_motion(CursorMove::Forward),
            KeyCode::Char('w') | KeyCode::Char('W') => self.move_motion(CursorMove::WordForward),
            KeyCode::Char('b') | KeyCode::Char('B') => self.move_motion(CursorMove::WordBack),
            KeyCode::Char('e') | KeyCode::Char('E') => self.move_motion(CursorMove::WordEnd),
            KeyCode::Char('0') | KeyCode::Home => {
                self.textarea.move_cursor(CursorMove::Head);
                self.take_count();
            }
            KeyCode::Char('^') => {
                self.move_to_first_non_blank();
                self.take_count();
            }
            KeyCode::Char('$') | KeyCode::End => {
                self.textarea.move_cursor(CursorMove::End);
                self.take_count();
            }
            KeyCode::Char('G') => {
                if let Some(count) = self.take_count() {
                    self.textarea.move_cursor(CursorMove::Jump(
                        count.saturating_sub(1).min(u16::MAX as usize) as u16,
                        0,
                    ));
                } else {
                    self.textarea.move_cursor(CursorMove::Bottom);
                }
            }
            KeyCode::PageUp => self.move_vertical(10, false),
            KeyCode::PageDown => self.move_vertical(10, true),
            KeyCode::Char('x') | KeyCode::Delete => {
                let count = self.take_count().unwrap_or(1);
                self.with_change(|editor| {
                    for _ in 0..count {
                        editor.textarea.delete_next_char();
                    }
                });
            }
            KeyCode::Char('X') => {
                let count = self.take_count().unwrap_or(1);
                self.with_change(|editor| {
                    for _ in 0..count {
                        editor.textarea.delete_char();
                    }
                });
            }
            KeyCode::Char('d') => self.begin_operator(Operator::Delete),
            KeyCode::Char('c') => self.begin_operator(Operator::Change),
            KeyCode::Char('y') => self.begin_operator(Operator::Yank),
            KeyCode::Char('Y') => {
                let count = self.take_count().unwrap_or(1);
                return self.yank_current_lines(count);
            }
            KeyCode::Char('p') => return self.paste(false),
            KeyCode::Char('P') => return self.paste(true),
            KeyCode::Char('u') => {
                self.finish_insert_change();
                if let Some(snapshot) = self.undo.undo(self.snapshot()) {
                    self.restore_snapshot(snapshot);
                    self.dirty = true;
                }
                self.clear_pending();
            }
            KeyCode::Char('v') => self.start_visual(VisualMode::Character),
            KeyCode::Char('V') => self.start_visual(VisualMode::Line),
            KeyCode::Char('J') => self.join_line(),
            _ => self.clear_pending(),
        }

        EditorCommand::None
    }

    fn handle_pending_operator(
        &mut self,
        key: KeyEvent,
        operator: Operator,
        operator_count: usize,
        anchor: (usize, usize),
    ) -> EditorCommand {
        if key.code == KeyCode::Esc {
            self.clear_pending();
            return EditorCommand::None;
        }

        if key.code == operator_key(operator) {
            let count = operator_count
                .saturating_mul(self.take_count().unwrap_or(1))
                .max(1);
            self.clear_pending();
            return self.operate_lines(operator, anchor.0, count);
        }

        let motion_count = self.take_count().unwrap_or(1);
        let before = self.textarea.cursor();
        let supported = self.apply_motion_key(key, motion_count);
        if !supported {
            self.clear_pending();
            self.textarea.move_cursor(CursorMove::Jump(
                before.0.min(u16::MAX as usize) as u16,
                before.1.min(u16::MAX as usize) as u16,
            ));
            return EditorCommand::None;
        }

        let cursor = self.cursor_position();
        let count = operator_count.max(1);
        self.clear_pending();
        self.operate_characterwise(operator, anchor, cursor, count)
    }

    fn handle_visual(&mut self, key: KeyEvent) -> EditorCommand {
        if key.code == KeyCode::Esc {
            self.cancel_visual();
            return EditorCommand::None;
        }

        if key.code == KeyCode::Char('q') && key.modifiers.is_empty() {
            self.cancel_visual();
            return EditorCommand::Close;
        }

        if self.is_count_key(key) {
            self.count_buffer.push(key_char(key));
            return EditorCommand::None;
        }

        match key.code {
            KeyCode::Char('v') if self.visual_mode == VisualMode::Character => {
                self.cancel_visual();
            }
            KeyCode::Char('V') => {
                self.visual_mode = if self.visual_mode == VisualMode::Line {
                    VisualMode::Character
                } else {
                    VisualMode::Line
                };
            }
            KeyCode::Char('h') | KeyCode::Left => self.move_visual(CursorMove::Back),
            KeyCode::Char('j') | KeyCode::Down => self.move_visual(CursorMove::Down),
            KeyCode::Char('k') | KeyCode::Up => self.move_visual(CursorMove::Up),
            KeyCode::Char('l') | KeyCode::Right => self.move_visual(CursorMove::Forward),
            KeyCode::Char('w') | KeyCode::Char('W') => self.move_visual(CursorMove::WordForward),
            KeyCode::Char('b') | KeyCode::Char('B') => self.move_visual(CursorMove::WordBack),
            KeyCode::Char('e') | KeyCode::Char('E') => self.move_visual(CursorMove::WordEnd),
            KeyCode::Char('0') | KeyCode::Home => self.move_visual(CursorMove::Head),
            KeyCode::Char('^') => self.move_visual_first_non_blank(),
            KeyCode::Char('$') | KeyCode::End => self.move_visual(CursorMove::End),
            KeyCode::Char('y') => return self.yank_visual(),
            KeyCode::Char('d') | KeyCode::Char('x') => {
                let kind = self.register_kind();
                let text = self.selected_text().unwrap_or_default();
                self.registers.set(text, kind);
                self.delete_visual();
                self.cancel_visual();
            }
            KeyCode::Char('c') => {
                let kind = self.register_kind();
                let text = self.selected_text().unwrap_or_default();
                self.registers.set(text, kind);
                self.delete_visual();
                self.cancel_visual();
                self.enter_insert();
            }
            KeyCode::Char('I') | KeyCode::Char('A') if self.visual_mode == VisualMode::Block => {
                self.enter_insert();
            }
            _ => self.clear_pending(),
        }
        self.take_count();
        EditorCommand::None
    }

    fn begin_operator(&mut self, operator: Operator) {
        let count = self.take_count().unwrap_or(1);
        self.pending_operator = Some((operator, count, self.cursor_position()));
        self.key_buffer = match operator {
            Operator::Delete => "d".to_owned(),
            Operator::Change => "c".to_owned(),
            Operator::Yank => "y".to_owned(),
        };
    }

    fn operate_lines(&mut self, operator: Operator, row: usize, count: usize) -> EditorCommand {
        let start = row.min(self.textarea.lines().len().saturating_sub(1));
        let end =
            (start + count.saturating_sub(1)).min(self.textarea.lines().len().saturating_sub(1));
        let text = self.textarea.lines()[start..=end].join("\n");
        self.registers.set(text.clone(), RegisterKind::Linewise);

        match operator {
            Operator::Yank => EditorCommand::CopyToClipboard(text),
            Operator::Delete | Operator::Change => {
                self.with_change(|editor| {
                    let mut lines = editor.textarea.lines().to_vec();
                    lines.drain(start..=end);
                    if lines.is_empty() {
                        lines.push(String::new());
                    }
                    editor.replace_lines(lines, (start.min(editor.textarea.lines().len()), 0));
                });
                if operator == Operator::Change {
                    self.enter_insert();
                }
                EditorCommand::None
            }
        }
    }

    fn operate_characterwise(
        &mut self,
        operator: Operator,
        anchor: (usize, usize),
        cursor: (usize, usize),
        _count: usize,
    ) -> EditorCommand {
        let mode = VisualMode::Character;
        let text = selected_text(self.textarea.lines(), anchor, cursor, mode);
        self.registers
            .set(text.clone(), RegisterKind::Characterwise);
        if operator == Operator::Yank {
            return EditorCommand::CopyToClipboard(text);
        }

        self.with_change(|editor| editor.remove_range(anchor, cursor));
        if operator == Operator::Change {
            self.enter_insert();
        }
        EditorCommand::None
    }

    fn yank_current_lines(&mut self, count: usize) -> EditorCommand {
        let row = self.textarea.cursor().0;
        self.registers.set(
            self.textarea.lines()[row..=(row + count.saturating_sub(1))
                .min(self.textarea.lines().len().saturating_sub(1))]
                .join("\n"),
            RegisterKind::Linewise,
        );
        EditorCommand::CopyToClipboard(
            self.registers
                .unnamed()
                .map(|register| register.text.clone())
                .unwrap_or_default(),
        )
    }

    fn yank_visual(&mut self) -> EditorCommand {
        let text = self.selected_text().unwrap_or_default();
        self.registers.set(text.clone(), self.register_kind());
        self.cancel_visual();
        EditorCommand::CopyToClipboard(text)
    }

    fn paste(&mut self, before: bool) -> EditorCommand {
        let Some(register) = self.registers.unnamed().cloned() else {
            return EditorCommand::PasteFromClipboard;
        };
        self.with_change(|editor| editor.insert_register(&register, before));
        EditorCommand::None
    }

    fn insert_register(&mut self, register: &RegisterValue, before: bool) {
        let cursor = self.cursor_position();
        let mut lines = self.textarea.lines().to_vec();
        match register.kind {
            RegisterKind::Characterwise => {
                let row = cursor.0;
                let column = if before {
                    cursor.1
                } else {
                    (cursor.1 + 1).min(char_len(&lines[row]))
                };
                let line = &lines[row];
                let byte = super::buffer::char_to_byte(line, column);
                lines[row].insert_str(byte, &register.text);
                self.replace_lines(lines, (row, column + register.text.chars().count()));
            }
            RegisterKind::Linewise => {
                let insert_at = if before { cursor.0 } else { cursor.0 + 1 };
                let additions = split_text(&register.text);
                let insert_at = insert_at.min(lines.len());
                lines.splice(insert_at..insert_at, additions.clone());
                self.replace_lines(lines, (insert_at, 0));
            }
            RegisterKind::Blockwise => {
                let additions = split_text(&register.text);
                let row = cursor.0;
                let column = cursor.1;
                for (offset, addition) in additions.iter().enumerate() {
                    let target_row = row + offset;
                    if target_row >= lines.len() {
                        lines.push(String::new());
                    }
                    let byte = super::buffer::char_to_byte(&lines[target_row], column);
                    lines[target_row].insert_str(byte, addition);
                }
                self.replace_lines(lines, (row, column));
            }
        }
    }

    fn delete_visual(&mut self) {
        let Some(anchor) = self.visual_anchor else {
            return;
        };
        let cursor = self.cursor_position();
        self.with_change(|editor| match editor.visual_mode {
            VisualMode::Line => {
                let ((start_row, _), (end_row, _)) = ordered_positions(anchor, cursor);
                let mut lines = editor.textarea.lines().to_vec();
                lines.drain(start_row..=end_row);
                if lines.is_empty() {
                    lines.push(String::new());
                }
                editor.replace_lines(lines, (start_row.min(editor.textarea.lines().len()), 0));
            }
            VisualMode::Character => editor.remove_range(anchor, cursor),
            VisualMode::Block => editor.remove_block_range(anchor, cursor),
        });
    }

    fn remove_block_range(&mut self, anchor: (usize, usize), cursor: (usize, usize)) {
        let start_row = anchor.0.min(cursor.0);
        let end_row = anchor.0.max(cursor.0);
        let start_column = anchor.1.min(cursor.1);
        let end_column = anchor.1.max(cursor.1).saturating_add(1);
        let mut lines = self.textarea.lines().to_vec();

        for row in start_row..=end_row.min(lines.len().saturating_sub(1)) {
            let line_length = char_len(&lines[row]);
            let start = start_column.min(line_length);
            let end = end_column.min(line_length);
            if start >= end {
                continue;
            }
            let start_byte = super::buffer::char_to_byte(&lines[row], start);
            let end_byte = super::buffer::char_to_byte(&lines[row], end);
            lines[row].replace_range(start_byte..end_byte, "");
        }
        self.replace_lines(lines, (start_row, start_column));
    }

    fn remove_range(&mut self, anchor: (usize, usize), cursor: (usize, usize)) {
        let range = selection_range(self.textarea.lines(), anchor, cursor, VisualMode::Character);
        let mut lines = self.textarea.lines().to_vec();
        let (start, end) = (range.start, range.end);
        if start.0 == end.0 {
            let line = &mut lines[start.0];
            let start_byte = super::buffer::char_to_byte(line, start.1);
            let end_byte = super::buffer::char_to_byte(line, end.1);
            line.replace_range(start_byte..end_byte, "");
            self.replace_lines(lines, start);
            return;
        }

        let prefix = char_slice(&lines[start.0], 0, start.1);
        let suffix = char_slice(&lines[end.0], end.1, char_len(&lines[end.0]));
        let mut merged = prefix;
        merged.push_str(&suffix);
        lines.splice(start.0..=end.0, [merged]);
        self.replace_lines(lines, start);
    }

    fn start_visual(&mut self, mode: VisualMode) {
        self.finish_insert_change();
        self.mode = VimMode::Normal;
        self.visual_mode = mode;
        self.visual_anchor = Some(self.cursor_position());
        self.textarea.cancel_selection();
        self.clear_pending_except_mode();
    }

    fn select_all(&mut self) {
        self.mode = VimMode::Normal;
        self.visual_mode = VisualMode::Character;
        self.visual_anchor = Some((0, 0));
        self.textarea
            .move_cursor(CursorMove::Jump(u16::MAX, u16::MAX));
        self.textarea.cancel_selection();
        self.clear_pending_except_mode();
    }

    fn cancel_visual(&mut self) {
        self.visual_anchor = None;
        self.textarea.cancel_selection();
        self.mode = VimMode::Normal;
        self.clear_pending();
    }

    fn move_visual(&mut self, movement: CursorMove) {
        let count = self.take_count().unwrap_or(1);
        for _ in 0..count {
            self.textarea.move_cursor(movement);
        }
    }

    fn move_visual_first_non_blank(&mut self) {
        self.move_to_first_non_blank();
    }

    fn move_motion(&mut self, movement: CursorMove) {
        let count = self.take_count().unwrap_or(1);
        for _ in 0..count {
            self.textarea.move_cursor(movement);
        }
    }

    fn move_vertical(&mut self, amount: usize, down: bool) {
        let count = self.take_count().unwrap_or(1).saturating_mul(amount);
        for _ in 0..count {
            self.textarea.move_cursor(if down {
                CursorMove::Down
            } else {
                CursorMove::Up
            });
        }
    }

    fn apply_motion_key(&mut self, key: KeyEvent, count: usize) -> bool {
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => self.move_motion_count(CursorMove::Back, count),
            KeyCode::Char('j') | KeyCode::Down => self.move_motion_count(CursorMove::Down, count),
            KeyCode::Char('k') | KeyCode::Up => self.move_motion_count(CursorMove::Up, count),
            KeyCode::Char('l') | KeyCode::Right => {
                self.move_motion_count(CursorMove::Forward, count)
            }
            KeyCode::Char('w') | KeyCode::Char('W') => {
                self.move_motion_count(CursorMove::WordForward, count)
            }
            KeyCode::Char('b') | KeyCode::Char('B') => {
                self.move_motion_count(CursorMove::WordBack, count)
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                self.move_motion_count(CursorMove::WordEnd, count)
            }
            KeyCode::Char('0') | KeyCode::Home => {
                self.textarea.move_cursor(CursorMove::Head);
                true
            }
            KeyCode::Char('^') => {
                self.move_to_first_non_blank();
                true
            }
            KeyCode::Char('$') | KeyCode::End => {
                self.textarea.move_cursor(CursorMove::End);
                true
            }
            _ => false,
        }
    }

    fn move_motion_count(&mut self, movement: CursorMove, count: usize) -> bool {
        for _ in 0..count {
            self.textarea.move_cursor(movement);
        }
        true
    }

    fn move_to_first_non_blank(&mut self) {
        let (row, _) = self.cursor_position();
        let column = self.textarea.lines()[row]
            .chars()
            .position(|character| !character.is_whitespace())
            .unwrap_or(0);
        self.textarea.move_cursor(CursorMove::Jump(
            row.min(u16::MAX as usize) as u16,
            column.min(u16::MAX as usize) as u16,
        ));
    }

    fn join_line(&mut self) {
        let (row, _) = self.cursor_position();
        if row + 1 >= self.textarea.lines().len() {
            return;
        }
        self.with_change(|editor| {
            let mut lines = editor.textarea.lines().to_vec();
            let next = lines.remove(row + 1);
            if !lines[row].is_empty() && !next.is_empty() {
                lines[row].push(' ');
            }
            lines[row].push_str(&next);
            let column = char_len(&lines[row]);
            editor.replace_lines(lines, (row, column));
        });
    }

    fn register_kind(&self) -> RegisterKind {
        match self.visual_mode {
            VisualMode::Character => RegisterKind::Characterwise,
            VisualMode::Line => RegisterKind::Linewise,
            VisualMode::Block => RegisterKind::Blockwise,
        }
    }

    fn replace_lines(&mut self, lines: Vec<String>, cursor: (usize, usize)) {
        let mut textarea = TextArea::from(if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        });
        Self::configure_textarea(&mut textarea);
        let row = cursor.0.min(textarea.lines().len().saturating_sub(1));
        let column = cursor.1.min(char_len(&textarea.lines()[row]));
        textarea.move_cursor(CursorMove::Jump(
            row.min(u16::MAX as usize) as u16,
            column.min(u16::MAX as usize) as u16,
        ));
        self.textarea = textarea;
    }

    fn with_change<F>(&mut self, action: F)
    where
        F: FnOnce(&mut Self),
    {
        let before = self.snapshot();
        action(self);
        if self.text() != before.text {
            self.undo.record(before, &self.text());
            self.dirty = true;
        }
    }

    fn enter_insert(&mut self) {
        self.finish_insert_change();
        self.visual_anchor = None;
        self.textarea.cancel_selection();
        self.insert_snapshot = Some(self.snapshot());
        self.mode = VimMode::Insert;
        self.clear_pending_except_mode();
    }

    fn finish_insert_change(&mut self) {
        let Some(before) = self.insert_snapshot.take() else {
            return;
        };
        if self.text() != before.text {
            self.undo.record(before, &self.text());
            self.dirty = true;
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text(),
            cursor: self.cursor_position(),
        }
    }

    pub fn cursor_position(&self) -> (usize, usize) {
        let cursor = self.textarea.cursor();
        (cursor.0, cursor.1)
    }

    fn restore_snapshot(&mut self, snapshot: Snapshot) {
        self.replace_lines(split_text(&snapshot.text), snapshot.cursor);
        self.visual_anchor = None;
        self.textarea.cancel_selection();
        self.clear_pending();
    }

    fn is_count_key(&self, key: KeyEvent) -> bool {
        matches!(key.code, KeyCode::Char(character) if character.is_ascii_digit())
            && (key.code != KeyCode::Char('0') || !self.count_buffer.is_empty())
    }

    fn take_count(&mut self) -> Option<usize> {
        if self.count_buffer.is_empty() {
            return None;
        }
        let count = self.count_buffer.parse::<usize>().unwrap_or(1).max(1);
        self.count_buffer.clear();
        Some(count)
    }

    fn clear_pending(&mut self) {
        self.key_buffer.clear();
        self.count_buffer.clear();
        self.pending_operator = None;
    }

    fn clear_pending_except_mode(&mut self) {
        self.key_buffer.clear();
        self.count_buffer.clear();
        self.pending_operator = None;
    }
}

fn key_char(key: KeyEvent) -> char {
    match key.code {
        KeyCode::Char(character) => character,
        _ => '0',
    }
}

fn operator_key(operator: Operator) -> KeyCode {
    match operator {
        Operator::Delete => KeyCode::Char('d'),
        Operator::Change => KeyCode::Char('c'),
        Operator::Yank => KeyCode::Char('y'),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{EditorCommand, VimEditor, VimMode};
    use crate::editor::VisualMode;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    fn ctrl(code: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(code), KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_a_selects_the_complete_buffer() {
        let mut editor = VimEditor::new("select\nsecond");
        editor.handle_key(ctrl('a'));

        assert_eq!(editor.mode, VimMode::Normal);
        assert_eq!(editor.visual_mode(), VisualMode::Character);
        assert_eq!(editor.selected_text().as_deref(), Some("select\nsecond"));
    }

    #[test]
    fn visual_y_returns_a_clipboard_command_and_exits_visual() {
        let mut editor = VimEditor::new("select");
        editor.handle_key(key(KeyCode::Char('v')));
        editor.handle_key(key(KeyCode::Char('l')));

        assert_eq!(
            editor.handle_key(key(KeyCode::Char('y'))),
            EditorCommand::CopyToClipboard("se".to_owned())
        );
        assert_eq!(editor.mode, VimMode::Normal);
    }

    #[test]
    fn linewise_visual_yanks_complete_lines() {
        let mut editor = VimEditor::new("one\ntwo\nthree");
        editor.handle_key(key(KeyCode::Char('V')));
        editor.handle_key(key(KeyCode::Char('j')));

        assert_eq!(
            editor.handle_key(key(KeyCode::Char('y'))),
            EditorCommand::CopyToClipboard("one\ntwo".to_owned())
        );
    }

    #[test]
    fn dd_deletes_a_line_and_undo_restores_it() {
        let mut editor = VimEditor::new("one\ntwo");
        editor.handle_key(key(KeyCode::Char('d')));
        editor.handle_key(key(KeyCode::Char('d')));
        assert_eq!(editor.text(), "two");

        editor.handle_key(key(KeyCode::Char('u')));
        assert_eq!(editor.text(), "one\ntwo");
    }

    #[test]
    fn trailing_newline_is_preserved() {
        let editor = VimEditor::new("select\n");
        assert_eq!(editor.text(), "select\n");
    }

    #[test]
    fn normal_escape_stays_in_normal_mode() {
        let mut editor = VimEditor::new("select");
        assert_eq!(editor.handle_key(key(KeyCode::Esc)), EditorCommand::None);
        assert_eq!(editor.mode, VimMode::Normal);
    }

    #[test]
    fn normal_q_requests_close() {
        let mut editor = VimEditor::new("select");
        assert_eq!(
            editor.handle_key(key(KeyCode::Char('q'))),
            EditorCommand::Close
        );
    }

    #[test]
    fn normal_p_requests_clipboard_when_register_is_empty() {
        let mut editor = VimEditor::new("select");
        assert_eq!(
            editor.handle_key(key(KeyCode::Char('p'))),
            EditorCommand::PasteFromClipboard
        );

        let mut editor = VimEditor::new("select");
        assert_eq!(
            editor.handle_key(key(KeyCode::Char('P'))),
            EditorCommand::PasteFromClipboard
        );
    }

    #[test]
    fn visual_q_requests_close_and_cancels_selection() {
        let mut editor = VimEditor::new("select");
        editor.handle_key(key(KeyCode::Char('v')));

        assert_eq!(
            editor.handle_key(key(KeyCode::Char('q'))),
            EditorCommand::Close
        );
        assert!(!editor.has_visual_selection());
        assert_eq!(editor.mode, VimMode::Normal);
    }

    #[test]
    fn insert_escape_returns_to_normal_without_closing() {
        let mut editor = VimEditor::new("select");
        editor.handle_key(key(KeyCode::Char('i')));

        assert_eq!(editor.handle_key(key(KeyCode::Esc)), EditorCommand::None);
        assert_eq!(editor.mode, VimMode::Normal);
    }

    #[test]
    fn restore_state_restores_cursor_scroll_and_dirty_state() {
        let mut editor = VimEditor::new("first\nsecond");

        editor.restore_state((1, 3), (4, 5), true);

        assert_eq!(editor.cursor_position(), (1, 3));
        assert_eq!(editor.scroll, (4, 5));
        assert!(editor.is_dirty());
        assert_eq!(editor.mode, VimMode::Normal);
    }

    #[test]
    fn ctrl_l_in_insert_appends_a_line() {
        let mut editor = VimEditor::new("select");
        editor.handle_key(key(KeyCode::Char('i')));
        editor.handle_key(ctrl('l'));

        assert_eq!(editor.mode, VimMode::Insert);
        assert_eq!(editor.text(), "select\n");
    }

    #[test]
    fn blockwise_delete_removes_the_same_columns_from_each_line() {
        let mut editor = VimEditor::new("abcd\nWXYZ");
        editor.handle_key(ctrl('v'));
        editor.handle_key(key(KeyCode::Char('l')));
        editor.handle_key(key(KeyCode::Char('j')));
        editor.handle_key(key(KeyCode::Char('d')));

        assert_eq!(editor.text(), "cd\nYZ");
        assert_eq!(editor.mode, VimMode::Normal);
    }
}
