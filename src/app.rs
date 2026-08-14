use std::{
    collections::{BTreeSet, HashSet},
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::catalog::{CatalogCache, CatalogEntry, SearchCatalogEntry, connection_key};
use crate::db::models::{ColumnMetadata, TableMetadata, TablePage};
use crate::services;
use crate::table_preferences::{TablePreferences, table_preference_key};
use crate::ui;
use crossbeam_channel::{Receiver, Sender};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Text;
use ratatui::widgets::{ScrollbarState, TableState};
use ratatui_textarea::{CursorMove, Input, TextArea};

use crate::{
    config::{AppConfig, ConnectionProfile},
    db::{
        models::{DbObject, ObjectKind, SqlOutput},
        query::{
            FilterOperator, FilterSpec, PageRequest, SortDirection, SortSpec, TableQuery,
            parse_filter_expression, parse_sort_expression,
        },
        sybase,
    },
    editor::{EditorCommand, VimEditor},
    worker::{WorkerRequest, WorkerResponse},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Browser,
    Editor,
    Confirm,
    Help,
    Search,
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableCopySource {
    LoadedData,
    CurrentCell,
    CurrentRow,
    CurrentColumn,
    VisualSelection,
    VisualRows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableCopyStage {
    Menu,
    HeaderChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Connections,
    Databases,
    Kinds,
    Objects,
    Content,
}

pub struct SearchSession {
    pub input: TextArea<'static>,
    pub suggestions: Vec<CatalogEntry>,
    pub selected_suggestion: usize,
    refresh_deadline: Option<Instant>,
}

#[derive(Debug, Clone)]
pub struct FilterSuggestion {
    pub label: String,
    insertion: String,
    replace_start: usize,
}

pub struct FilterSession {
    pub input: TextArea<'static>,
    pub suggestions: Vec<FilterSuggestion>,
    pub selected_suggestion: usize,
    pub parse_error: Option<String>,
    pub preview: Vec<String>,
}

pub struct SortSession {
    pub input: TextArea<'static>,
    pub suggestions: Vec<FilterSuggestion>,
    pub selected_suggestion: usize,
    pub parse_error: Option<String>,
    pub preview: Vec<String>,
}

pub struct ColumnSearchSession {
    pub input: TextArea<'static>,
    pub suggestions: Vec<String>,
    pub selected_suggestion: usize,
}

pub struct TableValueModal {
    pub column: String,
    pub data_type: String,
    pub row_number: usize,
    pub value: String,
}

pub struct TableCellEditorSession {
    pub column: String,
    pub data_type: String,
    pub row_number: usize,
    pub nullable: bool,
    pub original_value: String,
    pub input: TextArea<'static>,
    row_index: usize,
    metadata: ColumnMetadata,
}

pub struct TableDateTimePickerSession {
    pub column: String,
    pub data_type: String,
    pub row_number: usize,
    pub nullable: bool,
    pub show_date: bool,
    pub show_time: bool,
    pub is_null: bool,
    pub selected_component: usize,
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub fractional_seconds: String,
    original_value: String,
    row_index: usize,
    metadata: ColumnMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCellDraft {
    pub row_index: usize,
    pub column: String,
    pub original_value: String,
    pub value: String,
}

pub struct TableChangesSummary {
    pub edited_cells: Vec<String>,
    pub deleted_rows: Vec<usize>,
    pub new_row_count: usize,
    pub identity_columns: Vec<String>,
    pub identity_warning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableDateTimeKind {
    Date,
    DateTime,
    Time,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedTableDateTime {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    fractional_seconds: String,
    is_null: bool,
}

impl Focus {
    fn next(self) -> Self {
        match self {
            Self::Connections => Self::Databases,
            Self::Databases => Self::Kinds,
            Self::Kinds => Self::Objects,
            Self::Objects => Self::Content,
            Self::Content => Self::Connections,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Connections => Self::Content,
            Self::Databases => Self::Connections,
            Self::Kinds => Self::Databases,
            Self::Objects => Self::Kinds,
            Self::Content => Self::Objects,
        }
    }
}

#[derive(Debug, Clone)]
pub enum EditorPurpose {
    Query,
    ObjectDefinition(DbObject),
    TableValues(DbObject),
}

pub struct EditorSession {
    pub title: String,
    pub purpose: EditorPurpose,
    pub editor: VimEditor,
}

#[derive(Debug, Clone)]
enum ConfirmAction {
    Execute { sql: String },
    ExecuteTableChanges { sql: String },
    DiscardEditor,
    DiscardTableChanges,
}

pub struct App {
    pub config: AppConfig,
    pub config_path: PathBuf,
    pub mode: AppMode,
    pub focus: Focus,
    pub connection_index: usize,
    pub databases: Vec<String>,
    pub database_index: usize,
    pub kind_index: usize,
    pub objects: Vec<DbObject>,
    pub object_index: usize,
    pub content_title: String,
    pub content: String,
    pub highlighted_content: Option<Text<'static>>,
    pub content_scroll: u16,
    pub status: String,
    pub last_key: String,
    pub editor: Option<EditorSession>,
    pub should_quit: bool,
    pub busy_count: usize,
    pub table_page: Option<TablePage>,
    pub table_metadata: Option<TableMetadata>,
    pub table_query: TableQuery,
    pub table_filter_expression: String,
    pub table_loading_more: bool,
    pub table_state: TableState,
    pub horizontal_scroll: ScrollbarState,
    pub table_column_index: usize,
    pub table_visible_columns: usize,
    pub table_show_metadata: bool,
    pub table_visual_anchor: Option<(usize, usize)>,
    pub table_row_visual_anchor: Option<usize>,
    pub table_copy_stage: Option<TableCopyStage>,
    pub table_copy_source: Option<TableCopySource>,
    pub table_copy_menu_index: usize,
    pub table_value_modal: Option<TableValueModal>,
    pub table_value_scroll: u16,
    pub table_cell_editor: Option<TableCellEditorSession>,
    pub table_date_time_picker: Option<TableDateTimePickerSession>,
    pub table_cell_drafts: Vec<TableCellDraft>,
    pub table_row_deletions: BTreeSet<usize>,
    pub table_new_rows: BTreeSet<usize>,
    table_delete_sequence_armed: bool,
    pub table_changes_summary: Option<TableChangesSummary>,
    pub table_changes_summary_scroll: u16,
    pub table_sql_preview: Option<sybase::queries::TableSqlPreview>,
    pub table_sql_preview_scroll: u16,
    search_return_mode: AppMode,
    help_return_mode: AppMode,
    search: Option<SearchSession>,
    filter_session: Option<FilterSession>,
    sort_session: Option<SortSession>,
    column_search_session: Option<ColumnSearchSession>,
    active_search: Option<String>,
    catalog: CatalogCache,
    search_index: Vec<SearchCatalogEntry>,
    table_preferences: TablePreferences,
    catalog_refresh_pending: HashSet<usize>,
    pending_catalog_target: Option<CatalogEntry>,
    pending_table_navigation: Option<CatalogEntry>,
    last_catalog_refresh_check: Instant,
    request_tx: Sender<WorkerRequest>,
    response_rx: Receiver<WorkerResponse>,
    next_request_id: u64,
    pending_requests: HashSet<u64>,
    current_content_object: Option<DbObject>,
    open_editor_after_definition: Option<(u64, DbObject)>,
    confirm_action: Option<ConfirmAction>,
    confirm_message: String,
    return_to_editor_after_execution: bool,
    return_to_table_after_execution: bool,
}

impl App {
    pub fn new(
        config: AppConfig,
        config_path: PathBuf,
        request_tx: Sender<WorkerRequest>,
        response_rx: Receiver<WorkerResponse>,
    ) -> Self {
        let catalog = CatalogCache::load().unwrap_or_default();
        let search_index = catalog.search_entries();
        let table_preferences = TablePreferences::load().unwrap_or_default();

        Self {
            config,
            config_path,
            mode: AppMode::Browser,
            focus: Focus::Connections,
            connection_index: 0,
            databases: Vec::new(),
            database_index: 0,
            kind_index: 0,
            objects: Vec::new(),
            object_index: 0,
            content_title: "Salida".to_owned(),
            content: "Pulsa Enter sobre una conexión para comenzar. '?' abre la ayuda.".to_owned(),
            highlighted_content: None,
            content_scroll: 0,
            status: "Listo".to_owned(),
            table_page: None,
            table_metadata: None,
            table_query: TableQuery::default(),
            table_filter_expression: String::new(),
            table_loading_more: false,
            table_state: TableState::default(),
            horizontal_scroll: ScrollbarState::new(0),
            table_column_index: 0,
            table_visible_columns: 0,
            table_show_metadata: false,
            table_visual_anchor: None,
            table_row_visual_anchor: None,
            table_copy_stage: None,
            table_copy_source: None,
            table_copy_menu_index: 0,
            table_value_modal: None,
            table_value_scroll: 0,
            table_cell_editor: None,
            table_date_time_picker: None,
            table_cell_drafts: Vec::new(),
            table_row_deletions: BTreeSet::new(),
            table_new_rows: BTreeSet::new(),
            table_delete_sequence_armed: false,
            table_changes_summary: None,
            table_changes_summary_scroll: 0,
            table_sql_preview: None,
            table_sql_preview_scroll: 0,
            search_return_mode: AppMode::Browser,
            help_return_mode: AppMode::Browser,
            filter_session: None,
            sort_session: None,
            column_search_session: None,
            last_key: String::new(),
            editor: None,
            should_quit: false,
            busy_count: 0,
            request_tx,
            response_rx,
            next_request_id: 1,
            pending_requests: HashSet::new(),
            current_content_object: None,
            open_editor_after_definition: None,
            confirm_action: None,
            confirm_message: String::new(),
            return_to_editor_after_execution: false,
            return_to_table_after_execution: false,
            search: None,
            active_search: None,
            catalog,
            search_index,
            table_preferences,
            catalog_refresh_pending: HashSet::new(),
            pending_catalog_target: None,
            pending_table_navigation: None,
            last_catalog_refresh_check: Instant::now(),
        }
    }

    pub fn bootstrap(&mut self) {
        self.test_connection();
        self.load_databases();
        self.maybe_refresh_catalog(false);
    }

    pub fn current_profile(&self) -> Option<&ConnectionProfile> {
        self.config.connections.get(self.connection_index)
    }

    pub fn current_database(&self) -> Option<&str> {
        self.databases.get(self.database_index).map(String::as_str)
    }

    pub fn current_search_session(&self) -> Option<&SearchSession> {
        self.search.as_ref()
    }

    pub fn current_filter_session(&self) -> Option<&FilterSession> {
        self.filter_session.as_ref()
    }

    pub fn current_sort_session(&self) -> Option<&SortSession> {
        self.sort_session.as_ref()
    }

    pub fn current_column_search_session(&self) -> Option<&ColumnSearchSession> {
        self.column_search_session.as_ref()
    }

    pub fn current_table_value_modal(&self) -> Option<&TableValueModal> {
        self.table_value_modal.as_ref()
    }

    pub fn current_table_cell_editor(&self) -> Option<&TableCellEditorSession> {
        self.table_cell_editor.as_ref()
    }

    pub fn current_table_date_time_picker(&self) -> Option<&TableDateTimePickerSession> {
        self.table_date_time_picker.as_ref()
    }

    pub fn current_table_changes_summary(&self) -> Option<&TableChangesSummary> {
        self.table_changes_summary.as_ref()
    }

    pub fn current_table_sql_preview(&self) -> Option<&sybase::queries::TableSqlPreview> {
        self.table_sql_preview.as_ref()
    }

    pub fn table_cell_draft_value(&self, row_index: usize, column: &str) -> Option<&str> {
        self.table_cell_drafts
            .iter()
            .find(|draft| draft.row_index == row_index && draft.column.eq_ignore_ascii_case(column))
            .map(|draft| draft.value.as_str())
    }

    pub fn active_table_filter(&self) -> Option<&str> {
        (!self.table_filter_expression.is_empty()).then_some(&self.table_filter_expression)
    }

    pub fn active_table_sort(&self) -> Option<String> {
        (!self.table_query.sort.is_empty()).then(|| format_sort_expression(&self.table_query.sort))
    }

    pub fn active_table_pinned_columns(&self) -> Vec<String> {
        self.current_table_preference_key()
            .map(|key| self.table_preferences.pinned_columns(&key).to_vec())
            .unwrap_or_default()
    }

    pub fn pinned_table_column_count(&self) -> usize {
        let pinned = self.active_table_pinned_columns();
        self.table_page
            .as_ref()
            .map(|page| {
                page.columns
                    .iter()
                    .filter(|column| {
                        pinned
                            .iter()
                            .any(|pinned| pinned.eq_ignore_ascii_case(column))
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn active_database(&self) -> Option<&str> {
        self.current_database().or_else(|| {
            self.current_profile()
                .map(ConnectionProfile::initial_database)
        })
    }

    pub fn current_kind(&self) -> ObjectKind {
        ObjectKind::ALL[self.kind_index.min(ObjectKind::ALL.len() - 1)]
    }

    pub fn current_object(&self) -> Option<&DbObject> {
        self.objects.get(self.object_index)
    }

    pub fn confirm_message(&self) -> &str {
        &self.confirm_message
    }

    pub fn active_search_label(&self) -> Option<String> {
        self.active_search
            .as_ref()
            .map(|query| format!("Buscar global: {query}"))
    }

    pub fn poll_worker(&mut self) {
        while let Ok(response) = self.response_rx.try_recv() {
            self.handle_worker_response(response);
        }
        self.maybe_refresh_catalog(false);
        self.flush_search_refresh_if_due();
        self.busy_count = self.pending_requests.len();
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.last_key = format_key(key);
        match self.mode {
            AppMode::Browser => self.handle_browser_key(key),
            AppMode::Editor => self.handle_editor_key(key),
            AppMode::Confirm => self.handle_confirm_key(key),
            AppMode::Table => self.handle_table_key(key),
            AppMode::Search => self.handle_search_key(key),
            AppMode::Help => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
                ) {
                    self.mode = self.help_return_mode;
                }
            }
        }
    }

    fn handle_browser_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            self.status = "Búsqueda global: usa / para buscar en el catálogo".to_owned();
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::BackTab => self.focus = self.focus.previous(),
            KeyCode::Char('h') | KeyCode::Left => self.focus = self.focus.previous(),
            KeyCode::Char('l') | KeyCode::Right => {
                self.activate_selection();
                self.focus = self.focus.next();
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('g') | KeyCode::Home => self.move_to_edge(false),
            KeyCode::Char('G') | KeyCode::End => self.move_to_edge(true),
            KeyCode::Enter => self.activate_selection(),
            KeyCode::Char('r') => self.refresh_current(),
            KeyCode::Char('R') => self.reload_connections(),
            KeyCode::Char('c') => self.test_connection(),
            KeyCode::Char('e') => self.edit_selected_object(),
            KeyCode::Char('E') => self.edit_table_values(),
            KeyCode::Char(':') => self.open_query_editor(),
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('1') => self.focus = Focus::Connections,
            KeyCode::Char('2') => self.focus = Focus::Databases,
            KeyCode::Char('3') => self.focus = Focus::Kinds,
            KeyCode::Char('4') => self.focus = Focus::Objects,
            KeyCode::Char('5') => self.focus = Focus::Content,
            KeyCode::Char('y') => {
                if self.content.is_empty() {
                    self.status = "No hay contenido para copiar".to_owned();
                    return;
                }
                match services::clipboard::copy_text(&self.content) {
                    Ok(()) => {
                        let line_count = self.content.lines().count();
                        self.status =
                            format!("Contenido copiado al portapapeles · {line_count} líneas");
                    }
                    Err(error) => {
                        self.status = format!("ERROR al copiar al portapapeles: {error}");
                    }
                }
            }
            KeyCode::Char('/') => self.open_search(),
            KeyCode::F(5) => self.refresh_catalog(),

            _ => {}
        }
    }

    fn open_search(&mut self) {
        self.search_return_mode = self.mode;
        self.search = Some(SearchSession {
            input: TextArea::default(),
            suggestions: Vec::new(),
            selected_suggestion: 0,
            refresh_deadline: None,
        });

        self.mode = AppMode::Search;
        self.refresh_search_suggestions();
    }

    fn open_help(&mut self) {
        self.help_return_mode = self.mode;
        self.mode = AppMode::Help;
    }

    fn handle_editor_key(&mut self, key: KeyEvent) {
        let command = match self.editor.as_mut() {
            Some(session) => session.editor.handle_key(key),
            None => {
                self.mode = AppMode::Browser;
                return;
            }
        };

        match command {
            EditorCommand::None => {}
            EditorCommand::Save => self.save_editor(),
            EditorCommand::Close => {
                let dirty = self
                    .editor
                    .as_ref()
                    .is_some_and(|session| session.editor.is_dirty());
                if dirty {
                    self.confirm_action = Some(ConfirmAction::DiscardEditor);
                    self.confirm_message =
                        "Hay cambios sin ejecutar. ¿Cerrar el editor y descartarlos? [y/N]"
                            .to_owned();
                    self.mode = AppMode::Confirm;
                } else {
                    self.editor = None;
                    self.mode = AppMode::Browser;
                }
            }
        }
    }

    fn handle_table_key(&mut self, key: KeyEvent) {
        if self.table_date_time_picker.is_some() {
            self.handle_table_date_time_picker_key(key);
            return;
        }

        if self.table_cell_editor.is_some() {
            self.handle_table_cell_editor_key(key);
            return;
        }

        if self.table_sql_preview.is_some() {
            self.handle_table_sql_preview_key(key);
            return;
        }

        if self.table_changes_summary.is_some() {
            self.handle_table_changes_summary_key(key);
            return;
        }

        if self.table_value_modal.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.table_value_modal = None;
                    self.table_value_scroll = 0;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.table_value_scroll = self.table_value_scroll.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.table_value_scroll = self.table_value_scroll.saturating_add(1);
                }
                KeyCode::PageUp => {
                    self.table_value_scroll = self.table_value_scroll.saturating_sub(8);
                }
                KeyCode::PageDown => {
                    self.table_value_scroll = self.table_value_scroll.saturating_add(8);
                }
                KeyCode::Home | KeyCode::Char('g') => self.table_value_scroll = 0,
                KeyCode::End | KeyCode::Char('G') => self.table_value_scroll = u16::MAX,
                _ => {}
            }
            return;
        }

        if key.code == KeyCode::Esc {
            if self.filter_session.is_some() {
                self.filter_session = None;
                self.status = "Editor de filtros cancelado".to_owned();
                return;
            }
            if self.sort_session.is_some() {
                self.sort_session = None;
                self.status = "Editor de ordenamiento cancelado".to_owned();
                return;
            }
            if self.column_search_session.is_some() {
                self.column_search_session = None;
                self.status = "Búsqueda de columna cancelada".to_owned();
                return;
            }
            if self.table_copy_stage.is_some() {
                self.table_copy_stage = None;
                self.table_copy_source = None;
                self.status = "Menú de copia cancelado".to_owned();
                return;
            }
            if self.table_visual_anchor.take().is_some() {
                self.status = "Selección visual cancelada".to_owned();
                return;
            }
            if self.table_row_visual_anchor.take().is_some() {
                self.status = "Selección de filas cancelada".to_owned();
                return;
            }
            self.request_table_exit();
            return;
        }

        if self.filter_session.is_some() {
            self.handle_table_filter_key(key);
            return;
        }

        if self.sort_session.is_some() {
            self.handle_table_sort_key(key);
            return;
        }

        if self.column_search_session.is_some() {
            self.handle_table_column_search_key(key);
            return;
        }

        if key.code == KeyCode::Char('q') {
            self.request_table_exit();
            return;
        }

        if key.code == KeyCode::Char('c') {
            self.open_table_column_search();
            return;
        }

        if key.code == KeyCode::Char('p') {
            self.toggle_table_column_pin();
            return;
        }

        if key.code == KeyCode::Char('o') {
            if self.table_metadata.is_none() {
                self.status = "La metadata de columnas todavía no está disponible".to_owned();
                return;
            }
            self.open_table_sort();
            return;
        }

        if key.code == KeyCode::Char('O') {
            self.clear_table_sort();
            return;
        }

        if key.code == KeyCode::Char('f') {
            if self.table_metadata.is_none() {
                self.status = "La metadata de columnas todavía no está disponible".to_owned();
                return;
            }
            self.open_table_filter();
            return;
        }

        if key.code == KeyCode::Char('F') {
            self.clear_table_filter();
            return;
        }

        if key.code == KeyCode::Char('i') {
            self.table_show_metadata = !self.table_show_metadata;
            self.content_scroll = 0;
            self.table_visual_anchor = None;
            self.table_row_visual_anchor = None;
            return;
        }

        if self.table_show_metadata {
            if key.code == KeyCode::Char('y') {
                self.copy_table_metadata();
                return;
            }
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.content_scroll = self.content_scroll.saturating_add(1);
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.content_scroll = self.content_scroll.saturating_sub(1);
                }
                KeyCode::Char('g') | KeyCode::Home => self.content_scroll = 0,
                KeyCode::Char('G') | KeyCode::End => self.content_scroll = u16::MAX,
                _ => {}
            }
            return;
        }

        if self.table_copy_stage.is_some() {
            self.handle_table_copy_key(key);
            return;
        }

        if key.code == KeyCode::Char('?') {
            self.open_help();
            return;
        }

        if key.code == KeyCode::Char('/') {
            self.open_search();
            return;
        }

        if matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.open_table_changes_summary();
            return;
        }

        let Some((row_count, column_count, has_more)) = self
            .table_page
            .as_ref()
            .map(|page| (page.rows.len(), page.columns.len(), page.has_more))
        else {
            self.mode = AppMode::Browser;
            return;
        };

        if column_count == 0 {
            return;
        }

        if key.code != KeyCode::Char('d') {
            self.table_delete_sequence_armed = false;
        }
        if key.code == KeyCode::Char('d') {
            if self.table_delete_sequence_armed {
                self.copy_current_row_and_mark_for_delete();
                self.table_delete_sequence_armed = false;
            } else {
                self.mark_selected_rows_for_delete();
                self.table_delete_sequence_armed = true;
            }
            return;
        }

        if key.code == KeyCode::Char('u') {
            self.undo_table_row_change();
            return;
        }

        let clone_row = (key.code == KeyCode::Char('+')
            && key.modifiers.contains(KeyModifiers::SHIFT))
            || (key.code == KeyCode::Char('=') && key.modifiers.contains(KeyModifiers::SHIFT));
        if clone_row {
            self.append_table_new_row(true);
            return;
        }
        if key.code == KeyCode::Char('+') {
            self.append_table_new_row(false);
            return;
        }

        if key.code == KeyCode::Char('V')
            || (key.code == KeyCode::Char('v') && key.modifiers.contains(KeyModifiers::SHIFT))
        {
            if row_count == 0 {
                self.status = "No hay filas para seleccionar".to_owned();
                return;
            }
            if self.table_row_visual_anchor.take().is_some() {
                self.status = "Selección de filas desactivada".to_owned();
            } else {
                self.table_visual_anchor = None;
                self.table_row_visual_anchor = Some(self.table_state.selected().unwrap_or(0));
                self.status = "Selección de filas activa · mueve el cursor · y copia · Esc cancela"
                    .to_owned();
            }
            return;
        }

        if key.code == KeyCode::Char('e') {
            self.open_table_cell_editor();
            return;
        }

        if key.code == KeyCode::Enter {
            self.open_table_value_modal();
            return;
        }

        if key.code == KeyCode::Char('v') {
            if self.table_row_visual_anchor.is_some() {
                self.status = "Ya hay una selección de filas activa · Esc cancela".to_owned();
                return;
            }
            if self.table_visual_anchor.take().is_some() {
                self.status = "Selección visual desactivada".to_owned();
            } else {
                let row = self.table_state.selected().unwrap_or(0);
                self.table_visual_anchor = Some((row, self.table_column_index));
                self.status =
                    "Selección visual activa · mueve el cursor · y copia · Esc cancela".to_owned();
            }
            return;
        }

        if key.code == KeyCode::Char('Y') {
            self.open_table_copy_menu();
            return;
        }

        if key.code == KeyCode::Char('y') {
            if self.table_row_visual_anchor.is_some() {
                self.open_table_header_choice(TableCopySource::VisualRows);
            } else if self.table_visual_anchor.is_some() {
                self.open_table_header_choice(TableCopySource::VisualSelection);
            } else {
                self.copy_table_source(TableCopySource::CurrentCell, false);
            }
            return;
        }

        let visual = self.table_visual_anchor.is_some();

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if row_count == 0 {
                    return;
                }

                let current = self.table_state.selected().unwrap_or(0);
                let next = (current + 1).min(row_count - 1);

                self.table_state.select(Some(next));
                self.update_visual_status(visual);
                if next + 1 == row_count
                    && has_more
                    && self.table_new_rows.is_empty()
                    && !self.table_loading_more
                {
                    self.load_next_table_page();
                }
            }

            KeyCode::Char('k') | KeyCode::Up => {
                if row_count == 0 {
                    return;
                }

                let current = self.table_state.selected().unwrap_or(0);
                let previous = current.saturating_sub(1);

                self.table_state.select(Some(previous));
                self.update_visual_status(visual);
            }
            KeyCode::Char('h') | KeyCode::Left => {
                let previous = self.table_column_index.saturating_sub(1);
                self.table_column_index = previous;

                if previous < self.horizontal_scroll.get_position() {
                    self.horizontal_scroll.prev();
                }
                self.update_visual_status(visual);
            }

            KeyCode::Char('l') | KeyCode::Right => {
                let last_column = column_count.saturating_sub(1);
                let next = self.table_column_index.saturating_add(1).min(last_column);
                self.table_column_index = next;

                let viewport_end = self
                    .horizontal_scroll
                    .get_position()
                    .saturating_add(self.table_visible_columns.max(1));
                if next >= viewport_end {
                    self.horizontal_scroll.next();
                }
                self.update_visual_status(visual);
            }

            KeyCode::Char('g') | KeyCode::Home => {
                if row_count == 0 {
                    return;
                }

                self.table_state.select(Some(0));
                self.update_visual_status(visual);
            }

            KeyCode::Char('G') | KeyCode::End => {
                if row_count == 0 {
                    return;
                }

                self.table_state.select(Some(row_count - 1));
                if has_more && self.table_new_rows.is_empty() && !self.table_loading_more {
                    self.load_next_table_page();
                }
                self.update_visual_status(visual);
            }

            _ => {}
        }
    }

    fn open_table_value_modal(&mut self) {
        let Some(row_index) = self.table_state.selected() else {
            self.status = "No hay una fila seleccionada".to_owned();
            return;
        };
        let Some((column, raw_value)) = self.table_page.as_ref().and_then(|page| {
            let column = page.columns.get(self.table_column_index)?.clone();
            let value = selected_table_value(page, Some(row_index), self.table_column_index)?;
            Some((column, value))
        }) else {
            self.status = "No hay un valor seleccionado".to_owned();
            return;
        };
        let value = self
            .table_cell_draft_value(row_index, &column)
            .map(ToOwned::to_owned)
            .unwrap_or(raw_value);
        let data_type = self
            .table_metadata
            .as_ref()
            .and_then(|metadata| {
                metadata
                    .columns
                    .iter()
                    .find(|metadata| metadata.name.eq_ignore_ascii_case(&column))
            })
            .map(format_table_type)
            .unwrap_or_else(|| "desconocido".to_owned());

        self.table_value_modal = Some(TableValueModal {
            column,
            data_type,
            row_number: row_index.saturating_add(1),
            value,
        });
        self.table_value_scroll = 0;
        self.status = "Valor completo · j/k desplaza · Enter/Esc cierra".to_owned();
    }

    fn open_table_changes_summary(&mut self) {
        if !self.has_pending_table_changes() {
            self.status = "No hay cambios staged para resumir".to_owned();
            return;
        }

        let identity_columns = self.table_identity_columns();
        let new_rows = self.table_new_rows.clone();
        let requires_identity = self
            .table_cell_drafts
            .iter()
            .any(|draft| !new_rows.contains(&draft.row_index))
            || self
                .table_row_deletions
                .iter()
                .any(|row_index| !new_rows.contains(row_index));
        let identity_warning = if requires_identity && identity_columns.is_empty() {
            Some(
                "No se detectó una clave primaria ni un índice único. Los UPDATE/DELETE requerirán una identidad segura antes de generar SQL."
                    .to_owned(),
            )
        } else {
            None
        };
        let edited_cells = self
            .table_cell_drafts
            .iter()
            .map(|draft| {
                format!(
                    "Fila {} · {}: {} → {}",
                    draft.row_index.saturating_add(1),
                    draft.column,
                    compact_table_summary_value(&draft.original_value),
                    compact_table_summary_value(&draft.value),
                )
            })
            .collect();
        let deleted_rows = self
            .table_row_deletions
            .iter()
            .map(|row_index| row_index.saturating_add(1))
            .collect();

        self.table_changes_summary = Some(TableChangesSummary {
            edited_cells,
            deleted_rows,
            new_row_count: self.table_new_rows.len(),
            identity_columns,
            identity_warning,
        });
        self.table_changes_summary_scroll = 0;
        self.status = "Resumen de cambios · Enter abre SQL · Esc cierra".to_owned();
    }

    fn handle_table_changes_summary_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.table_changes_summary = None;
                self.table_changes_summary_scroll = 0;
                self.status = "Resumen de cambios cerrado".to_owned();
            }
            KeyCode::Enter => self.open_table_sql_preview(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.table_changes_summary_scroll =
                    self.table_changes_summary_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.table_changes_summary_scroll =
                    self.table_changes_summary_scroll.saturating_add(1);
            }
            KeyCode::PageUp => {
                self.table_changes_summary_scroll =
                    self.table_changes_summary_scroll.saturating_sub(8);
            }
            KeyCode::PageDown => {
                self.table_changes_summary_scroll =
                    self.table_changes_summary_scroll.saturating_add(8);
            }
            KeyCode::Home | KeyCode::Char('g') => self.table_changes_summary_scroll = 0,
            KeyCode::End | KeyCode::Char('G') => {
                self.table_changes_summary_scroll = u16::MAX;
            }
            _ => {}
        }
    }

    fn open_table_sql_preview(&mut self) {
        let Some(metadata) = self.table_metadata.as_ref().cloned() else {
            self.status = "No hay metadata para generar SQL staged".to_owned();
            return;
        };
        let Some(page) = self.table_page.as_ref().cloned() else {
            self.status = "No hay datos cargados para generar SQL staged".to_owned();
            return;
        };
        let drafts = self
            .table_cell_drafts
            .iter()
            .map(|draft| {
                (
                    draft.row_index,
                    draft.column.clone(),
                    draft.original_value.clone(),
                    draft.value.clone(),
                )
            })
            .collect::<Vec<_>>();
        let deleted_rows = self.table_row_deletions.iter().copied().collect::<Vec<_>>();
        let new_rows = self.table_new_rows.iter().copied().collect::<Vec<_>>();
        self.table_sql_preview = Some(sybase::queries::preview_staged_table_changes(
            &metadata,
            &page,
            &drafts,
            &deleted_rows,
            &new_rows,
        ));
        self.table_sql_preview_scroll = 0;
        self.status =
            "Vista previa SQL · Ctrl+S solicita ejecución · Esc vuelve al resumen".to_owned();
    }

    fn handle_table_sql_preview_key(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.request_table_sql_execution();
            return;
        }

        match key.code {
            KeyCode::Esc => {
                self.table_sql_preview = None;
                self.table_sql_preview_scroll = 0;
                self.status = "Vista previa SQL cerrada".to_owned();
            }
            KeyCode::Enter => {
                self.status = "La vista previa SQL es solo lectura".to_owned();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.table_sql_preview_scroll = self.table_sql_preview_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.table_sql_preview_scroll = self.table_sql_preview_scroll.saturating_add(1);
            }
            KeyCode::PageUp => {
                self.table_sql_preview_scroll = self.table_sql_preview_scroll.saturating_sub(8);
            }
            KeyCode::PageDown => {
                self.table_sql_preview_scroll = self.table_sql_preview_scroll.saturating_add(8);
            }
            KeyCode::Home | KeyCode::Char('g') => self.table_sql_preview_scroll = 0,
            KeyCode::End | KeyCode::Char('G') => self.table_sql_preview_scroll = u16::MAX,
            _ => {}
        }
    }

    fn request_table_sql_execution(&mut self) {
        let Some(preview) = self.table_sql_preview.as_ref().cloned() else {
            self.status = "No hay una vista previa SQL activa".to_owned();
            return;
        };
        if !preview.blockers.is_empty() {
            self.status =
                "Ejecución bloqueada: corrige todos los bloqueos de la vista SQL".to_owned();
            return;
        }
        if !contains_executable_table_sql(&preview.sql) {
            self.status = "Ejecución bloqueada: no hay SQL ejecutable".to_owned();
            return;
        }
        if !self
            .current_profile()
            .is_some_and(|profile| profile.allow_writes)
        {
            self.status =
                "Escritura bloqueada: cambia allow_writes = true para esta conexión".to_owned();
            return;
        }

        let table = self
            .table_metadata
            .as_ref()
            .map(|metadata| metadata.identifier.qualified_name())
            .unwrap_or_else(|| "la tabla actual".to_owned());
        self.confirm_action = Some(ConfirmAction::ExecuteTableChanges { sql: preview.sql });
        self.confirm_message =
            format!("Se ejecutarán los cambios staged de {table}. ¿Confirmar? [y/N]");
        self.mode = AppMode::Confirm;
    }

    fn table_identity_columns(&self) -> Vec<String> {
        let Some(metadata) = self.table_metadata.as_ref() else {
            return Vec::new();
        };
        let Some(page_columns) = self.table_page.as_ref().map(|page| &page.columns) else {
            return Vec::new();
        };
        metadata
            .indexes
            .iter()
            .filter(|index| index.is_primary && !index.columns.is_empty())
            .find(|index| {
                index.columns.iter().all(|index_column| {
                    page_columns
                        .iter()
                        .any(|page_column| page_column.eq_ignore_ascii_case(index_column))
                })
            })
            .or_else(|| {
                metadata
                    .indexes
                    .iter()
                    .filter(|index| index.is_unique && !index.columns.is_empty())
                    .find(|index| {
                        index.columns.iter().all(|index_column| {
                            page_columns
                                .iter()
                                .any(|page_column| page_column.eq_ignore_ascii_case(index_column))
                        })
                    })
            })
            .map(|index| index.columns.clone())
            .unwrap_or_default()
    }

    fn open_table_cell_editor(&mut self) {
        let Some(row_index) = self.table_state.selected() else {
            self.status = "No hay una fila seleccionada".to_owned();
            return;
        };
        let Some((column, raw_value)) = self.table_page.as_ref().and_then(|page| {
            let column = page.columns.get(self.table_column_index)?.clone();
            let value = selected_table_value(page, Some(row_index), self.table_column_index)?;
            Some((column, value))
        }) else {
            self.status = "No hay un valor seleccionado".to_owned();
            return;
        };
        let Some(metadata) = self.table_metadata.as_ref().and_then(|table| {
            table
                .columns
                .iter()
                .find(|candidate| candidate.name.eq_ignore_ascii_case(&column))
                .cloned()
        }) else {
            self.status = "La metadata es necesaria para editar esta celda".to_owned();
            return;
        };

        let existing_draft = self.table_cell_drafts.iter().find(|draft| {
            draft.row_index == row_index && draft.column.eq_ignore_ascii_case(&column)
        });
        let original_value = existing_draft
            .map(|draft| draft.original_value.clone())
            .unwrap_or_else(|| raw_value.clone());
        let value = existing_draft
            .map(|draft| draft.value.clone())
            .unwrap_or(raw_value);
        let data_type = format_table_type(&metadata);

        if table_date_time_kind(&metadata.data_type).is_some() {
            self.open_table_date_time_picker(row_index, column, value, metadata);
            return;
        }

        self.table_cell_editor = Some(TableCellEditorSession {
            column,
            data_type,
            row_number: row_index.saturating_add(1),
            nullable: metadata.nullable,
            original_value,
            input: search_input(&value),
            row_index,
            metadata,
        });
        self.status = "Editando celda · Enter/Ctrl+S guarda borrador · Esc cancela".to_owned();
    }

    fn open_table_date_time_picker(
        &mut self,
        row_index: usize,
        column: String,
        value: String,
        metadata: ColumnMetadata,
    ) {
        let Some(kind) = table_date_time_kind(&metadata.data_type) else {
            return;
        };
        let picker_value = if value.trim().is_empty() {
            if metadata.nullable {
                "<NULL>".to_owned()
            } else {
                match kind {
                    TableDateTimeKind::Date => "2000-01-01".to_owned(),
                    TableDateTimeKind::DateTime => "2000-01-01 00:00:00".to_owned(),
                    TableDateTimeKind::Time => "00:00:00".to_owned(),
                }
            }
        } else {
            value.clone()
        };
        let parsed = match parse_table_date_time_value(&picker_value, kind) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.status = format!("No se pudo abrir el picker: {error}");
                return;
            }
        };
        let original_value = self
            .table_cell_drafts
            .iter()
            .find(|draft| {
                draft.row_index == row_index && draft.column.eq_ignore_ascii_case(&column)
            })
            .map(|draft| draft.original_value.clone())
            .unwrap_or_else(|| {
                if value.trim().is_empty() {
                    value.clone()
                } else {
                    format_parsed_table_date_time(&parsed, kind)
                }
            });

        self.table_date_time_picker = Some(TableDateTimePickerSession {
            column,
            data_type: format_table_type(&metadata),
            row_number: row_index.saturating_add(1),
            nullable: metadata.nullable,
            show_date: !matches!(kind, TableDateTimeKind::Time),
            show_time: !matches!(kind, TableDateTimeKind::Date),
            is_null: parsed.is_null,
            selected_component: 0,
            year: parsed.year,
            month: parsed.month,
            day: parsed.day,
            hour: parsed.hour,
            minute: parsed.minute,
            second: parsed.second,
            fractional_seconds: parsed.fractional_seconds,
            original_value,
            row_index,
            metadata,
        });
        self.status =
            "Picker date/hora · Tab cambia componente · ↑/↓ ajusta · Enter guarda · Esc cancela"
                .to_owned();
    }

    fn handle_table_date_time_picker_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.table_date_time_picker = None;
            self.status = "Picker date/hora cancelado".to_owned();
            return;
        }
        if key.code == KeyCode::Enter {
            self.commit_table_date_time_draft();
            return;
        }
        if key.code == KeyCode::Char(' ') {
            self.toggle_table_date_time_null();
            return;
        }
        if key.code == KeyCode::BackTab
            || (key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT))
            || key.code == KeyCode::Left
        {
            self.move_table_date_time_component(-1);
            return;
        }
        if key.code == KeyCode::Tab || key.code == KeyCode::Right {
            self.move_table_date_time_component(1);
            return;
        }
        if key.code == KeyCode::Up {
            self.adjust_table_date_time_component(1);
            return;
        }
        if key.code == KeyCode::Down {
            self.adjust_table_date_time_component(-1);
        }
    }

    fn move_table_date_time_component(&mut self, delta: isize) {
        let Some(session) = self.table_date_time_picker.as_mut() else {
            return;
        };
        let component_count = if session.show_date && session.show_time {
            6
        } else {
            3
        };
        session.selected_component = if delta < 0 {
            if session.selected_component == 0 {
                component_count - 1
            } else {
                session.selected_component - 1
            }
        } else {
            (session.selected_component + 1) % component_count
        };
    }

    fn adjust_table_date_time_component(&mut self, delta: i32) {
        let Some(session) = self.table_date_time_picker.as_mut() else {
            return;
        };
        session.is_null = false;
        let component = session.selected_component;
        if session.show_date {
            match component {
                0 => session.year = stepped_clamped(session.year, 1, 9999, delta),
                1 => session.month = stepped_wrapped(session.month, 1, 12, delta),
                2 => {
                    let max_day = days_in_month(session.year, session.month);
                    session.day = stepped_clamped_u8(session.day, 1, max_day, delta);
                }
                3 => session.hour = stepped_wrapped(session.hour, 0, 23, delta),
                4 => session.minute = stepped_wrapped(session.minute, 0, 59, delta),
                5 => session.second = stepped_wrapped(session.second, 0, 59, delta),
                _ => {}
            }
        } else {
            match component {
                0 => session.hour = stepped_wrapped(session.hour, 0, 23, delta),
                1 => session.minute = stepped_wrapped(session.minute, 0, 59, delta),
                2 => session.second = stepped_wrapped(session.second, 0, 59, delta),
                _ => {}
            }
        }
        let max_day = days_in_month(session.year, session.month);
        session.day = session.day.min(max_day);
    }

    fn toggle_table_date_time_null(&mut self) {
        let Some(session) = self.table_date_time_picker.as_mut() else {
            return;
        };
        if !session.nullable {
            self.status = "Esta columna no permite NULL".to_owned();
            return;
        }
        session.is_null = !session.is_null;
        self.status = if session.is_null {
            "Valor NULL seleccionado · Space reactiva fecha/hora".to_owned()
        } else {
            "Fecha/hora activada · Enter guarda borrador".to_owned()
        };
    }

    fn commit_table_date_time_draft(&mut self) {
        let Some(session) = self.table_date_time_picker.as_ref() else {
            return;
        };
        let value = format_table_date_time_picker(session);
        if let Err(error) = validate_table_cell_value(&value, &session.metadata) {
            self.status = format!("Valor inválido: {error}");
            return;
        }

        let row_index = session.row_index;
        let column = session.column.clone();
        let original_value = session.original_value.clone();
        let change = store_table_cell_draft(
            &mut self.table_cell_drafts,
            row_index,
            column.clone(),
            original_value,
            value,
        );
        self.status = match change {
            TableDraftChange::Reverted => format!("Borrador revertido: {column}"),
            TableDraftChange::Updated => format!("Borrador actualizado: {column}"),
            TableDraftChange::Saved => {
                format!("Borrador date/hora guardado: {column} · todavía no se ejecuta SQL")
            }
        };
        self.table_date_time_picker = None;
    }

    fn handle_table_cell_editor_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.table_cell_editor = None;
            self.status = "Edición de celda cancelada".to_owned();
            return;
        }
        if key.code == KeyCode::Enter
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s'))
        {
            self.commit_table_cell_draft();
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            if let Some(session) = self.table_cell_editor.as_mut() {
                session.input = TextArea::default();
            }
            return;
        }

        if let Some(session) = self.table_cell_editor.as_mut() {
            match key.code {
                KeyCode::Left => session.input.move_cursor(CursorMove::Back),
                KeyCode::Right => session.input.move_cursor(CursorMove::Forward),
                _ => {
                    let input: Input = key.into();
                    session.input.input_without_shortcuts(input);
                }
            }
        }
    }

    fn commit_table_cell_draft(&mut self) {
        let Some(session) = self.table_cell_editor.as_ref() else {
            return;
        };
        let value = session.input.lines().join("\n");
        if let Err(error) = validate_table_cell_value(&value, &session.metadata) {
            self.status = format!("Valor inválido: {error}");
            return;
        }

        let row_index = session.row_index;
        let column = session.column.clone();
        let original_value = session.original_value.clone();
        let change = store_table_cell_draft(
            &mut self.table_cell_drafts,
            row_index,
            column.clone(),
            original_value,
            value,
        );
        self.status = match change {
            TableDraftChange::Reverted => format!("Borrador revertido: {column}"),
            TableDraftChange::Updated => format!("Borrador actualizado: {column}"),
            TableDraftChange::Saved => {
                format!("Borrador guardado: {column} · todavía no se ejecuta SQL")
            }
        };
        self.table_cell_editor = None;
    }

    fn clear_table_cell_editing(&mut self) {
        self.table_cell_editor = None;
        self.table_date_time_picker = None;
        self.table_cell_drafts.clear();
        self.table_row_deletions.clear();
        self.table_new_rows.clear();
        self.table_changes_summary = None;
        self.table_changes_summary_scroll = 0;
        self.table_sql_preview = None;
        self.table_sql_preview_scroll = 0;
        self.table_row_visual_anchor = None;
        self.table_delete_sequence_armed = false;
    }

    fn has_pending_table_changes(&self) -> bool {
        !self.table_cell_drafts.is_empty()
            || !self.table_row_deletions.is_empty()
            || !self.table_new_rows.is_empty()
    }

    fn request_table_exit(&mut self) {
        self.table_delete_sequence_armed = false;
        if !self.has_pending_table_changes() {
            self.exit_table_mode();
            return;
        }

        self.confirm_action = Some(ConfirmAction::DiscardTableChanges);
        self.confirm_message = table_exit_message(
            self.table_cell_drafts.len(),
            self.table_row_deletions.len(),
            self.table_new_rows.len(),
        );
        self.mode = AppMode::Confirm;
    }

    fn exit_table_mode(&mut self) {
        self.table_page = None;
        self.table_metadata = None;
        self.table_show_metadata = false;
        self.table_visual_anchor = None;
        self.table_row_visual_anchor = None;
        self.table_copy_stage = None;
        self.table_copy_source = None;
        self.sort_session = None;
        self.column_search_session = None;
        self.clear_table_cell_editing();
        self.table_row_deletions.clear();
        self.table_new_rows.clear();
        self.table_delete_sequence_armed = false;
        self.table_value_modal = None;
        self.table_value_scroll = 0;
        self.pending_table_navigation = None;
        self.mode = AppMode::Browser;
    }

    fn update_visual_status(&mut self, visual: bool) {
        if self.table_row_visual_anchor.is_some() {
            self.status =
                "Selección de filas activa · mueve el cursor · y copia · Esc cancela".to_owned();
        } else if visual {
            self.status = "Selección visual activa · y copia · Esc cancela".to_owned();
        }
    }

    fn selected_table_row_range(&self) -> Option<(usize, usize)> {
        let row_count = self.table_page.as_ref()?.rows.len();
        if row_count == 0 {
            return None;
        }
        let current = self.table_state.selected()?.min(row_count - 1);
        let anchor = self
            .table_row_visual_anchor
            .unwrap_or(current)
            .min(row_count - 1);
        Some((anchor.min(current), anchor.max(current)))
    }

    fn mark_selected_rows_for_delete(&mut self) {
        let Some((start, end)) = self.selected_table_row_range() else {
            self.status = "No hay filas para marcar".to_owned();
            return;
        };
        for row_index in start..=end {
            self.table_row_deletions.insert(row_index);
        }
        self.table_row_visual_anchor = None;
        let count = end - start + 1;
        self.status = format!(
            "{} {} marcada(s) para borrar · u deshace · todavía no se ejecuta SQL",
            count,
            if count == 1 { "fila" } else { "filas" }
        );
    }

    fn unmark_selected_rows_for_delete(&mut self) {
        let Some((start, end)) = self.selected_table_row_range() else {
            self.status = "No hay filas para desmarcar".to_owned();
            return;
        };
        let mut count = 0;
        for row_index in start..=end {
            count += usize::from(self.table_row_deletions.remove(&row_index));
        }
        self.status = if count == 0 {
            "No había filas marcadas para borrar".to_owned()
        } else {
            format!(
                "{} {} desmarcada(s)",
                count,
                if count == 1 { "fila" } else { "filas" }
            )
        };
    }

    fn copy_current_row_and_mark_for_delete(&mut self) {
        let Some(row_index) = self.table_state.selected() else {
            self.status = "No hay una fila seleccionada para dd".to_owned();
            return;
        };
        let Some(page) = self.table_page.clone() else {
            self.status = "No hay datos cargados para dd".to_owned();
            return;
        };
        let Some(text) = table_copy_rows_text(&page, Some(row_index), Some(row_index), false)
        else {
            self.status = "No se pudo copiar la fila para dd".to_owned();
            return;
        };
        self.table_row_deletions.insert(row_index);
        self.status = match services::clipboard::copy_text(&text) {
            Ok(()) => "Fila copiada y marcada para borrar · todavía no se ejecuta SQL".to_owned(),
            Err(error) => format!("Fila marcada para borrar, pero no se pudo copiar: {error}"),
        };
    }

    fn table_row_values_with_drafts(&self, row_index: usize) -> Option<Vec<String>> {
        let page = self.table_page.as_ref()?;
        let values = page.rows.get(row_index)?;
        Some(
            page.columns
                .iter()
                .enumerate()
                .map(|(column_index, column)| {
                    self.table_cell_draft_value(row_index, column)
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| values.get(column_index).cloned().unwrap_or_default())
                })
                .collect(),
        )
    }

    fn append_table_new_row(&mut self, clone_current: bool) {
        if self.table_metadata.is_none() {
            self.status = "La metadata es necesaria para crear filas".to_owned();
            return;
        }
        let values = if clone_current {
            let Some(row_index) = self.table_state.selected() else {
                self.status = "No hay una fila seleccionada para clonar".to_owned();
                return;
            };
            let Some(values) = self.table_row_values_with_drafts(row_index) else {
                self.status = "No se pudo clonar la fila seleccionada".to_owned();
                return;
            };
            values
        } else {
            let Some(column_count) = self.table_page.as_ref().map(|page| page.columns.len()) else {
                self.status = "No hay una tabla cargada".to_owned();
                return;
            };
            if column_count == 0 {
                self.status = "La tabla no tiene columnas".to_owned();
                return;
            }
            vec![String::new(); column_count]
        };
        let Some(page) = self.table_page.as_mut() else {
            self.status = "No hay una tabla cargada".to_owned();
            return;
        };
        let row_index = page.rows.len();
        page.rows.push(values);
        self.table_new_rows.insert(row_index);
        self.table_state.select(Some(row_index));
        self.table_visual_anchor = None;
        self.table_row_visual_anchor = None;
        self.table_delete_sequence_armed = false;
        self.status = if clone_current {
            "Fila clonada como borrador nuevo · usa e para editar".to_owned()
        } else {
            "Fila nueva creada como borrador · usa e para editar".to_owned()
        };
    }

    fn undo_table_row_change(&mut self) {
        let Some(row_index) = self.table_state.selected() else {
            self.status = "No hay una fila seleccionada".to_owned();
            return;
        };
        if self.table_new_rows.contains(&row_index) {
            self.discard_new_row(row_index);
        } else {
            self.unmark_selected_rows_for_delete();
        }
    }

    fn discard_new_row(&mut self, row_index: usize) {
        if !self.table_new_rows.contains(&row_index) {
            self.status = "La fila seleccionada no es un borrador nuevo".to_owned();
            return;
        }
        let Some(page) = self.table_page.as_mut() else {
            return;
        };
        if row_index >= page.rows.len() {
            return;
        }
        page.rows.remove(row_index);
        self.table_new_rows = shift_row_indexes(&self.table_new_rows, row_index);
        self.table_row_deletions = shift_row_indexes(&self.table_row_deletions, row_index);
        self.table_cell_drafts
            .retain(|draft| draft.row_index != row_index);
        for draft in &mut self.table_cell_drafts {
            if draft.row_index > row_index {
                draft.row_index -= 1;
            }
        }
        let next_row = self
            .table_page
            .as_ref()
            .and_then(|page| (!page.rows.is_empty()).then_some(row_index.min(page.rows.len() - 1)));
        self.table_state.select(next_row);
        self.table_visual_anchor = None;
        self.table_row_visual_anchor = None;
        self.table_delete_sequence_armed = false;
        self.status = "Fila nueva descartada".to_owned();
    }

    fn open_table_filter(&mut self) {
        let expression = self.table_filter_expression.clone();
        self.filter_session = Some(FilterSession {
            input: search_input(&expression),
            suggestions: Vec::new(),
            selected_suggestion: 0,
            parse_error: None,
            preview: Vec::new(),
        });
        self.mode = AppMode::Table;
        self.refresh_table_filter_suggestions();
        self.status = "Escribe el filtro · Tab completa · Enter aplica · Esc cancela".to_owned();
    }

    fn handle_table_filter_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Tab {
            self.accept_table_filter_suggestion();
            return;
        }
        if key.code == KeyCode::Up {
            self.move_table_filter_suggestion(-1);
            return;
        }
        if key.code == KeyCode::Down {
            self.move_table_filter_suggestion(1);
            return;
        }
        if key.code == KeyCode::Enter {
            self.apply_table_filter();
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            if let Some(session) = self.filter_session.as_mut() {
                session.input = TextArea::default();
            }
            self.refresh_table_filter_suggestions();
            return;
        }

        if let Some(session) = self.filter_session.as_mut() {
            match key.code {
                KeyCode::Left => session.input.move_cursor(CursorMove::Back),
                KeyCode::Right => session.input.move_cursor(CursorMove::Forward),
                _ => {
                    let input: Input = key.into();
                    session.input.input_without_shortcuts(input);
                }
            }
        }
        self.refresh_table_filter_suggestions();
    }

    fn move_table_filter_suggestion(&mut self, delta: isize) {
        let Some(session) = self.filter_session.as_mut() else {
            return;
        };
        session.selected_suggestion = shifted_index(
            session.selected_suggestion,
            session.suggestions.len(),
            delta,
        );
    }

    fn accept_table_filter_suggestion(&mut self) {
        let Some(suggestion) = self
            .filter_session
            .as_ref()
            .and_then(|session| session.suggestions.get(session.selected_suggestion))
            .cloned()
        else {
            return;
        };
        let Some(session) = self.filter_session.as_mut() else {
            return;
        };
        let current = session.input.lines().join("\n");
        let prefix = current.get(..suggestion.replace_start).unwrap_or("");
        let updated = format!("{}{value}", prefix, value = suggestion.insertion);
        session.input = search_input(&updated);
        session.input.move_cursor(CursorMove::End);
        self.refresh_table_filter_suggestions();
    }

    fn refresh_table_filter_suggestions(&mut self) {
        let Some(expression) = self
            .filter_session
            .as_ref()
            .map(|session| session.input.lines().join("\n"))
        else {
            return;
        };
        let columns = self.table_columns();
        let suggestions = table_filter_suggestions(&expression, &columns);
        let preview = parse_filter_expression(&expression, &columns)
            .map(|filters| filters.iter().map(format_filter_spec).collect::<Vec<_>>())
            .unwrap_or_default();
        let parse_error = if expression.trim().is_empty() {
            None
        } else {
            parse_filter_expression(&expression, &columns)
                .err()
                .map(|error| error.to_string())
        };

        if let Some(session) = self.filter_session.as_mut() {
            session.suggestions = suggestions;
            session.selected_suggestion = session
                .selected_suggestion
                .min(session.suggestions.len().saturating_sub(1));
            session.parse_error = parse_error;
            session.preview = preview;
        }
    }

    fn apply_table_filter(&mut self) {
        let Some(session) = self.filter_session.as_ref() else {
            return;
        };
        let expression = session.input.lines().join("\n").trim().to_owned();
        if expression.is_empty() {
            self.filter_session = None;
            self.clear_table_filter();
            return;
        }
        let columns = self.table_columns();
        let filters = match parse_filter_expression(&expression, &columns) {
            Ok(filters) => filters,
            Err(error) => {
                self.status = format!("Filtro inválido: {error}");
                return;
            }
        };

        self.table_filter_expression = expression;
        self.table_query.filters = filters;
        self.table_query.page.cursor = None;
        self.filter_session = None;
        self.reset_table_page();
        self.load_table_query_page("Aplicando filtro...");
    }

    fn clear_table_filter(&mut self) {
        if self.table_query.filters.is_empty() && self.table_filter_expression.is_empty() {
            self.status = "No hay filtros activos".to_owned();
            return;
        }
        self.table_query.filters.clear();
        self.table_filter_expression.clear();
        self.table_query.page.cursor = None;
        self.filter_session = None;
        self.reset_table_page();
        self.load_table_query_page("Limpiando filtros...");
    }

    fn open_table_sort(&mut self) {
        let expression = format_sort_expression(&self.table_query.sort);
        self.sort_session = Some(SortSession {
            input: search_input(&expression),
            suggestions: Vec::new(),
            selected_suggestion: 0,
            parse_error: None,
            preview: Vec::new(),
        });
        self.mode = AppMode::Table;
        self.refresh_table_sort_suggestions();
        self.status = "Escribe el orden · Tab completa · Enter aplica · Esc cancela".to_owned();
    }

    fn handle_table_sort_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Tab {
            self.accept_table_sort_suggestion();
            return;
        }
        if key.code == KeyCode::Up {
            self.move_table_sort_suggestion(-1);
            return;
        }
        if key.code == KeyCode::Down {
            self.move_table_sort_suggestion(1);
            return;
        }
        if key.code == KeyCode::Enter {
            self.apply_table_sort();
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            if let Some(session) = self.sort_session.as_mut() {
                session.input = TextArea::default();
            }
            self.refresh_table_sort_suggestions();
            return;
        }

        if let Some(session) = self.sort_session.as_mut() {
            match key.code {
                KeyCode::Left => session.input.move_cursor(CursorMove::Back),
                KeyCode::Right => session.input.move_cursor(CursorMove::Forward),
                _ => {
                    let input: Input = key.into();
                    session.input.input_without_shortcuts(input);
                }
            }
        }
        self.refresh_table_sort_suggestions();
    }

    fn move_table_sort_suggestion(&mut self, delta: isize) {
        let Some(session) = self.sort_session.as_mut() else {
            return;
        };
        session.selected_suggestion = shifted_index(
            session.selected_suggestion,
            session.suggestions.len(),
            delta,
        );
    }

    fn accept_table_sort_suggestion(&mut self) {
        let Some(suggestion) = self
            .sort_session
            .as_ref()
            .and_then(|session| session.suggestions.get(session.selected_suggestion))
            .cloned()
        else {
            return;
        };
        let Some(session) = self.sort_session.as_mut() else {
            return;
        };
        let current = session.input.lines().join("\n");
        let prefix = current.get(..suggestion.replace_start).unwrap_or("");
        let updated = format!("{}{value}", prefix, value = suggestion.insertion);
        session.input = search_input(&updated);
        session.input.move_cursor(CursorMove::End);
        self.refresh_table_sort_suggestions();
    }

    fn refresh_table_sort_suggestions(&mut self) {
        let Some(expression) = self
            .sort_session
            .as_ref()
            .map(|session| session.input.lines().join("\n"))
        else {
            return;
        };
        let columns = self.table_columns();
        let suggestions = table_sort_suggestions(&expression, &columns);
        let preview = parse_sort_expression(&expression, &columns)
            .map(|sort| sort.iter().map(format_sort_spec).collect::<Vec<_>>())
            .unwrap_or_default();
        let parse_error = if expression.trim().is_empty() {
            None
        } else {
            parse_sort_expression(&expression, &columns)
                .err()
                .map(|error| error.to_string())
        };

        if let Some(session) = self.sort_session.as_mut() {
            session.suggestions = suggestions;
            session.selected_suggestion = session
                .selected_suggestion
                .min(session.suggestions.len().saturating_sub(1));
            session.parse_error = parse_error;
            session.preview = preview;
        }
    }

    fn apply_table_sort(&mut self) {
        let Some(session) = self.sort_session.as_ref() else {
            return;
        };
        let expression = session.input.lines().join("\n").trim().to_owned();
        if expression.is_empty() {
            self.sort_session = None;
            self.clear_table_sort();
            return;
        }
        let columns = self.table_columns();
        let sort = match parse_sort_expression(&expression, &columns) {
            Ok(sort) => sort,
            Err(error) => {
                self.status = format!("Ordenamiento inválido: {error}");
                return;
            }
        };

        self.table_query.sort = sort;
        self.sort_session = None;
        self.reset_table_page();
        self.load_table_query_page("Aplicando ordenamiento...");
    }

    fn clear_table_sort(&mut self) {
        if self.table_query.sort.is_empty() {
            self.status = "No hay ordenamiento activo".to_owned();
            return;
        }
        self.table_query.sort.clear();
        self.sort_session = None;
        self.reset_table_page();
        self.load_table_query_page("Limpiando ordenamiento...");
    }

    fn open_table_column_search(&mut self) {
        if self.table_page.is_none() {
            self.status = "No hay columnas disponibles para buscar".to_owned();
            return;
        }
        self.column_search_session = Some(ColumnSearchSession {
            input: TextArea::default(),
            suggestions: Vec::new(),
            selected_suggestion: 0,
        });
        self.mode = AppMode::Table;
        self.refresh_table_column_search();
        self.status = "Busca una columna · Tab completa · Enter salta · Esc cancela".to_owned();
    }

    fn handle_table_column_search_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Tab {
            self.accept_table_column_suggestion();
            return;
        }
        if key.code == KeyCode::Up {
            self.move_table_column_suggestion(-1);
            return;
        }
        if key.code == KeyCode::Down {
            self.move_table_column_suggestion(1);
            return;
        }
        if key.code == KeyCode::Enter {
            self.jump_to_selected_table_column();
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            if let Some(session) = self.column_search_session.as_mut() {
                session.input = TextArea::default();
            }
            self.refresh_table_column_search();
            return;
        }

        if let Some(session) = self.column_search_session.as_mut() {
            match key.code {
                KeyCode::Left => session.input.move_cursor(CursorMove::Back),
                KeyCode::Right => session.input.move_cursor(CursorMove::Forward),
                _ => {
                    let input: Input = key.into();
                    session.input.input_without_shortcuts(input);
                }
            }
        }
        self.refresh_table_column_search();
    }

    fn move_table_column_suggestion(&mut self, delta: isize) {
        let Some(session) = self.column_search_session.as_mut() else {
            return;
        };
        session.selected_suggestion = shifted_index(
            session.selected_suggestion,
            session.suggestions.len(),
            delta,
        );
    }

    fn accept_table_column_suggestion(&mut self) {
        let Some(suggestion) = self
            .column_search_session
            .as_ref()
            .and_then(|session| session.suggestions.get(session.selected_suggestion))
            .cloned()
        else {
            return;
        };
        if let Some(session) = self.column_search_session.as_mut() {
            session.input = search_input(&suggestion);
            session.input.move_cursor(CursorMove::End);
        }
        self.refresh_table_column_search();
    }

    fn refresh_table_column_search(&mut self) {
        let Some(query) = self
            .column_search_session
            .as_ref()
            .map(|session| session.input.lines().join("\n"))
        else {
            return;
        };
        let columns = self.display_table_columns();
        let mut matches = columns
            .iter()
            .filter_map(|column| Some((crate::search::score(column, &query)?, column.clone())))
            .collect::<Vec<_>>();
        matches.sort_by_key(|(score, _)| *score);

        if let Some(session) = self.column_search_session.as_mut() {
            session.suggestions = matches.into_iter().map(|(_, column)| column).collect();
            session.selected_suggestion = session
                .selected_suggestion
                .min(session.suggestions.len().saturating_sub(1));
        }
    }

    fn jump_to_selected_table_column(&mut self) {
        let Some(column) = self
            .column_search_session
            .as_ref()
            .and_then(|session| session.suggestions.get(session.selected_suggestion))
            .cloned()
        else {
            self.status = "No se encontró esa columna".to_owned();
            return;
        };
        let Some(index) = self
            .display_table_columns()
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(&column))
        else {
            self.status = format!("La columna ya no está disponible: {column}");
            return;
        };

        self.column_search_session = None;
        self.focus_table_column(index);
        self.status = format!("Columna enfocada: {column}");
    }

    fn current_table_preference_key(&self) -> Option<String> {
        let profile = self.current_profile()?;
        let database = self.current_database()?;
        let object = self.current_content_object.as_ref()?;
        Some(table_preference_key(
            &connection_key(profile),
            database,
            &object.owner,
            &object.name,
        ))
    }

    fn current_pinned_columns(&self) -> Vec<String> {
        self.current_table_preference_key()
            .map(|key| self.table_preferences.pinned_columns(&key).to_vec())
            .unwrap_or_default()
    }

    fn toggle_table_column_pin(&mut self) {
        let Some(column) = self
            .table_page
            .as_ref()
            .and_then(|page| page.columns.get(self.table_column_index))
            .cloned()
        else {
            self.status = "No hay una columna seleccionada".to_owned();
            return;
        };
        let Some(key) = self.current_table_preference_key() else {
            self.status = "No se pudo identificar la tabla actual".to_owned();
            return;
        };

        let mut pinned = self.table_preferences.pinned_columns(&key).to_vec();
        let was_pinned = pinned
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&column));
        if was_pinned {
            pinned.retain(|candidate| !candidate.eq_ignore_ascii_case(&column));
        } else {
            pinned.push(column.clone());
        }
        self.table_preferences.set_pinned_columns(key, pinned);
        let save_result = self.table_preferences.save();
        self.reorder_current_table_page(Some(column.clone()));
        self.status = match save_result {
            Ok(()) => {
                if was_pinned {
                    format!("Columna desfijada: {column}")
                } else {
                    format!("Columna fijada: {column}")
                }
            }
            Err(error) => format!("ERROR al guardar preferencias: {error:#}"),
        };
    }

    fn reorder_current_table_page(&mut self, selected_column: Option<String>) {
        let Some(page) = self.table_page.take() else {
            return;
        };
        let page = self.apply_table_preferences_to_page(page);
        self.table_page = Some(page);
        self.table_visual_anchor = None;
        self.table_row_visual_anchor = None;
        let selected_index = selected_column.as_deref().and_then(|column| {
            self.table_page.as_ref().and_then(|page| {
                page.columns
                    .iter()
                    .position(|candidate| candidate.eq_ignore_ascii_case(column))
            })
        });
        self.table_column_index = selected_index.unwrap_or(0);
        self.horizontal_scroll = ScrollbarState::new(
            self.table_page
                .as_ref()
                .map_or(0, |page| page.columns.len()),
        );
        self.focus_table_column(self.table_column_index);
    }

    fn focus_table_column(&mut self, index: usize) {
        let Some(column_count) = self.table_page.as_ref().map(|page| page.columns.len()) else {
            return;
        };
        if column_count == 0 {
            return;
        }

        let selected = index.min(column_count - 1);
        let visible = self.table_visible_columns.max(1).min(column_count);
        let max_offset = column_count.saturating_sub(visible);
        let position = selected
            .saturating_sub(visible.saturating_sub(1))
            .min(max_offset);
        self.table_column_index = selected;
        self.horizontal_scroll = ScrollbarState::new(column_count)
            .position(position)
            .viewport_content_length(visible);
    }

    fn display_table_columns(&self) -> Vec<String> {
        self.table_page
            .as_ref()
            .map(|page| page.columns.clone())
            .unwrap_or_default()
    }

    fn apply_table_preferences_to_page(&self, page: TablePage) -> TablePage {
        let pinned = self.current_pinned_columns();
        reorder_table_page(page, &pinned)
    }

    fn reset_table_page(&mut self) {
        self.table_query.page.cursor = None;
        self.clear_table_cell_editing();
        self.table_row_deletions.clear();
        self.table_new_rows.clear();
        self.table_delete_sequence_armed = false;
        self.table_row_visual_anchor = None;
        self.table_page = None;
        self.table_state = TableState::default();
        self.table_column_index = 0;
        self.horizontal_scroll = ScrollbarState::new(0);
        self.table_loading_more = false;
    }

    fn table_columns(&self) -> Vec<String> {
        self.table_metadata
            .as_ref()
            .map(|metadata| {
                metadata
                    .columns
                    .iter()
                    .map(|column| column.name.clone())
                    .collect()
            })
            .or_else(|| self.table_page.as_ref().map(|page| page.columns.clone()))
            .unwrap_or_default()
    }

    fn open_table_copy_menu(&mut self) {
        self.table_copy_stage = Some(TableCopyStage::Menu);
        self.table_copy_source = None;
        self.table_copy_menu_index = 0;
        self.status = "Elige qué copiar".to_owned();
    }

    fn open_table_header_choice(&mut self, source: TableCopySource) {
        self.table_copy_stage = Some(TableCopyStage::HeaderChoice);
        self.table_copy_source = Some(source);
        self.status = "¿Copiar también la cabecera? y/n".to_owned();
    }

    fn handle_table_copy_key(&mut self, key: KeyEvent) {
        match self.table_copy_stage {
            Some(TableCopyStage::Menu) => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.table_copy_menu_index = (self.table_copy_menu_index + 1).min(2);
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.table_copy_menu_index = self.table_copy_menu_index.saturating_sub(1);
                }
                KeyCode::Enter => {
                    let source = match self.table_copy_menu_index {
                        0 => TableCopySource::LoadedData,
                        1 => TableCopySource::CurrentRow,
                        _ => TableCopySource::CurrentColumn,
                    };
                    self.open_table_header_choice(source);
                }
                _ => {}
            },
            Some(TableCopyStage::HeaderChoice) => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(source) = self.table_copy_source.take() {
                        self.table_copy_stage = None;
                        self.copy_table_source(source, true);
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Enter => {
                    if let Some(source) = self.table_copy_source.take() {
                        self.table_copy_stage = None;
                        self.copy_table_source(source, false);
                    }
                }
                _ => {}
            },
            None => {}
        }
    }

    fn copy_table_metadata(&mut self) {
        let Some(metadata) = self.table_metadata.as_ref() else {
            self.status = "No hay metadata para copiar".to_owned();
            return;
        };

        let mut lines = vec!["ordinal\tcolumn\ttype\tnullable".to_owned()];
        lines.extend(metadata.columns.iter().map(|column| {
            format!(
                "{}\t{}\t{}\t{}",
                column.ordinal_position,
                column.name,
                format_table_type(column),
                if column.nullable { "NULL" } else { "NOT NULL" }
            )
        }));
        lines.push(String::new());
        lines.push("index\tcolumns\tunique\tprimary".to_owned());
        lines.extend(metadata.indexes.iter().map(|index| {
            format!(
                "{}\t{}\t{}\t{}",
                index.name,
                index.columns.join(", "),
                index.is_unique,
                index.is_primary
            )
        }));
        self.copy_text(lines.join("\n"), "Metadata copiada");
    }

    fn copy_table_source(&mut self, source: TableCopySource, include_header: bool) {
        let Some(page) = self.table_page.clone() else {
            self.status = "No hay datos cargados para copiar".to_owned();
            return;
        };
        let column = self
            .table_column_index
            .min(page.columns.len().saturating_sub(1));
        let text = match source {
            TableCopySource::VisualRows => table_copy_rows_text(
                &page,
                self.table_row_visual_anchor,
                self.table_state.selected(),
                include_header,
            ),
            _ => table_copy_text(
                &page,
                source,
                self.table_state.selected(),
                column,
                self.table_visual_anchor,
                include_header,
            ),
        };
        let Some(text) = text else {
            self.status = match source {
                TableCopySource::CurrentCell | TableCopySource::CurrentRow => {
                    "No hay una fila seleccionada".to_owned()
                }
                TableCopySource::VisualSelection => "No hay una selección visual activa".to_owned(),
                TableCopySource::VisualRows => "No hay una selección de filas activa".to_owned(),
                _ => "No hay datos cargados para copiar".to_owned(),
            };
            return;
        };
        let label = match source {
            TableCopySource::LoadedData => "Datos copiados",
            TableCopySource::CurrentCell => "Celda copiada",
            TableCopySource::CurrentRow => "Fila copiada",
            TableCopySource::CurrentColumn => "Columna copiada",
            TableCopySource::VisualSelection => "Selección copiada",
            TableCopySource::VisualRows => "Filas seleccionadas copiadas",
        };
        self.copy_text(text, label);
        self.table_visual_anchor = None;
        self.table_row_visual_anchor = None;
        self.table_copy_stage = None;
        self.table_copy_source = None;
    }

    fn copy_text(&mut self, text: String, label: &str) {
        match services::clipboard::copy_text(&text) {
            Ok(()) => self.status = format!("{label} al portapapeles"),
            Err(error) => self.status = format!("ERROR al copiar al portapapeles: {error}"),
        }
    }
    fn handle_search_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.flush_search_refresh();
            self.search = None;
            self.mode = self.search_return_mode;
            return;
        }

        if key.code == KeyCode::Tab {
            self.flush_search_refresh();
            self.accept_search_suggestion();
            return;
        }

        if key.code == KeyCode::Up {
            self.flush_search_refresh();
            self.move_search_suggestion(-1);
            return;
        }

        if key.code == KeyCode::Down {
            self.flush_search_refresh();
            self.move_search_suggestion(1);
            return;
        }

        if key.code == KeyCode::Enter {
            self.flush_search_refresh();
            let Some(session) = self.search.take() else {
                self.mode = self.search_return_mode;
                return;
            };

            let query = session.input.lines().join("\n").trim().to_owned();
            let return_mode = self.search_return_mode;

            if query.is_empty() {
                self.mode = return_mode;
                return;
            }

            self.active_search = Some(query);
            if let Some(entry) = session
                .suggestions
                .get(session.selected_suggestion)
                .cloned()
            {
                if return_mode == AppMode::Table && self.has_pending_table_changes() {
                    self.pending_table_navigation = Some(entry);
                    self.confirm_action = Some(ConfirmAction::DiscardTableChanges);
                    self.confirm_message = table_exit_message(
                        self.table_cell_drafts.len(),
                        self.table_row_deletions.len(),
                        self.table_new_rows.len(),
                    );
                    self.mode = AppMode::Confirm;
                    return;
                }
                self.mode = AppMode::Browser;
                self.navigate_to_catalog_entry(entry);
            } else {
                self.mode = return_mode;
                self.status = "No hay coincidencias en el catálogo".to_owned();
            }
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            if let Some(session) = self.search.as_mut() {
                session.input = TextArea::default();
                session.selected_suggestion = 0;
                session.refresh_deadline = None;
            }

            self.active_search = None;
            self.refresh_search_suggestions();
            return;
        }

        if let Some(session) = self.search.as_mut() {
            let input: Input = key.into();
            session.input.input(input);
        }

        self.schedule_search_refresh();
    }

    fn move_search_suggestion(&mut self, delta: isize) {
        let Some(session) = self.search.as_mut() else {
            return;
        };

        if session.suggestions.is_empty() {
            return;
        }

        session.selected_suggestion = shifted_index(
            session.selected_suggestion,
            session.suggestions.len(),
            delta,
        );
    }

    fn accept_search_suggestion(&mut self) {
        let suggestion = self.search.as_ref().and_then(|session| {
            session
                .suggestions
                .get(session.selected_suggestion)
                .cloned()
        });

        let Some(suggestion) = suggestion else {
            return;
        };

        let mut input = search_input(&suggestion.path());
        input.move_cursor(CursorMove::End);

        if let Some(session) = self.search.as_mut() {
            session.input = input;
        }

        self.refresh_search_suggestions();
    }

    fn refresh_search_suggestions(&mut self) {
        let Some(query) = self
            .search
            .as_ref()
            .map(|session| session.input.lines().join("\n"))
        else {
            return;
        };

        let suggestions = self.search_suggestions(&query);

        if let Some(session) = self.search.as_mut() {
            session.suggestions = suggestions;
            session.selected_suggestion = session
                .selected_suggestion
                .min(session.suggestions.len().saturating_sub(1));
        }
    }

    fn schedule_search_refresh(&mut self) {
        if let Some(session) = self.search.as_mut() {
            session.refresh_deadline = Some(Instant::now() + Duration::from_millis(120));
        }
    }

    fn flush_search_refresh_if_due(&mut self) {
        let due = self
            .search
            .as_ref()
            .and_then(|session| session.refresh_deadline)
            .is_some_and(|deadline| Instant::now() >= deadline);
        if due {
            self.flush_search_refresh();
        }
    }

    fn flush_search_refresh(&mut self) {
        let scheduled = self
            .search
            .as_mut()
            .and_then(|session| session.refresh_deadline.take())
            .is_some();
        if scheduled {
            self.refresh_search_suggestions();
        }
    }

    fn search_suggestions(&self, query: &str) -> Vec<CatalogEntry> {
        let query = query.trim();
        let prepared_query = crate::search::prepare_query(query);
        let mut matches = self
            .search_index
            .iter()
            .filter_map(|entry| {
                if query.is_empty() {
                    return Some((None, entry.entry.clone()));
                }
                let score = crate::search::best_prepared_match_score(
                    [&entry.search_text, &entry.path],
                    &prepared_query,
                )?;
                Some((Some(score), entry.entry.clone()))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(score, _)| *score);
        matches
            .into_iter()
            .map(|(_, entry)| entry)
            .take(8)
            .collect()
    }

    fn navigate_to_catalog_entry(&mut self, entry: CatalogEntry) {
        let Some(connection_index) = self
            .config
            .connections
            .iter()
            .position(|profile| connection_key(profile) == entry.connection_key)
        else {
            self.status = "La conexión del catálogo ya no existe".to_owned();
            return;
        };

        self.connection_index = connection_index;

        let Some(database_index) = self
            .databases
            .iter()
            .position(|database| database == &entry.database)
        else {
            self.pending_catalog_target = Some(entry);
            self.database_index = 0;
            self.load_databases();
            self.status = "Cargando la base del resultado...".to_owned();
            return;
        };
        self.database_index = database_index;

        let Some(kind) = entry.object_kind() else {
            self.focus = Focus::Databases;
            self.status = format!("Base seleccionada: {}", entry.database);
            return;
        };
        self.kind_index = ObjectKind::ALL
            .iter()
            .position(|candidate| *candidate == kind)
            .unwrap_or(0);

        let object = DbObject {
            owner: entry.owner.unwrap_or_else(|| "dbo".to_owned()),
            name: entry.name,
            kind,
        };

        if self.objects.iter().any(|candidate| candidate == &object) {
            self.object_index = self
                .objects
                .iter()
                .position(|candidate| candidate == &object)
                .unwrap_or(0);
            self.focus = Focus::Objects;
            self.open_selected_object(object);
        } else {
            self.pending_catalog_target = Some(CatalogEntry {
                owner: Some(object.owner),
                name: object.name,
                ..entry
            });
            self.objects.clear();
            self.object_index = 0;
            self.load_objects();
            self.status = "Cargando el objeto del resultado...".to_owned();
        }
    }

    fn open_selected_object(&mut self, object: DbObject) {
        if object.kind == ObjectKind::Table {
            self.preview_selected_table();
        } else {
            self.load_definition(false);
        }
    }

    fn maybe_refresh_catalog(&mut self, force: bool) {
        if !force && self.last_catalog_refresh_check.elapsed() < Duration::from_secs(30) {
            return;
        }
        self.last_catalog_refresh_check = Instant::now();

        let refresh_indices = self
            .config
            .connections
            .iter()
            .enumerate()
            .filter_map(|(index, profile)| {
                let needs_refresh = force
                    || self
                        .catalog
                        .needs_refresh_for(profile, self.config.catalog_ttl_hours);
                (needs_refresh && !self.catalog_refresh_pending.contains(&index)).then_some(index)
            })
            .collect::<Vec<_>>();

        for index in refresh_indices {
            self.request_catalog_refresh(index);
        }
    }

    fn refresh_catalog(&mut self) {
        self.maybe_refresh_catalog(true);
        self.status = "Actualizando catálogo...".to_owned();
    }

    fn request_catalog_refresh(&mut self, connection_index: usize) {
        let Some(profile) = self.config.connections.get(connection_index).cloned() else {
            return;
        };
        let request_id =
            self.begin_request(format!("Actualizando catálogo de {}...", profile.name));
        self.catalog_refresh_pending.insert(connection_index);
        self.send(WorkerRequest::RefreshCatalog {
            request_id,
            connection_index,
            profile,
        });
    }

    fn handle_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let Some(action) = self.confirm_action.take() else {
                    self.mode = AppMode::Browser;
                    return;
                };
                match action {
                    ConfirmAction::Execute { sql } => self.dispatch_execute(sql),
                    ConfirmAction::ExecuteTableChanges { sql } => self.dispatch_table_changes(sql),
                    ConfirmAction::DiscardEditor => {
                        self.editor = None;
                        self.mode = AppMode::Browser;
                        self.status = "Cambios descartados".to_owned();
                    }
                    ConfirmAction::DiscardTableChanges => {
                        let navigation = self.pending_table_navigation.take();
                        self.exit_table_mode();
                        if let Some(entry) = navigation {
                            self.navigate_to_catalog_entry(entry);
                        } else {
                            self.status = "Cambios de tabla descartados".to_owned();
                        }
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc | KeyCode::Enter => {
                let return_to_table = self.confirm_action.as_ref().is_some_and(|action| {
                    matches!(
                        action,
                        ConfirmAction::DiscardTableChanges
                            | ConfirmAction::ExecuteTableChanges { .. }
                    )
                });
                self.confirm_action = None;
                self.return_to_editor_after_execution = false;
                self.return_to_table_after_execution = false;
                if return_to_table {
                    self.pending_table_navigation = None;
                }
                self.mode = if return_to_table {
                    AppMode::Table
                } else if self.editor.is_some() {
                    AppMode::Editor
                } else {
                    AppMode::Browser
                };
                self.status = "Operación cancelada".to_owned();
            }
            _ => {}
        }
    }

    fn move_selection(&mut self, delta: isize) {
        match self.focus {
            Focus::Connections => {
                self.connection_index =
                    shifted_index(self.connection_index, self.config.connections.len(), delta);
            }
            Focus::Databases => {
                self.database_index =
                    shifted_index(self.database_index, self.databases.len(), delta);
            }
            Focus::Kinds => {
                self.kind_index = shifted_index(self.kind_index, ObjectKind::ALL.len(), delta);
            }
            Focus::Objects => {
                self.object_index = shifted_index(self.object_index, self.objects.len(), delta);
            }
            Focus::Content => {
                if delta < 0 {
                    self.content_scroll = self
                        .content_scroll
                        .saturating_sub(delta.unsigned_abs() as u16);
                } else {
                    self.content_scroll = self.content_scroll.saturating_add(delta as u16);
                }
            }
        }
    }

    fn move_to_edge(&mut self, end: bool) {
        let edge = |len: usize| if end { len.saturating_sub(1) } else { 0 };
        match self.focus {
            Focus::Connections => self.connection_index = edge(self.config.connections.len()),
            Focus::Databases => self.database_index = edge(self.databases.len()),
            Focus::Kinds => self.kind_index = edge(ObjectKind::ALL.len()),
            Focus::Objects => self.object_index = edge(self.objects.len()),
            Focus::Content => {
                self.content_scroll = if end { u16::MAX } else { 0 };
            }
        }
    }

    fn activate_selection(&mut self) {
        match self.focus {
            Focus::Connections => {
                self.databases.clear();
                self.objects.clear();
                self.database_index = 0;
                self.object_index = 0;
                self.test_connection();
                self.load_databases();
            }
            Focus::Databases | Focus::Kinds => self.load_objects(),
            Focus::Objects => {
                if self.current_kind() == ObjectKind::Table {
                    self.preview_selected_table();
                } else {
                    self.load_definition(false);
                }
            }
            Focus::Content => {}
        }
    }

    fn refresh_current(&mut self) {
        match self.focus {
            Focus::Connections => {
                self.test_connection();
                self.load_databases();
            }
            Focus::Databases | Focus::Kinds => self.load_objects(),
            Focus::Objects | Focus::Content => {
                if self.current_object().is_some() {
                    self.load_definition(false);
                } else {
                    self.load_objects();
                }
            }
        }
    }

    fn reload_connections(&mut self) {
        match AppConfig::load() {
            Ok((config, path)) => {
                let selected_name = self.current_profile().map(|p| p.name.clone());
                self.config = config;
                self.config_path = path;
                let selected_raw_index = selected_name
                    .and_then(|name| self.config.connections.iter().position(|p| p.name == name))
                    .unwrap_or(0);
                self.connection_index = selected_raw_index;
                self.databases.clear();
                self.objects.clear();
                self.status = "Conexiones recargadas desde disco".to_owned();
                self.test_connection();
                self.load_databases();
            }
            Err(error) => {
                self.set_error(format!("No se pudo recargar la configuración: {error:#}"))
            }
        }
    }

    fn test_connection(&mut self) {
        let Some(profile) = self.current_profile().cloned() else {
            self.set_error("No hay una conexión seleccionada".to_owned());
            return;
        };
        let connection_index = self.connection_index;
        let request_id = self.begin_request("Probando conexión...");
        self.send(WorkerRequest::TestConnection {
            request_id,
            connection_index,
            profile,
        });
    }

    fn load_databases(&mut self) {
        let Some(profile) = self.current_profile().cloned() else {
            return;
        };
        let connection_index = self.connection_index;
        let request_id = self.begin_request("Cargando bases de datos...");
        self.send(WorkerRequest::LoadDatabases {
            request_id,
            connection_index,
            profile,
        });
    }

    fn load_objects(&mut self) {
        let Some(profile) = self.current_profile().cloned() else {
            return;
        };
        let connection_index = self.connection_index;
        let Some(database) = self.current_database().map(ToOwned::to_owned) else {
            self.set_error("Selecciona una base de datos".to_owned());
            return;
        };
        let kind = self.current_kind();
        let request_id = self.begin_request(format!("Cargando {kind} de {database}..."));
        self.send(WorkerRequest::LoadObjects {
            request_id,
            connection_index,
            database,
            kind,
            profile,
        });
    }

    fn load_definition(&mut self, open_editor: bool) {
        let Some(profile) = self.current_profile().cloned() else {
            return;
        };
        let connection_index = self.connection_index;
        let Some(database) = self.current_database().map(ToOwned::to_owned) else {
            return;
        };
        let Some(object) = self.current_object().cloned() else {
            self.set_error("Selecciona un objeto".to_owned());
            return;
        };
        let request_id = self.begin_request(format!("Leyendo {}...", object.qualified_name()));
        if open_editor {
            self.open_editor_after_definition = Some((request_id, object.clone()));
        }
        self.send(WorkerRequest::LoadDefinition {
            request_id,
            connection_index,
            database,
            object,
            profile,
        });
    }

    fn preview_selected_table(&mut self) {
        let Some(object) = self.current_object().cloned() else {
            self.set_error("Selecciona una tabla".to_owned());
            return;
        };
        if object.kind != ObjectKind::Table {
            self.set_error("La vista previa con 'p' solo aplica a tablas".to_owned());
            return;
        }
        let Some(profile) = self.current_profile().cloned() else {
            return;
        };
        let connection_index = self.connection_index;
        let Some(database) = self.current_database().map(ToOwned::to_owned) else {
            return;
        };
        let request_id = self.begin_request(format!("Consultando {}...", object.qualified_name()));
        self.clear_table_cell_editing();
        self.table_metadata = None;
        self.table_page = None;
        self.table_query = TableQuery::new(PageRequest::default());
        self.table_filter_expression.clear();
        self.filter_session = None;
        self.sort_session = None;
        self.column_search_session = None;
        self.table_loading_more = false;
        self.current_content_object = Some(object.clone());
        self.send(WorkerRequest::LoadTableMetadata {
            request_id,
            connection_index,
            database,
            object,
            profile,
        });
    }

    fn load_first_table_page(&mut self, object: DbObject) {
        let Some(profile) = self.current_profile().cloned() else {
            return;
        };
        let connection_index = self.connection_index;
        let Some(database) = self.current_database().map(ToOwned::to_owned) else {
            return;
        };
        let request_id = self.begin_request(format!("Consultando {}...", object.qualified_name()));
        self.clear_table_cell_editing();
        self.table_query = TableQuery::default();
        self.sort_session = None;
        self.column_search_session = None;
        self.send(WorkerRequest::QueryTable {
            request_id,
            connection_index,
            database,
            object,
            query: self.table_query.clone(),
            profile,
        });
    }

    fn load_table_query_page(&mut self, status: &str) {
        let Some(object) = self.current_content_object.clone() else {
            self.status = "No hay una tabla activa".to_owned();
            return;
        };
        let Some(profile) = self.current_profile().cloned() else {
            return;
        };
        let Some(database) = self.current_database().map(ToOwned::to_owned) else {
            return;
        };
        let request_id = self.begin_request(status);
        self.send(WorkerRequest::QueryTable {
            request_id,
            connection_index: self.connection_index,
            database,
            object,
            query: self.table_query.clone(),
            profile,
        });
    }

    fn load_next_table_page(&mut self) {
        let Some(page) = self.table_page.as_ref() else {
            return;
        };
        let Some(cursor) = page.next_cursor.clone() else {
            return;
        };
        let Some(object) = self.current_content_object.clone() else {
            return;
        };
        let Some(profile) = self.current_profile().cloned() else {
            return;
        };
        let Some(database) = self.current_database().map(ToOwned::to_owned) else {
            return;
        };
        self.table_query.page.cursor = Some(cursor);
        let request_id = self.begin_request("Cargando más filas...");
        self.table_loading_more = true;
        self.send(WorkerRequest::QueryTable {
            request_id,
            connection_index: self.connection_index,
            database,
            object,
            query: self.table_query.clone(),
            profile,
        });
    }

    fn edit_selected_object(&mut self) {
        let Some(object) = self.current_object().cloned() else {
            self.set_error("Selecciona un procedimiento, función o vista".to_owned());
            return;
        };
        if !object.kind.editable() {
            self.set_error(
                "Para tablas usa 'E' y edita valores mediante T-SQL protegido".to_owned(),
            );
            return;
        }
        if self.current_content_object.as_ref() == Some(&object) && !self.content.is_empty() {
            self.open_object_editor(object, self.content.clone());
        } else {
            self.load_definition(true);
        }
    }

    fn edit_table_values(&mut self) {
        let Some(object) = self.current_object().cloned() else {
            self.set_error("Selecciona una tabla".to_owned());
            return;
        };
        if object.kind != ObjectKind::Table {
            self.set_error("'E' está reservado para editar valores de tablas".to_owned());
            return;
        }
        let qualified = sybase::queries::qualified_identifier(&object.owner, &object.name);
        let sql = format!(
            "-- Edición transaccional de valores en {}\n\
             -- Sustituye columna, valor y WHERE. No quites el WHERE.\n\
             set quoted_identifier on\n\
             begin tran\n\n\
             update {}\n\
                set columna = valor\n\
              where condicion_unica = valor\n\n\
             -- Revisa el resultado antes de cambiar rollback por commit.\n\
             rollback tran\n",
            object.qualified_name(),
            qualified
        );
        self.editor = Some(EditorSession {
            title: format!("Editar valores · {}", object.qualified_name()),
            purpose: EditorPurpose::TableValues(object),
            editor: VimEditor::new(sql),
        });
        self.content_title = "Resultado de ejecución".to_owned();
        self.content.clear();
        self.content_scroll = 0;
        self.mode = AppMode::Editor;
        self.status = "Editor NORMAL · i para insertar · Ctrl+S para ejecutar".to_owned();
    }

    fn open_query_editor(&mut self) {
        let database = self.active_database().unwrap_or("master");
        let template = format!(
            "-- T-SQL en {database}\n-- Ctrl+S ejecuta; las escrituras requieren confirmación.\n\nselect db_name() as base_actual\n"
        );
        self.editor = Some(EditorSession {
            title: format!("Consulta T-SQL · {database}"),
            purpose: EditorPurpose::Query,
            editor: VimEditor::new(template),
        });
        self.content_title = "Resultado de ejecución".to_owned();
        self.content.clear();
        self.content_scroll = 0;
        self.mode = AppMode::Editor;
        self.status = "Editor NORMAL · i para insertar · Ctrl+S para ejecutar".to_owned();
    }

    fn open_object_editor(&mut self, object: DbObject, definition: String) {
        let definition = normalize_definition_for_edit(&definition);
        self.editor = Some(EditorSession {
            title: format!("Editar {} · {}", object.kind, object.qualified_name()),
            purpose: EditorPurpose::ObjectDefinition(object),
            editor: VimEditor::new(definition),
        });
        self.content_title = "Resultado de ejecución".to_owned();
        self.content.clear();
        self.content_scroll = 0;
        self.mode = AppMode::Editor;
        self.status = "Editor NORMAL · Ctrl+S propone guardar · Esc cierra".to_owned();
    }

    fn save_editor(&mut self) {
        let Some((sql, purpose)) = self
            .editor
            .as_ref()
            .map(|session| (session.editor.text(), session.purpose.clone()))
        else {
            return;
        };
        if sql.trim().is_empty() {
            self.set_error("El editor está vacío".to_owned());
            return;
        }

        let write = is_write_sql(&sql);
        let allow_writes = self
            .current_profile()
            .is_some_and(|profile| profile.allow_writes);
        if write && !allow_writes {
            self.set_error(
                "Escritura bloqueada: cambia allow_writes = true para esta conexión".to_owned(),
            );
            return;
        }

        self.return_to_editor_after_execution = true;
        if write {
            self.confirm_action = Some(ConfirmAction::Execute { sql });
            self.confirm_message = match &purpose {
                EditorPurpose::ObjectDefinition(object) => format!(
                    "Se ejecutará DDL para {}. ¿Confirmar? [y/N]",
                    object.qualified_name()
                ),
                EditorPurpose::TableValues(object) => format!(
                    "Se modificarán datos de {}. Verifica WHERE/TRAN. ¿Ejecutar? [y/N]",
                    object.qualified_name()
                ),
                EditorPurpose::Query => {
                    "La consulta contiene una operación de escritura. ¿Ejecutar? [y/N]".to_owned()
                }
            };
            self.mode = AppMode::Confirm;
        } else {
            self.dispatch_execute(sql);
        }
    }

    fn dispatch_execute(&mut self, sql: String) {
        self.return_to_table_after_execution = false;
        let Some(profile) = self.current_profile().cloned() else {
            return;
        };
        let connection_index = self.connection_index;
        let database = self
            .active_database()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| profile.initial_database().to_owned());
        let request_id = self.begin_request(format!("Ejecutando T-SQL en {database}..."));
        self.send(WorkerRequest::ExecuteSql {
            request_id,
            connection_index,
            database,
            sql,
            profile,
        });
        self.mode = if self.editor.is_some() {
            AppMode::Editor
        } else {
            AppMode::Browser
        };
    }

    fn dispatch_table_changes(&mut self, sql: String) {
        if !self
            .current_profile()
            .is_some_and(|profile| profile.allow_writes)
        {
            self.status =
                "Escritura bloqueada: cambia allow_writes = true para esta conexión".to_owned();
            self.mode = AppMode::Table;
            return;
        }
        if !contains_executable_table_sql(&sql) {
            self.status = "Ejecución bloqueada: no hay SQL ejecutable".to_owned();
            self.mode = AppMode::Table;
            return;
        }
        let Some(profile) = self.current_profile().cloned() else {
            self.status = "No hay una conexión seleccionada".to_owned();
            self.mode = AppMode::Table;
            return;
        };
        let connection_index = self.connection_index;
        let database = self
            .active_database()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| profile.initial_database().to_owned());
        let request_id = self.begin_request(format!("Ejecutando cambios staged en {database}..."));
        let request = WorkerRequest::ExecuteSql {
            request_id,
            connection_index,
            database,
            sql,
            profile,
        };
        self.return_to_table_after_execution = true;
        if let Err(error) = self.request_tx.send(request) {
            self.pending_requests.remove(&request_id);
            self.busy_count = self.pending_requests.len();
            self.return_to_table_after_execution = false;
            self.status = format!("ERROR: el worker de base de datos terminó: {error}");
            self.mode = AppMode::Table;
        } else {
            self.mode = AppMode::Table;
        }
    }

    fn handle_table_changes_execution_result(
        &mut self,
        database: String,
        result: Result<SqlOutput, String>,
    ) {
        match result {
            Ok(output) => {
                let content = output.combined();
                let committed = table_execution_committed(&output);
                self.content_title = format!("Resultado · {database}");
                self.content_scroll = 0;
                self.content = content.clone();
                self.highlighted_content = None;
                if committed {
                    self.reset_table_page();
                    self.table_show_metadata = false;
                    self.mode = AppMode::Table;
                    self.status =
                        "Cambios ejecutados y confirmados · recargando tabla...".to_owned();
                    self.load_table_query_page("Recargando tabla después del commit...");
                } else {
                    self.mode = AppMode::Table;
                    self.status = if table_execution_rolled_back(&output) {
                        "Transacción revertida · los cambios staged se conservaron".to_owned()
                    } else if output.success {
                        "No se confirmó el commit · los cambios staged se conservaron".to_owned()
                    } else {
                        "ASE/isql devolvió un error · los cambios staged se conservaron".to_owned()
                    };
                }
            }
            Err(error) => {
                self.content_title = "Error de ejecución".to_owned();
                self.content_scroll = 0;
                self.content = error.clone();
                self.highlighted_content = None;
                self.mode = AppMode::Table;
                self.status = format!(
                    "ERROR: {} · los cambios staged se conservaron",
                    first_line(&error)
                );
            }
        }
    }

    fn handle_worker_response(&mut self, response: WorkerResponse) {
        let request_id = match &response {
            WorkerResponse::ConnectionTested { request_id, .. }
            | WorkerResponse::DatabasesLoaded { request_id, .. }
            | WorkerResponse::ObjectsLoaded { request_id, .. }
            | WorkerResponse::DefinitionLoaded { request_id, .. }
            | WorkerResponse::TablePreviewed { request_id, .. }
            | WorkerResponse::TableMetadataLoaded { request_id, .. }
            | WorkerResponse::TablePageLoaded { request_id, .. }
            | WorkerResponse::SqlExecuted { request_id, .. }
            | WorkerResponse::CatalogRefreshed { request_id, .. } => *request_id,
        };
        self.pending_requests.remove(&request_id);

        match response {
            WorkerResponse::ConnectionTested {
                connection_index,
                result,
                ..
            } => {
                if self.connection_index != connection_index {
                    return;
                }
                match result {
                    Ok(server) => self.status = format!("Conectado: {server}"),
                    Err(error) => self.set_error(error),
                }
            }
            WorkerResponse::DatabasesLoaded {
                connection_index,
                result,
                ..
            } => {
                if self.connection_index != connection_index {
                    return;
                }
                match result {
                    Ok(databases) => {
                        let preferred = self
                            .current_profile()
                            .map(|profile| profile.initial_database().to_owned());
                        self.databases = databases;
                        let preferred_raw_index = preferred
                            .and_then(|name| self.databases.iter().position(|db| db == &name))
                            .unwrap_or(0);
                        self.database_index = preferred_raw_index;
                        self.status = format!("{} bases disponibles", self.databases.len());
                        if let Some(target) = self.pending_catalog_target.take() {
                            self.navigate_to_catalog_entry(target);
                        } else if !self.databases.is_empty() {
                            self.load_objects();
                        }
                    }
                    Err(error) => self.set_error(error),
                }
            }
            WorkerResponse::ObjectsLoaded {
                connection_index,
                database,
                kind,
                result,
                ..
            } => {
                if self.connection_index != connection_index
                    || self.current_database() != Some(database.as_str())
                    || self.current_kind() != kind
                {
                    return;
                }
                match result {
                    Ok(objects) => {
                        self.objects = objects;
                        self.object_index = 0;
                        self.current_content_object = None;
                        self.content_title = format!("{kind} · {database}");
                        self.content_scroll = 0;
                        self.content = format!(
                            "{} objetos encontrados. Enter abre la tabla o definición; 'e' edita.",
                            self.objects.len()
                        );
                        self.highlighted_content = None;
                        self.table_page = None;
                        self.table_metadata = None;
                        self.clear_table_cell_editing();
                        self.table_filter_expression.clear();
                        self.filter_session = None;
                        self.sort_session = None;
                        self.column_search_session = None;

                        self.status = format!("{} {} cargados", self.objects.len(), kind);
                        if let Some(target) = self.pending_catalog_target.take() {
                            self.navigate_to_catalog_entry(target);
                        }
                    }
                    Err(error) => self.set_error(error),
                }
            }
            WorkerResponse::DefinitionLoaded {
                request_id,
                connection_index,
                database,
                object,
                result,
            } => {
                let requested_editor = self.open_editor_after_definition.as_ref().is_some_and(
                    |(pending_id, pending_object)| {
                        *pending_id == request_id && pending_object == &object
                    },
                );
                if self.connection_index != connection_index
                    || self.current_database() != Some(database.as_str())
                {
                    if requested_editor {
                        self.open_editor_after_definition = None;
                    }
                    return;
                }
                match result {
                    Ok(definition) => {
                        self.content_title =
                            format!("{} · {}", object.kind, object.qualified_name());
                        self.content_scroll = 0;
                        self.content = definition.clone();
                        self.highlighted_content = Some(ui::highlight_sql(&definition));
                        self.table_page = None;
                        self.table_metadata = None;
                        self.clear_table_cell_editing();
                        self.table_filter_expression.clear();
                        self.filter_session = None;
                        self.sort_session = None;
                        self.column_search_session = None;
                        self.table_show_metadata = false;
                        self.current_content_object = Some(object.clone());
                        self.status = format!("Definición cargada: {}", object.qualified_name());
                        if requested_editor {
                            self.open_editor_after_definition = None;
                            self.open_object_editor(object, definition);
                        }
                    }
                    Err(error) => {
                        if requested_editor {
                            self.open_editor_after_definition = None;
                        }
                        self.set_error(error);
                    }
                }
            }
            WorkerResponse::TablePreviewed {
                connection_index,
                database,
                object,
                result,
                ..
            } => {
                if self.connection_index != connection_index
                    || self.current_database() != Some(database.as_str())
                    || self.current_content_object.as_ref() != Some(&object)
                {
                    return;
                }
                match result {
                    Ok(output) => {
                        let page =
                            match TablePage::new(output.columns, output.rows, None, false, None) {
                                Ok(page) => page,
                                Err(error) => {
                                    self.set_error(error.to_string());
                                    return;
                                }
                            };
                        let page = self.apply_table_preferences_to_page(page);
                        self.clear_table_cell_editing();
                        self.content_title = format!("Datos · {}", object.qualified_name());
                        self.content_scroll = 0;
                        self.horizontal_scroll = ScrollbarState::new(page.columns.len());
                        self.table_state = TableState::default();
                        self.table_column_index = 0;
                        self.table_visible_columns = 0;
                        if !page.rows.is_empty() {
                            self.table_state.select(Some(0));
                        }
                        self.table_page = Some(page);
                        self.table_metadata = None;
                        self.table_filter_expression.clear();
                        self.filter_session = None;
                        self.sort_session = None;
                        self.column_search_session = None;
                        self.table_show_metadata = false;
                        self.current_content_object = Some(object);
                        self.highlighted_content = None;
                        self.mode = AppMode::Table;
                        self.status = "Tabla cargada".to_owned();
                    }
                    Err(error) => self.set_error(error),
                }
            }
            WorkerResponse::TableMetadataLoaded {
                request_id,
                connection_index,
                database,
                object,
                result,
            } => {
                if self.connection_index != connection_index
                    || self.current_database() != Some(database.as_str())
                    || self.current_content_object.as_ref() != Some(&object)
                {
                    return;
                }

                match result {
                    Ok(metadata) => {
                        let column_count = metadata.columns.len();
                        let index_count = metadata.indexes.len();
                        self.table_metadata = Some(metadata);
                        self.status = format!(
                            "Metadata cargada · {column_count} columnas · {index_count} índices"
                        );
                        self.load_first_table_page(object);
                    }
                    Err(error) => self.set_error(error),
                }
                self.pending_requests.remove(&request_id);
            }
            WorkerResponse::TablePageLoaded {
                connection_index,
                database,
                object,
                result,
                ..
            } => {
                if self.connection_index != connection_index
                    || self.current_database() != Some(database.as_str())
                {
                    return;
                }

                match result {
                    Ok(page) => {
                        let page = self.apply_table_preferences_to_page(page);
                        let loading_more = self.table_loading_more;
                        if loading_more {
                            let Some(current_page) = self.table_page.as_mut() else {
                                self.set_error(
                                    "No hay una página base para agregar filas".to_owned(),
                                );
                                self.table_loading_more = false;
                                return;
                            };
                            if current_page.columns != page.columns {
                                self.set_error(
                                    "La página recibida tiene columnas incompatibles".to_owned(),
                                );
                                self.table_loading_more = false;
                                return;
                            }
                            current_page.rows.extend(page.rows);
                            current_page.next_cursor = page.next_cursor;
                            current_page.has_more = page.has_more;
                            current_page.total_rows = page.total_rows;
                        } else {
                            self.clear_table_cell_editing();
                            self.horizontal_scroll = ScrollbarState::new(page.columns.len());
                            self.table_state = TableState::default();
                            self.table_column_index = 0;
                            self.table_visible_columns = 0;
                            if !page.rows.is_empty() {
                                self.table_state.select(Some(0));
                            }
                            self.table_page = Some(page);
                        }
                        self.table_show_metadata = false;
                        self.table_loading_more = false;
                        self.content_title = format!("Datos · {}", object.qualified_name());
                        self.current_content_object = Some(object.clone());
                        self.highlighted_content = None;
                        self.mode = AppMode::Table;
                        self.status = if loading_more {
                            "Más filas cargadas".to_owned()
                        } else if !self.table_filter_expression.is_empty()
                            && !self.table_query.sort.is_empty()
                        {
                            "Tabla filtrada y ordenada".to_owned()
                        } else if !self.table_filter_expression.is_empty() {
                            "Tabla filtrada".to_owned()
                        } else if !self.table_query.sort.is_empty() {
                            "Tabla ordenada".to_owned()
                        } else {
                            "Tabla cargada".to_owned()
                        };
                    }
                    Err(error) => {
                        self.table_loading_more = false;
                        self.set_error(error);
                    }
                }
            }
            WorkerResponse::SqlExecuted {
                connection_index,
                database,
                result,
                ..
            } => {
                if self.connection_index != connection_index
                    || self.active_database() != Some(database.as_str())
                {
                    return;
                }
                if self.return_to_table_after_execution {
                    self.handle_table_changes_execution_result(database, result);
                    self.return_to_table_after_execution = false;
                    self.busy_count = self.pending_requests.len();
                    return;
                }
                match result {
                    Ok(output) => {
                        self.content_title = format!("Resultado · {database}");
                        self.content_scroll = 0;
                        self.content = output.combined();
                        self.highlighted_content = None;
                        self.table_page = None;
                        self.table_metadata = None;
                        self.table_filter_expression.clear();
                        self.filter_session = None;
                        self.sort_session = None;
                        self.column_search_session = None;
                        self.table_show_metadata = false;
                        if output.success {
                            if let Some(session) = self.editor.as_mut() {
                                session.editor.mark_clean();
                            }
                            self.status = "T-SQL ejecutado correctamente".to_owned();
                        } else {
                            self.status = "ASE/isql devolvió un error".to_owned();
                        }
                    }
                    Err(error) => self.set_error(error),
                }
                self.mode = if self.return_to_editor_after_execution && self.editor.is_some() {
                    AppMode::Editor
                } else {
                    AppMode::Browser
                };
                self.return_to_editor_after_execution = false;
            }
            WorkerResponse::CatalogRefreshed {
                connection_index,
                result,
                ..
            } => {
                self.catalog_refresh_pending.remove(&connection_index);
                match result {
                    Ok(refresh) => {
                        let skipped_databases = refresh.skipped_databases.len();
                        let skipped_kinds = refresh.skipped_kinds.len();
                        self.catalog.upsert(refresh.connection);
                        self.rebuild_search_index();
                        if let Err(error) = self.catalog.save() {
                            self.status =
                                format!("Catálogo actualizado, pero no guardado: {error:#}");
                        } else if skipped_databases > 0 || skipped_kinds > 0 {
                            self.status = format!(
                                "Catálogo actualizado · {} bases omitidas · {} categorías omitidas",
                                skipped_databases, skipped_kinds
                            );
                        } else {
                            self.status = "Catálogo actualizado".to_owned();
                        }
                        self.refresh_search_suggestions();
                        self.resolve_pending_catalog_target();
                    }
                    Err(error) => {
                        self.status = format!("No se pudo actualizar el catálogo: {error}");
                    }
                }
            }
        }
        self.busy_count = self.pending_requests.len();
    }

    fn resolve_pending_catalog_target(&mut self) {
        let Some(target) = self.pending_catalog_target.take() else {
            return;
        };
        self.navigate_to_catalog_entry(target);
    }

    fn rebuild_search_index(&mut self) {
        self.search_index = self.catalog.search_entries();
    }

    fn begin_request(&mut self, status: impl Into<String>) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        self.pending_requests.insert(request_id);
        self.busy_count = self.pending_requests.len();
        self.status = status.into();
        request_id
    }

    fn send(&mut self, request: WorkerRequest) {
        if let Err(error) = self.request_tx.send(request) {
            self.pending_requests.clear();
            self.busy_count = 0;
            self.set_error(format!("El worker de base de datos terminó: {error}"));
        }
    }

    fn set_error(&mut self, message: String) {
        self.status = format!("ERROR: {}", first_line(&message));
        self.highlighted_content = None;
        self.table_page = None;
        self.table_metadata = None;
        self.table_show_metadata = false;
        self.clear_table_cell_editing();
        self.table_row_deletions.clear();
        self.table_new_rows.clear();
        self.table_row_visual_anchor = None;
        self.table_delete_sequence_armed = false;
        self.sort_session = None;
        self.column_search_session = None;
        self.content_title = "Error".to_owned();
        self.content_scroll = 0;
        self.content = message;
    }
}

fn reorder_table_page(page: TablePage, pinned_columns: &[String]) -> TablePage {
    if page.columns.is_empty() || pinned_columns.is_empty() {
        return page;
    }

    let mut order = Vec::with_capacity(page.columns.len());
    for pinned in pinned_columns {
        if let Some(index) = page
            .columns
            .iter()
            .position(|column| column.eq_ignore_ascii_case(pinned))
            && !order.contains(&index)
        {
            order.push(index);
        }
    }
    for index in 0..page.columns.len() {
        if !order.contains(&index) {
            order.push(index);
        }
    }

    if order
        .iter()
        .enumerate()
        .all(|(index, value)| index == *value)
    {
        return page;
    }

    let TablePage {
        columns,
        rows,
        next_cursor,
        has_more,
        total_rows,
    } = page;
    let reordered_columns = order.iter().map(|index| columns[*index].clone()).collect();
    let reordered_rows = rows
        .into_iter()
        .map(|row| order.iter().map(|index| row[*index].clone()).collect())
        .collect();

    TablePage::new(
        reordered_columns,
        reordered_rows,
        next_cursor,
        has_more,
        total_rows,
    )
    .expect("reordered table page preserves row widths")
}

fn selected_table_value(
    page: &TablePage,
    selected_row: Option<usize>,
    selected_column: usize,
) -> Option<String> {
    page.rows
        .get(selected_row?)
        .and_then(|row| row.get(selected_column))
        .cloned()
}

fn search_input(text: &str) -> TextArea<'static> {
    if text.is_empty() {
        return TextArea::default();
    }

    text.lines().map(ToOwned::to_owned).collect()
}

fn shifted_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    if delta < 0 {
        current.saturating_sub(delta.unsigned_abs()).min(len - 1)
    } else {
        current.saturating_add(delta as usize).min(len - 1)
    }
}

fn table_filter_suggestions(expression: &str, columns: &[String]) -> Vec<FilterSuggestion> {
    let has_trailing_space = expression.chars().last().is_some_and(char::is_whitespace);
    let token_start = if has_trailing_space {
        expression.len()
    } else {
        expression
            .char_indices()
            .rev()
            .find(|(_, character)| character.is_whitespace())
            .map_or(0, |(index, _)| index + 1)
    };
    let token = &expression[token_start..];
    let prefix = &expression[..token_start];
    let normalized_prefix = prefix.trim_end().to_ascii_uppercase();
    let expecting_column = prefix.trim().is_empty()
        || normalized_prefix.ends_with("AND")
        || normalized_prefix.ends_with('(');
    let mut suggestions = Vec::new();

    if expecting_column {
        let mut matches = columns
            .iter()
            .filter_map(|column| {
                let score = crate::search::score(column, token)?;
                Some((score, column))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(score, _)| *score);
        suggestions.extend(matches.into_iter().map(|(_, column)| FilterSuggestion {
            label: column.clone(),
            insertion: format!("{column} "),
            replace_start: token_start,
        }));
        return suggestions;
    }

    let operators = [
        ("=", "= "),
        ("!=", "!= "),
        ("<>", "<> "),
        (">=", ">= "),
        ("<=", "<= "),
        (">", "> "),
        ("<", "< "),
        ("LIKE", "LIKE "),
        ("NOT LIKE", "NOT LIKE "),
        ("IS NULL", "IS NULL "),
        ("IS NOT NULL", "IS NOT NULL "),
    ];
    if prefix.trim_end().split_whitespace().count() <= 1 {
        suggestions.extend(
            operators
                .iter()
                .filter(|(label, _)| {
                    token.is_empty()
                        || label
                            .to_ascii_lowercase()
                            .starts_with(&token.to_ascii_lowercase())
                })
                .map(|(label, insertion)| FilterSuggestion {
                    label: (*label).to_owned(),
                    insertion: (*insertion).to_owned(),
                    replace_start: token_start,
                }),
        );
    } else if normalized_prefix.ends_with("IS NULL") || normalized_prefix.ends_with("IS NOT NULL") {
        suggestions.push(FilterSuggestion {
            label: "AND".to_owned(),
            insertion: "AND ".to_owned(),
            replace_start: token_start,
        });
    } else if normalized_prefix.ends_with("IS NOT") {
        suggestions.extend(
            [("NULL", "NULL ")]
                .iter()
                .filter(|(label, _)| {
                    token.is_empty()
                        || label
                            .to_ascii_lowercase()
                            .starts_with(&token.to_ascii_lowercase())
                })
                .map(|(label, insertion)| FilterSuggestion {
                    label: (*label).to_owned(),
                    insertion: (*insertion).to_owned(),
                    replace_start: token_start,
                }),
        );
    } else if normalized_prefix.ends_with("IS") {
        suggestions.extend(
            [("NULL", "NULL "), ("NOT NULL", "NOT NULL ")]
                .iter()
                .filter(|(label, _)| {
                    token.is_empty()
                        || label
                            .to_ascii_lowercase()
                            .starts_with(&token.to_ascii_lowercase())
                })
                .map(|(label, insertion)| FilterSuggestion {
                    label: (*label).to_owned(),
                    insertion: (*insertion).to_owned(),
                    replace_start: token_start,
                }),
        );
    } else if normalized_prefix.ends_with("NOT") {
        suggestions.push(FilterSuggestion {
            label: "LIKE".to_owned(),
            insertion: "LIKE ".to_owned(),
            replace_start: token_start,
        });
    } else if normalized_prefix.ends_with('=')
        || normalized_prefix.ends_with("!=")
        || normalized_prefix.ends_with("<>")
        || normalized_prefix.ends_with('>')
        || normalized_prefix.ends_with('<')
        || normalized_prefix.ends_with("LIKE")
    {
        suggestions.push(FilterSuggestion {
            label: "valor…".to_owned(),
            insertion: "'' ".to_owned(),
            replace_start: token_start,
        });
    } else {
        suggestions.extend(
            ["AND"]
                .iter()
                .filter(|keyword| {
                    keyword
                        .to_ascii_lowercase()
                        .starts_with(&token.to_ascii_lowercase())
                })
                .map(|keyword| FilterSuggestion {
                    label: (*keyword).to_owned(),
                    insertion: "AND ".to_owned(),
                    replace_start: token_start,
                }),
        );
    }
    suggestions
}

fn table_sort_suggestions(expression: &str, columns: &[String]) -> Vec<FilterSuggestion> {
    let term_start = expression.rfind(',').map_or(0, |index| index + 1);
    let term = &expression[term_start..];
    let leading_whitespace = term.len() - term.trim_start().len();
    let content_start = term_start + leading_whitespace;
    let content = &expression[content_start..];
    let has_trailing_space = content.chars().last().is_some_and(char::is_whitespace);
    let token_start = if has_trailing_space {
        expression.len()
    } else {
        content
            .char_indices()
            .rev()
            .find(|(_, character)| character.is_whitespace())
            .map_or(content_start, |(index, _)| content_start + index + 1)
    };
    let token = &expression[token_start..];
    let before_token = expression[content_start..token_start].trim();

    if before_token.is_empty() {
        let mut matches = columns
            .iter()
            .filter_map(|column| {
                let score = crate::search::score(column, token)?;
                Some((score, column))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(score, _)| *score);
        return matches
            .into_iter()
            .map(|(_, column)| FilterSuggestion {
                label: column.clone(),
                insertion: format!("{column} "),
                replace_start: token_start,
            })
            .collect();
    }

    ["ASC", "DESC"]
        .iter()
        .filter(|direction| {
            token.is_empty()
                || direction
                    .to_ascii_lowercase()
                    .starts_with(&token.to_ascii_lowercase())
        })
        .map(|direction| FilterSuggestion {
            label: (*direction).to_owned(),
            insertion: format!("{direction} "),
            replace_start: token_start,
        })
        .collect()
}

fn format_sort_spec(sort: &SortSpec) -> String {
    let direction = match sort.direction {
        SortDirection::Ascending => "ASC",
        SortDirection::Descending => "DESC",
    };
    format!("{} {direction}", sort.column)
}

fn format_sort_expression(sort: &[SortSpec]) -> String {
    sort.iter()
        .map(format_sort_spec)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_filter_spec(filter: &FilterSpec) -> String {
    let operator = match filter.operator {
        FilterOperator::Equals => "=",
        FilterOperator::NotEquals => "!=",
        FilterOperator::Contains => "contains",
        FilterOperator::StartsWith => "starts with",
        FilterOperator::EndsWith => "ends with",
        FilterOperator::GreaterThan => ">",
        FilterOperator::GreaterThanOrEqual => ">=",
        FilterOperator::LessThan => "<",
        FilterOperator::LessThanOrEqual => "<=",
        FilterOperator::Like => "LIKE",
        FilterOperator::NotLike => "NOT LIKE",
        FilterOperator::IsNull => "IS NULL",
        FilterOperator::IsNotNull => "IS NOT NULL",
    };
    match filter.value.as_deref() {
        Some(value) => format!(
            "{} {} '{}'",
            filter.column,
            operator,
            value.replace('\'', "''")
        ),
        None => format!("{} {}", filter.column, operator),
    }
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value)
}

fn normalize_definition_for_edit(definition: &str) -> String {
    let mut output = Vec::new();
    let mut changed = false;
    for line in definition.lines() {
        if !changed {
            let leading = line.len() - line.trim_start().len();
            let trimmed = &line[leading..];
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("create procedure")
                || lower.starts_with("create proc")
                || lower.starts_with("create function")
                || lower.starts_with("create view")
            {
                let mut updated = line.to_owned();
                updated.replace_range(leading..leading + "create".len(), "create or replace");
                output.push(updated);
                changed = true;
                continue;
            }
        }
        output.push(line.to_owned());
    }
    output.join("\n")
}

fn is_write_sql(sql: &str) -> bool {
    const WRITE_WORDS: &[&str] = &[
        "alter",
        "begin",
        "checkpoint",
        "commit",
        "create",
        "dbcc",
        "delete",
        "drop",
        "dump",
        "exec",
        "execute",
        "grant",
        "insert",
        "into",
        "kill",
        "load",
        "merge",
        "revoke",
        "rollback",
        "shutdown",
        "truncate",
        "update",
        "updatetext",
        "writetext",
    ];

    let without_line_comments = sql
        .lines()
        .map(|line| line.split_once("--").map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();

    without_line_comments
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|word| WRITE_WORDS.contains(&word))
}

fn contains_executable_table_sql(sql: &str) -> bool {
    sql.lines().any(|line| {
        let code = line.split_once("--").map_or(line, |(code, _)| code);
        let Some(keyword) = code.split_whitespace().next() else {
            return false;
        };
        ["update", "delete", "insert"]
            .iter()
            .any(|expected| keyword.eq_ignore_ascii_case(expected))
    })
}

fn table_execution_committed(output: &SqlOutput) -> bool {
    output.success
        && output
            .combined()
            .contains(sybase::queries::staged_committed_marker())
}

fn table_execution_rolled_back(output: &SqlOutput) -> bool {
    output
        .combined()
        .contains(sybase::queries::staged_rolled_back_marker())
}

fn format_key(key: KeyEvent) -> String {
    let mut parts = Vec::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl".to_owned());
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt".to_owned());
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift".to_owned());
    }
    let code = match key.code {
        KeyCode::Char(character) => character.to_string(),
        KeyCode::F(number) => format!("F{number}"),
        other => format!("{other:?}"),
    };
    parts.push(code);
    parts.join("+")
}

fn format_table_type(column: &ColumnMetadata) -> String {
    match column.data_type.to_ascii_lowercase().as_str() {
        "char" | "varchar" | "nchar" | "nvarchar" | "binary" | "varbinary" | "unichar"
        | "univarchar" => column.length.map_or_else(
            || column.data_type.clone(),
            |length| format!("{}({length})", column.data_type),
        ),
        "numeric" | "decimal" => match (column.precision, column.scale) {
            (Some(precision), Some(scale)) => {
                format!("{}({precision},{scale})", column.data_type)
            }
            _ => column.data_type.clone(),
        },
        _ => column.data_type.clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableDraftChange {
    Saved,
    Updated,
    Reverted,
}

fn store_table_cell_draft(
    drafts: &mut Vec<TableCellDraft>,
    row_index: usize,
    column: String,
    original_value: String,
    value: String,
) -> TableDraftChange {
    let draft_index = drafts.iter().position(|draft| {
        draft.row_index == row_index && draft.column.eq_ignore_ascii_case(&column)
    });
    if value == original_value {
        if let Some(index) = draft_index {
            drafts.remove(index);
        }
        return TableDraftChange::Reverted;
    }
    if let Some(index) = draft_index {
        drafts[index].value = value;
        TableDraftChange::Updated
    } else {
        drafts.push(TableCellDraft {
            row_index,
            column,
            original_value,
            value,
        });
        TableDraftChange::Saved
    }
}

fn table_draft_exit_message(count: usize) -> String {
    let noun = if count == 1 { "borrador" } else { "borradores" };
    format!("Hay {count} {noun} sin guardar. ¿Salir y descartarlos? [y/N]")
}

fn compact_table_summary_value(value: &str) -> String {
    let value = value.replace(['\r', '\n'], "↵");
    if value.is_empty() {
        return "<vacío>".to_owned();
    }
    let mut shortened = value.chars().take(36).collect::<String>();
    if shortened.chars().count() < value.chars().count() {
        shortened.push('…');
    }
    shortened
}

fn table_exit_message(draft_count: usize, deletion_count: usize, new_row_count: usize) -> String {
    match (draft_count, deletion_count, new_row_count) {
        (drafts, 0, 0) => table_draft_exit_message(drafts),
        (0, deletions, 0) => {
            let noun = if deletions == 1 {
                "fila marcada"
            } else {
                "filas marcadas"
            };
            let pronoun = if deletions == 1 {
                "descartarla"
            } else {
                "descartarlas"
            };
            format!("Hay {deletions} {noun} para borrar sin guardar. ¿Salir y {pronoun}? [y/N]")
        }
        (drafts, deletions, 0) => {
            let draft_noun = if drafts == 1 {
                "borrador"
            } else {
                "borradores"
            };
            let row_noun = if deletions == 1 {
                "fila marcada"
            } else {
                "filas marcadas"
            };
            format!(
                "Hay {drafts} {draft_noun} y {deletions} {row_noun} para borrar sin guardar. ¿Salir y descartar los cambios? [y/N]"
            )
        }
        (0, 0, new_rows) => {
            let noun = if new_rows == 1 {
                "fila nueva"
            } else {
                "filas nuevas"
            };
            let pronoun = if new_rows == 1 {
                "descartarla"
            } else {
                "descartarlas"
            };
            format!("Hay {new_rows} {noun} sin guardar. ¿Salir y {pronoun}? [y/N]")
        }
        (drafts, deletions, new_rows) => {
            let draft_noun = if drafts == 1 {
                "borrador"
            } else {
                "borradores"
            };
            let row_noun = if deletions == 1 {
                "fila marcada"
            } else {
                "filas marcadas"
            };
            let new_row_label = if new_rows == 1 {
                "1 fila nueva".to_owned()
            } else {
                format!("{new_rows} filas nuevas")
            };
            format!(
                "Hay {drafts} {draft_noun}, {deletions} {row_noun} y {new_row_label} sin guardar. ¿Salir y descartar los cambios? [y/N]"
            )
        }
    }
}

fn table_date_time_kind(data_type: &str) -> Option<TableDateTimeKind> {
    match data_type.to_ascii_lowercase().as_str() {
        "date" => Some(TableDateTimeKind::Date),
        "datetime" | "smalldatetime" => Some(TableDateTimeKind::DateTime),
        "time" => Some(TableDateTimeKind::Time),
        _ => None,
    }
}

fn is_table_null_marker(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "null" | "<null>"
    )
}

fn parse_table_date_time_value(
    value: &str,
    kind: TableDateTimeKind,
) -> Result<ParsedTableDateTime, String> {
    if is_table_null_marker(value) {
        return Ok(ParsedTableDateTime {
            year: 2000,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            fractional_seconds: String::new(),
            is_null: true,
        });
    }

    let value = value.trim();
    match kind {
        TableDateTimeKind::Date => {
            let (year, month, day) = parse_date_expression(value)?;
            Ok(ParsedTableDateTime {
                year,
                month,
                day,
                hour: 0,
                minute: 0,
                second: 0,
                fractional_seconds: String::new(),
                is_null: false,
            })
        }
        TableDateTimeKind::DateTime => {
            let parts = value.replace('T', " ");
            let tokens = parts.split_whitespace().collect::<Vec<_>>();
            let (year, month, day, time_token) = if tokens
                .first()
                .is_some_and(|token| token.contains('-') || token.contains('/'))
            {
                let (year, month, day) = parse_numeric_date_token(tokens[0])?;
                (year, month, day, tokens.get(1).copied())
            } else if tokens.len() >= 3 {
                let month = parse_month_token(tokens[0])?;
                let day = tokens[1]
                    .parse::<u8>()
                    .map_err(|_| "el día no es válido".to_owned())?;
                let year = tokens[2]
                    .parse::<u16>()
                    .map_err(|_| "el año no es válido".to_owned())?;
                validate_date(year, month, day)?;
                (year, month, day, tokens.get(3).copied())
            } else {
                return Err("usa YYYY-MM-DD HH:MM:SS".to_owned());
            };
            let (hour, minute, second, fractional_seconds) = time_token
                .map(parse_time_token)
                .transpose()?
                .unwrap_or((0, 0, 0, String::new()));
            Ok(ParsedTableDateTime {
                year,
                month,
                day,
                hour,
                minute,
                second,
                fractional_seconds,
                is_null: false,
            })
        }
        TableDateTimeKind::Time => {
            let (hour, minute, second, fractional_seconds) = parse_time_token(value)?;
            Ok(ParsedTableDateTime {
                year: 2000,
                month: 1,
                day: 1,
                hour,
                minute,
                second,
                fractional_seconds,
                is_null: false,
            })
        }
    }
}

fn parse_date_expression(value: &str) -> Result<(u16, u8, u8), String> {
    let tokens = value.split_whitespace().collect::<Vec<_>>();
    if let Some(token) = tokens.first()
        && (token.contains('-') || token.contains('/'))
    {
        return parse_numeric_date_token(token);
    }
    if tokens.len() < 3 {
        return Err("usa YYYY-MM-DD".to_owned());
    }
    let month = parse_month_token(tokens[0])?;
    let day = tokens[1]
        .parse::<u8>()
        .map_err(|_| "el día no es válido".to_owned())?;
    let year = tokens[2]
        .parse::<u16>()
        .map_err(|_| "el año no es válido".to_owned())?;
    validate_date(year, month, day)?;
    Ok((year, month, day))
}

fn parse_numeric_date_token(value: &str) -> Result<(u16, u8, u8), String> {
    let parts = value.split(|character| character == '-' || character == '/');
    let parts = parts.collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err("usa YYYY-MM-DD".to_owned());
    }
    let year = parts[0]
        .parse::<u16>()
        .map_err(|_| "el año no es válido".to_owned())?;
    let month = parts[1]
        .parse::<u8>()
        .map_err(|_| "el mes no es válido".to_owned())?;
    let day = parts[2]
        .parse::<u8>()
        .map_err(|_| "el día no es válido".to_owned())?;
    validate_date(year, month, day)?;
    Ok((year, month, day))
}

fn parse_month_token(value: &str) -> Result<u8, String> {
    let month = match value.to_ascii_lowercase().as_str() {
        "1" | "01" | "jan" | "january" => 1,
        "2" | "02" | "feb" | "february" => 2,
        "3" | "03" | "mar" | "march" => 3,
        "4" | "04" | "apr" | "april" => 4,
        "5" | "05" | "may" => 5,
        "6" | "06" | "jun" | "june" => 6,
        "7" | "07" | "jul" | "july" => 7,
        "8" | "08" | "aug" | "august" => 8,
        "9" | "09" | "sep" | "september" => 9,
        "10" | "oct" | "october" => 10,
        "11" | "nov" | "november" => 11,
        "12" | "dec" | "december" => 12,
        _ => return Err("el mes no es válido".to_owned()),
    };
    Ok(month)
}

fn parse_time_token(value: &str) -> Result<(u8, u8, u8, String), String> {
    let value = value.trim();
    let upper = value.to_ascii_uppercase();
    let meridiem = if upper.ends_with("AM") {
        Some(false)
    } else if upper.ends_with("PM") {
        Some(true)
    } else {
        None
    };
    let clock = meridiem.map_or(value, |_| &value[..value.len().saturating_sub(2)]);
    let (clock, fractional_seconds) = clock
        .split_once('.')
        .map_or((clock, ""), |(clock, fraction)| (clock, fraction));
    if !fractional_seconds
        .chars()
        .all(|character| character.is_ascii_digit())
    {
        return Err("los milisegundos no son válidos".to_owned());
    }
    let parts = clock.split(':').collect::<Vec<_>>();
    if parts.len() < 2 || parts.len() > 3 {
        return Err("usa HH:MM o HH:MM:SS".to_owned());
    }
    let mut hour = parts[0]
        .parse::<u8>()
        .map_err(|_| "la hora no es válida".to_owned())?;
    let minute = parts[1]
        .parse::<u8>()
        .map_err(|_| "los minutos no son válidos".to_owned())?;
    let second = parts
        .get(2)
        .map_or(Ok(0), |part| part.parse::<u8>())
        .map_err(|_| "los segundos no son válidos".to_owned())?;
    if minute > 59 || second > 59 {
        return Err("minutos y segundos deben estar entre 0 y 59".to_owned());
    }
    if let Some(is_pm) = meridiem {
        if !(1..=12).contains(&hour) {
            return Err("la hora AM/PM debe estar entre 1 y 12".to_owned());
        }
        hour = if hour == 12 { 0 } else { hour };
        if is_pm {
            hour = hour.saturating_add(12);
        }
    } else if hour > 23 {
        return Err("la hora debe estar entre 0 y 23".to_owned());
    }
    Ok((hour, minute, second, fractional_seconds.to_owned()))
}

fn validate_date(year: u16, month: u8, day: u8) -> Result<(), String> {
    if !(1..=12).contains(&month) {
        return Err("el mes debe estar entre 1 y 12".to_owned());
    }
    if !(1..=days_in_month(year, month)).contains(&day) {
        return Err("el día no es válido para ese mes".to_owned());
    }
    Ok(())
}

fn is_leap_year(year: u16) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn stepped_clamped(value: u16, minimum: u16, maximum: u16, delta: i32) -> u16 {
    if delta < 0 {
        value
            .saturating_sub(delta.unsigned_abs() as u16)
            .max(minimum)
    } else {
        value.saturating_add(delta as u16).min(maximum)
    }
}

fn stepped_clamped_u8(value: u8, minimum: u8, maximum: u8, delta: i32) -> u8 {
    if delta < 0 {
        value
            .saturating_sub(delta.unsigned_abs() as u8)
            .max(minimum)
    } else {
        value.saturating_add(delta as u8).min(maximum)
    }
}

fn stepped_wrapped(value: u8, minimum: u8, maximum: u8, delta: i32) -> u8 {
    if delta < 0 {
        if value <= minimum { maximum } else { value - 1 }
    } else if value >= maximum {
        minimum
    } else {
        value + 1
    }
}

fn format_parsed_table_date_time(value: &ParsedTableDateTime, kind: TableDateTimeKind) -> String {
    if value.is_null {
        return "<NULL>".to_owned();
    }
    let date = format!("{:04}-{:02}-{:02}", value.year, value.month, value.day);
    let time = if value.fractional_seconds.is_empty() {
        format!("{:02}:{:02}:{:02}", value.hour, value.minute, value.second)
    } else {
        format!(
            "{:02}:{:02}:{:02}.{}",
            value.hour, value.minute, value.second, value.fractional_seconds
        )
    };
    match kind {
        TableDateTimeKind::Date => date,
        TableDateTimeKind::DateTime => format!("{date} {time}"),
        TableDateTimeKind::Time => time,
    }
}

fn format_table_date_time_picker(session: &TableDateTimePickerSession) -> String {
    format_parsed_table_date_time(
        &ParsedTableDateTime {
            year: session.year,
            month: session.month,
            day: session.day,
            hour: session.hour,
            minute: session.minute,
            second: session.second,
            fractional_seconds: session.fractional_seconds.clone(),
            is_null: session.is_null,
        },
        if session.show_date && session.show_time {
            TableDateTimeKind::DateTime
        } else if session.show_date {
            TableDateTimeKind::Date
        } else {
            TableDateTimeKind::Time
        },
    )
}

fn validate_table_cell_value(value: &str, column: &ColumnMetadata) -> Result<(), String> {
    let trimmed = value.trim();
    if is_table_null_marker(trimmed) {
        return if column.nullable {
            Ok(())
        } else {
            Err("la columna no permite NULL".to_owned())
        };
    }

    let data_type = column.data_type.to_ascii_lowercase();
    match data_type.as_str() {
        "tinyint" | "smallint" | "int" | "integer" | "bigint" => trimmed
            .parse::<i128>()
            .map(|_| ())
            .map_err(|_| format!("{data_type} requiere un entero")),
        "numeric" | "decimal" => validate_decimal_value(trimmed, column),
        "float" | "real" | "double" | "double precision" => trimmed
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(|_| ())
            .ok_or_else(|| format!("{data_type} requiere un número válido")),
        "date" => parse_table_date_time_value(trimmed, TableDateTimeKind::Date)
            .map(|_| ())
            .map_err(|error| format!("date inválido: {error}")),
        "datetime" | "smalldatetime" => {
            parse_table_date_time_value(trimmed, TableDateTimeKind::DateTime)
                .map(|_| ())
                .map_err(|error| format!("datetime inválido: {error}"))
        }
        "time" => parse_table_date_time_value(trimmed, TableDateTimeKind::Time)
            .map(|_| ())
            .map_err(|error| format!("time inválido: {error}")),
        "bit" => {
            if matches!(
                trimmed.to_ascii_lowercase().as_str(),
                "0" | "1" | "true" | "false"
            ) {
                Ok(())
            } else {
                Err("bit requiere 0, 1, true o false".to_owned())
            }
        }
        "char" | "varchar" | "nchar" | "nvarchar" | "binary" | "varbinary" | "unichar"
        | "univarchar" => validate_text_length(value, column),
        "money" | "smallmoney" => validate_decimal_syntax(trimmed)
            .map_err(|_| format!("{data_type} requiere un número válido")),
        _ => Ok(()),
    }
}

fn validate_text_length(value: &str, column: &ColumnMetadata) -> Result<(), String> {
    if let Some(length) = column.length
        && value.chars().count() > length as usize
    {
        return Err(format!("supera la longitud máxima de {length}"));
    }
    Ok(())
}

fn validate_decimal_value(value: &str, column: &ColumnMetadata) -> Result<(), String> {
    validate_decimal_syntax(value).map_err(|_| "requiere un número decimal válido".to_owned())?;
    let Some(scale) = column.scale else {
        return Ok(());
    };
    let fractional_digits = value
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    if fractional_digits > scale as usize {
        return Err(format!("permite como máximo {scale} decimales"));
    }
    if let Some(precision) = column.precision {
        let unsigned = value.trim_start_matches(|character| matches!(character, '+' | '-'));
        let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        let integer_digits = integer.trim_start_matches('0').len();
        let significant_digits = integer_digits + fraction.len();
        if significant_digits > precision as usize {
            return Err(format!("supera la precisión máxima de {precision}"));
        }
        if integer_digits > precision.saturating_sub(scale) as usize {
            return Err(format!(
                "permite como máximo {} dígitos enteros",
                precision.saturating_sub(scale)
            ));
        }
    }
    Ok(())
}

fn validate_decimal_syntax(value: &str) -> Result<(), ()> {
    let unsigned = value.trim_start_matches(|character| matches!(character, '+' | '-'));
    let Some((integer, fraction)) =
        unsigned
            .split_once('.')
            .map_or(Some((unsigned, "")), |(integer, fraction)| {
                (!integer.is_empty() || !fraction.is_empty()).then_some((integer, fraction))
            })
    else {
        return Err(());
    };
    if (integer.is_empty() && fraction.is_empty())
        || !integer.chars().all(|character| character.is_ascii_digit())
        || !fraction.chars().all(|character| character.is_ascii_digit())
    {
        return Err(());
    }
    Ok(())
}

fn table_copy_text(
    page: &TablePage,
    source: TableCopySource,
    selected_row: Option<usize>,
    selected_column: usize,
    visual_anchor: Option<(usize, usize)>,
    include_header: bool,
) -> Option<String> {
    if page.columns.is_empty() {
        return None;
    }

    let column = selected_column.min(page.columns.len() - 1);
    let (start_row, end_row, start_column, end_column) = match source {
        TableCopySource::LoadedData => (0, page.rows.len(), 0, page.columns.len()),
        TableCopySource::CurrentCell => {
            let row = selected_row?;
            (row, row.saturating_add(1), column, column.saturating_add(1))
        }
        TableCopySource::CurrentRow => {
            let row = selected_row?;
            (row, row.saturating_add(1), 0, page.columns.len())
        }
        TableCopySource::CurrentColumn => (0, page.rows.len(), column, column.saturating_add(1)),
        TableCopySource::VisualSelection => {
            let row = selected_row?;
            let (anchor_row, anchor_column) = visual_anchor?;
            (
                anchor_row.min(row),
                anchor_row.max(row).saturating_add(1),
                anchor_column.min(column),
                anchor_column.max(column).saturating_add(1),
            )
        }
        TableCopySource::VisualRows => return None,
    };

    if end_row > page.rows.len() || end_column > page.columns.len() {
        return None;
    }

    let mut lines = Vec::new();
    if include_header {
        lines.push(page.columns[start_column..end_column].join("\t"));
    }
    lines.extend(
        page.rows[start_row..end_row]
            .iter()
            .map(|values| values[start_column..end_column].join("\t")),
    );
    Some(lines.join("\n"))
}

fn table_copy_rows_text(
    page: &TablePage,
    anchor_row: Option<usize>,
    selected_row: Option<usize>,
    include_header: bool,
) -> Option<String> {
    if page.columns.is_empty() || page.rows.is_empty() {
        return None;
    }
    let anchor_row = anchor_row?;
    let selected_row = selected_row?;
    let start_row = anchor_row.min(selected_row);
    let end_row = anchor_row.max(selected_row);
    if end_row >= page.rows.len() {
        return None;
    }

    let mut lines = Vec::new();
    if include_header {
        lines.push(page.columns.join("\t"));
    }
    lines.extend(
        page.rows[start_row..=end_row]
            .iter()
            .map(|values| values.join("\t")),
    );
    Some(lines.join("\n"))
}

fn shift_row_indexes(rows: &BTreeSet<usize>, removed_row: usize) -> BTreeSet<usize> {
    rows.iter()
        .filter_map(|row_index| {
            if *row_index == removed_row {
                None
            } else if *row_index > removed_row {
                Some(*row_index - 1)
            } else {
                Some(*row_index)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        TableCopySource, TableDateTimeKind, TableDraftChange, compact_table_summary_value,
        contains_executable_table_sql, format_parsed_table_date_time, format_table_type,
        is_write_sql, normalize_definition_for_edit, parse_table_date_time_value,
        reorder_table_page, selected_table_value, shift_row_indexes, shifted_index,
        store_table_cell_draft, table_copy_rows_text, table_copy_text, table_date_time_kind,
        table_draft_exit_message, table_execution_committed, table_execution_rolled_back,
        table_exit_message, table_filter_suggestions, table_sort_suggestions,
        validate_table_cell_value,
    };
    use crate::db::models::{ColumnMetadata, SqlOutput, TablePage};
    use crate::db::query::PageCursor;
    use std::collections::BTreeSet;

    #[test]
    fn detects_writes_conservatively() {
        assert!(is_write_sql(
            "update dbo.cliente set nombre = 'A' where id = 1"
        ));
        assert!(is_write_sql(
            "begin tran\ndelete from dbo.cliente where id = 1\nrollback"
        ));
        assert!(is_write_sql("select * into #tmp from dbo.cliente"));
        assert!(!is_write_sql(
            "-- update ignored\nselect * from dbo.cliente"
        ));
    }

    #[test]
    fn turns_create_into_create_or_replace_for_editing() {
        let source = "/* header */\ncreate procedure dbo.demo as\nselect 1";
        let edited = normalize_definition_for_edit(source);
        assert!(edited.contains("create or replace procedure dbo.demo"));
        assert!(!edited.contains("\ncreate procedure dbo.demo"));
    }

    #[test]
    fn selection_is_clamped() {
        assert_eq!(shifted_index(0, 3, -1), 0);
        assert_eq!(shifted_index(1, 3, 1), 2);
        assert_eq!(shifted_index(2, 3, 1), 2);
        assert_eq!(shifted_index(0, 0, 1), 0);
    }

    #[test]
    fn formats_parameterized_table_types_for_copy() {
        let varchar = ColumnMetadata {
            name: "name".to_owned(),
            data_type: "varchar".to_owned(),
            length: Some(255),
            precision: None,
            scale: None,
            nullable: true,
            ordinal_position: 1,
        };
        let numeric = ColumnMetadata {
            data_type: "numeric".to_owned(),
            precision: Some(12),
            scale: Some(2),
            ..varchar.clone()
        };

        assert_eq!(format_table_type(&varchar), "varchar(255)");
        assert_eq!(format_table_type(&numeric), "numeric(12,2)");
    }

    #[test]
    fn serializes_a_visual_selection_as_tsv_with_optional_header() {
        let page = TablePage::new(
            vec!["id".to_owned(), "name".to_owned(), "status".to_owned()],
            vec![
                vec!["1".to_owned(), "Ada".to_owned(), "active".to_owned()],
                vec!["2".to_owned(), "Lin".to_owned(), "blocked".to_owned()],
            ],
            Some(PageCursor::Offset(2)),
            false,
            None,
        )
        .expect("valid table page");

        assert_eq!(
            table_copy_text(
                &page,
                TableCopySource::VisualSelection,
                Some(1),
                2,
                Some((0, 1)),
                true,
            )
            .as_deref(),
            Some("name\tstatus\nAda\tactive\nLin\tblocked")
        );
    }

    #[test]
    fn serializes_current_cell_without_header() {
        let page = TablePage::new(
            vec!["id".to_owned(), "name".to_owned()],
            vec![vec!["1".to_owned(), "Ada".to_owned()]],
            None,
            false,
            None,
        )
        .expect("valid table page");

        assert_eq!(
            table_copy_text(&page, TableCopySource::CurrentCell, Some(0), 1, None, false)
                .as_deref(),
            Some("Ada")
        );
    }

    #[test]
    fn suggests_columns_and_operators_from_filter_context() {
        let columns = vec!["status".to_owned(), "total".to_owned()];

        let columns_suggestions = table_filter_suggestions("sta", &columns);
        assert_eq!(columns_suggestions[0].label, "status");

        let operators = table_filter_suggestions("status ", &columns);
        assert!(
            operators
                .iter()
                .any(|suggestion| suggestion.label == "LIKE")
        );

        let next_columns = table_filter_suggestions("status = 'active' AND ", &columns);
        assert!(
            next_columns
                .iter()
                .any(|suggestion| suggestion.label == "total")
        );
    }

    #[test]
    fn suggests_sort_columns_and_directions_from_context() {
        let columns = vec!["created_at".to_owned(), "status".to_owned()];

        let columns_suggestions = table_sort_suggestions("cre", &columns);
        assert_eq!(columns_suggestions[0].label, "created_at");

        let directions = table_sort_suggestions("created_at ", &columns);
        assert_eq!(
            directions
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            vec!["ASC", "DESC"]
        );

        let next_columns = table_sort_suggestions("created_at DESC, ", &columns);
        assert!(
            next_columns
                .iter()
                .any(|suggestion| suggestion.label == "status")
        );
    }

    #[test]
    fn reorders_pinned_columns_before_unpinned_columns() {
        let page = TablePage::new(
            vec!["id".to_owned(), "name".to_owned(), "status".to_owned()],
            vec![vec!["1".to_owned(), "Ada".to_owned(), "active".to_owned()]],
            None,
            false,
            None,
        )
        .expect("valid table page");

        let reordered = reorder_table_page(page, &["status".to_owned(), "id".to_owned()]);

        assert_eq!(reordered.columns, ["status", "id", "name"]);
        assert_eq!(reordered.rows, [["active", "1", "Ada"]]);
    }

    #[test]
    fn reads_the_complete_selected_cell_value() {
        let page = TablePage::new(
            vec!["id".to_owned(), "description".to_owned()],
            vec![vec!["1".to_owned(), "A complete value".to_owned()]],
            None,
            false,
            None,
        )
        .expect("valid table page");

        assert_eq!(
            selected_table_value(&page, Some(0), 1).as_deref(),
            Some("A complete value")
        );
        assert!(selected_table_value(&page, None, 1).is_none());
    }

    #[test]
    fn validates_table_cell_values_against_column_metadata() {
        let varchar = ColumnMetadata {
            name: "name".to_owned(),
            data_type: "varchar".to_owned(),
            length: Some(5),
            precision: None,
            scale: None,
            nullable: false,
            ordinal_position: 1,
        };
        assert!(validate_table_cell_value("Ada", &varchar).is_ok());
        assert!(validate_table_cell_value("Too long", &varchar).is_err());

        let integer = ColumnMetadata {
            data_type: "int".to_owned(),
            ..varchar.clone()
        };
        assert!(validate_table_cell_value("42", &integer).is_ok());
        assert!(validate_table_cell_value("not-a-number", &integer).is_err());

        let nullable = ColumnMetadata {
            nullable: true,
            ..varchar
        };
        assert!(validate_table_cell_value("NULL", &nullable).is_ok());
    }

    #[test]
    fn validates_decimal_precision_and_scale() {
        let decimal = ColumnMetadata {
            name: "amount".to_owned(),
            data_type: "decimal".to_owned(),
            length: None,
            precision: Some(5),
            scale: Some(2),
            nullable: false,
            ordinal_position: 1,
        };

        assert!(validate_table_cell_value("123.45", &decimal).is_ok());
        assert!(validate_table_cell_value("123.456", &decimal).is_err());
        assert!(validate_table_cell_value("1234.56", &decimal).is_err());
    }

    #[test]
    fn stores_updates_and_reverts_a_cell_draft() {
        let mut drafts = Vec::new();
        assert_eq!(
            store_table_cell_draft(
                &mut drafts,
                2,
                "name".to_owned(),
                "Ada".to_owned(),
                "Grace".to_owned(),
            ),
            TableDraftChange::Saved
        );
        assert_eq!(drafts[0].value, "Grace");

        assert_eq!(
            store_table_cell_draft(
                &mut drafts,
                2,
                "NAME".to_owned(),
                "Ada".to_owned(),
                "Marie".to_owned(),
            ),
            TableDraftChange::Updated
        );
        assert_eq!(drafts[0].value, "Marie");

        assert_eq!(
            store_table_cell_draft(
                &mut drafts,
                2,
                "name".to_owned(),
                "Ada".to_owned(),
                "Ada".to_owned(),
            ),
            TableDraftChange::Reverted
        );
        assert!(drafts.is_empty());
    }

    #[test]
    fn describes_pending_drafts_before_table_exit() {
        assert_eq!(
            table_draft_exit_message(1),
            "Hay 1 borrador sin guardar. ¿Salir y descartarlos? [y/N]"
        );
        assert_eq!(
            table_draft_exit_message(2),
            "Hay 2 borradores sin guardar. ¿Salir y descartarlos? [y/N]"
        );
        assert_eq!(
            table_exit_message(0, 1, 0),
            "Hay 1 fila marcada para borrar sin guardar. ¿Salir y descartarla? [y/N]"
        );
        assert_eq!(
            table_exit_message(2, 3, 0),
            "Hay 2 borradores y 3 filas marcadas para borrar sin guardar. ¿Salir y descartar los cambios? [y/N]"
        );
        assert_eq!(
            table_exit_message(0, 0, 1),
            "Hay 1 fila nueva sin guardar. ¿Salir y descartarla? [y/N]"
        );
    }

    #[test]
    fn compacts_values_for_the_pending_changes_summary() {
        assert_eq!(compact_table_summary_value(""), "<vacío>");
        assert_eq!(compact_table_summary_value("line\none"), "line↵one");

        let long_value = "a".repeat(40);
        assert_eq!(
            compact_table_summary_value(&long_value),
            format!("{}…", "a".repeat(36))
        );
    }

    #[test]
    fn recognizes_only_executable_staged_sql_statements() {
        assert!(contains_executable_table_sql(
            "-- update ignored\nset quoted_identifier on\nUPDATE \"dbo\".\"orders\""
        ));
        assert!(contains_executable_table_sql(
            "delete from \"dbo\".\"orders\""
        ));
        assert!(contains_executable_table_sql(
            "insert into \"dbo\".\"orders\""
        ));
        assert!(!contains_executable_table_sql(
            "-- insert ignored\nset quoted_identifier on\n"
        ));
    }

    #[test]
    fn clears_table_changes_only_after_a_confirmed_commit_marker() {
        let committed = SqlOutput {
            stdout: format!(
                "{}\n",
                crate::db::sybase::queries::staged_committed_marker()
            ),
            stderr: String::new(),
            success: true,
        };
        let rolled_back = SqlOutput {
            stdout: crate::db::sybase::queries::staged_rolled_back_marker().to_owned(),
            stderr: String::new(),
            success: true,
        };
        let process_failed_with_commit_marker = SqlOutput {
            stdout: crate::db::sybase::queries::staged_committed_marker().to_owned(),
            stderr: String::new(),
            success: false,
        };

        assert!(table_execution_committed(&committed));
        assert!(!table_execution_committed(&rolled_back));
        assert!(!table_execution_committed(
            &process_failed_with_commit_marker
        ));
        assert!(table_execution_rolled_back(&rolled_back));
    }

    #[test]
    fn copies_a_range_of_selected_rows_with_optional_header() {
        let page = TablePage::new(
            vec!["id".to_owned(), "name".to_owned()],
            vec![
                vec!["1".to_owned(), "Ada".to_owned()],
                vec!["2".to_owned(), "Grace".to_owned()],
                vec!["3".to_owned(), "Marie".to_owned()],
            ],
            None,
            false,
            None,
        )
        .expect("valid table page");

        assert_eq!(
            table_copy_rows_text(&page, Some(2), Some(1), false).as_deref(),
            Some("2\tGrace\n3\tMarie")
        );
        assert_eq!(
            table_copy_rows_text(&page, Some(2), Some(1), true).as_deref(),
            Some("id\tname\n2\tGrace\n3\tMarie")
        );
    }

    #[test]
    fn shifts_row_indexes_after_discarding_a_new_row() {
        let rows = BTreeSet::from([1, 3, 4]);
        assert_eq!(shift_row_indexes(&rows, 1), BTreeSet::from([2, 3]));
    }

    #[test]
    fn parses_and_formats_date_time_values() {
        let date = parse_table_date_time_value("2024-02-29", TableDateTimeKind::Date)
            .expect("leap day is valid");
        assert_eq!(
            format_parsed_table_date_time(&date, TableDateTimeKind::Date),
            "2024-02-29"
        );

        let datetime =
            parse_table_date_time_value("Jun 1 2024 12:05:07PM", TableDateTimeKind::DateTime)
                .expect("ASE datetime format is supported");
        assert_eq!(
            format_parsed_table_date_time(&datetime, TableDateTimeKind::DateTime),
            "2024-06-01 12:05:07"
        );

        let time = parse_table_date_time_value("23:59:58.123", TableDateTimeKind::Time)
            .expect("time with fractional seconds is supported");
        assert_eq!(
            format_parsed_table_date_time(&time, TableDateTimeKind::Time),
            "23:59:58.123"
        );
    }

    #[test]
    fn rejects_invalid_dates_and_identifies_date_time_types() {
        assert!(parse_table_date_time_value("2023-02-29", TableDateTimeKind::Date).is_err());
        assert!(parse_table_date_time_value("25:00", TableDateTimeKind::Time).is_err());
        assert_eq!(
            table_date_time_kind("datetime"),
            Some(TableDateTimeKind::DateTime)
        );
        assert_eq!(table_date_time_kind("varchar"), None);
    }
}
