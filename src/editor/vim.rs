use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_textarea::{CursorMove, Input, TextArea};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    Normal,
    Insert,
    Visual,
}

impl std::fmt::Display for VimMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
        };
        f.write_str(label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCommand {
    None,
    Save,
    Close,
}

pub struct VimEditor {
    pub textarea: TextArea<'static>,
    pub mode: VimMode,
    key_buffer: String,
    dirty: bool,
    pub scroll: (u16, u16),
}

impl VimEditor {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let mut textarea: TextArea<'static> = if text.is_empty() {
            TextArea::default()
        } else {
            text.lines().map(ToOwned::to_owned).collect()
        };
        textarea.set_line_number_style(ratatui::style::Style::default());
        textarea.set_cursor_line_style(ratatui::style::Style::default());
        Self {
            textarea,
            mode: VimMode::Normal,
            key_buffer: String::new(),
            dirty: false,
            scroll: (0, 0),
        }
    }

    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn key_buffer(&self) -> &str {
        &self.key_buffer
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> EditorCommand {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            self.key_buffer.clear();
            return EditorCommand::Save;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            self.textarea.move_cursor(CursorMove::End);
            self.textarea.insert_newline();
            self.mode = VimMode::Insert;
            self.dirty = true;
            self.key_buffer.clear();
            return EditorCommand::None;
        }

        match self.mode {
            VimMode::Insert => self.handle_insert(key),
            VimMode::Normal => self.handle_normal(key),
            VimMode::Visual => self.handle_visual(key),
        }
    }

    fn handle_insert(&mut self, key: KeyEvent) -> EditorCommand {
        match key.code {
            KeyCode::Esc => {
                self.mode = VimMode::Normal;
                self.key_buffer.clear();
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
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
            if self.textarea.redo() {
                self.dirty = true;
            }
            self.key_buffer.clear();
            return EditorCommand::None;
        }

        match key.code {
            KeyCode::Esc => {
                self.key_buffer.clear();
                return EditorCommand::Close;
            }
            KeyCode::Char('i') => {
                self.mode = VimMode::Insert;
                self.key_buffer.clear();
            }
            KeyCode::Char('a') => {
                self.textarea.move_cursor(CursorMove::Forward);
                self.mode = VimMode::Insert;
                self.key_buffer.clear();
            }
            KeyCode::Char('A') => {
                self.textarea.move_cursor(CursorMove::End);
                self.mode = VimMode::Insert;
                self.key_buffer.clear();
            }
            KeyCode::Char('I') => {
                self.textarea.move_cursor(CursorMove::Head);
                self.mode = VimMode::Insert;
                self.key_buffer.clear();
            }
            KeyCode::Char('o') => {
                self.textarea.move_cursor(CursorMove::End);
                self.textarea.insert_newline();
                self.mode = VimMode::Insert;
                self.dirty = true;
                self.key_buffer.clear();
            }
            KeyCode::Char('O') => {
                self.textarea.move_cursor(CursorMove::Head);
                self.textarea.insert_newline();
                self.textarea.move_cursor(CursorMove::Up);
                self.mode = VimMode::Insert;
                self.dirty = true;
                self.key_buffer.clear();
            }
            KeyCode::Char('h') | KeyCode::Left => self.textarea.move_cursor(CursorMove::Back),
            KeyCode::Char('j') | KeyCode::Down => self.textarea.move_cursor(CursorMove::Down),
            KeyCode::Char('k') | KeyCode::Up => self.textarea.move_cursor(CursorMove::Up),
            KeyCode::Char('l') | KeyCode::Right => self.textarea.move_cursor(CursorMove::Forward),
            KeyCode::Char('w') => self.textarea.move_cursor(CursorMove::WordForward),
            KeyCode::Char('b') => self.textarea.move_cursor(CursorMove::WordBack),
            KeyCode::Char('0') | KeyCode::Home => self.textarea.move_cursor(CursorMove::Head),
            KeyCode::Char('$') | KeyCode::End => self.textarea.move_cursor(CursorMove::End),
            KeyCode::Char('G') => self.textarea.move_cursor(CursorMove::Bottom),
            KeyCode::Char('g') => {
                if self.key_buffer == "g" {
                    self.textarea.move_cursor(CursorMove::Top);
                    self.key_buffer.clear();
                } else {
                    self.key_buffer = "g".to_owned();
                }
                return EditorCommand::None;
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                if self.textarea.delete_next_char() {
                    self.dirty = true;
                }
                self.key_buffer.clear();
            }
            KeyCode::Char('u') => {
                if self.textarea.undo() {
                    self.dirty = true;
                }
                self.key_buffer.clear();
            }
            KeyCode::Char('v') => {
                self.textarea.start_selection();
                self.mode = VimMode::Visual;
                self.key_buffer.clear();
            }
            KeyCode::Char('p') => {
                if self.textarea.paste() {
                    self.dirty = true;
                }
                self.key_buffer.clear();
            }
            KeyCode::Char('d') => {
                if self.key_buffer == "d" {
                    self.select_current_line();
                    if self.textarea.cut() {
                        self.dirty = true;
                    }
                    self.key_buffer.clear();
                } else {
                    self.key_buffer = "d".to_owned();
                }
                return EditorCommand::None;
            }
            KeyCode::Char('y') => {
                if self.key_buffer == "y" {
                    self.select_current_line();
                    self.textarea.copy();
                    self.key_buffer.clear();
                } else {
                    self.key_buffer = "y".to_owned();
                }
                return EditorCommand::None;
            }
            _ => self.key_buffer.clear(),
        }

        if !matches!(
            key.code,
            KeyCode::Char('g') | KeyCode::Char('d') | KeyCode::Char('y')
        ) {
            self.key_buffer.clear();
        }
        EditorCommand::None
    }

    fn select_current_line(&mut self) {
        self.textarea.move_cursor(CursorMove::Head);
        self.textarea.start_selection();
        let cursor = self.textarea.cursor();
        self.textarea.move_cursor(CursorMove::Down);
        if cursor == self.textarea.cursor() {
            self.textarea.move_cursor(CursorMove::End);
        }
    }

    fn handle_visual(&mut self, key: KeyEvent) -> EditorCommand {
        match key.code {
            KeyCode::Esc => {
                self.textarea.cancel_selection();
                self.mode = VimMode::Normal;
            }
            KeyCode::Char('h') | KeyCode::Left => self.textarea.move_cursor(CursorMove::Back),
            KeyCode::Char('j') | KeyCode::Down => self.textarea.move_cursor(CursorMove::Down),
            KeyCode::Char('k') | KeyCode::Up => self.textarea.move_cursor(CursorMove::Up),
            KeyCode::Char('l') | KeyCode::Right => self.textarea.move_cursor(CursorMove::Forward),
            KeyCode::Char('w') => self.textarea.move_cursor(CursorMove::WordForward),
            KeyCode::Char('b') => self.textarea.move_cursor(CursorMove::WordBack),
            KeyCode::Char('0') | KeyCode::Home => self.textarea.move_cursor(CursorMove::Head),
            KeyCode::Char('$') | KeyCode::End => self.textarea.move_cursor(CursorMove::End),
            KeyCode::Char('y') => {
                self.textarea.move_cursor(CursorMove::Forward);
                self.textarea.copy();
                self.textarea.cancel_selection();
                self.mode = VimMode::Normal;
            }
            KeyCode::Char('d') | KeyCode::Char('x') => {
                self.textarea.move_cursor(CursorMove::Forward);
                if self.textarea.cut() {
                    self.dirty = true;
                }
                self.mode = VimMode::Normal;
            }
            _ => {}
        }
        self.key_buffer.clear();
        EditorCommand::None
    }
}
