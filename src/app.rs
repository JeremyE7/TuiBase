use std::{
    collections::HashSet,
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::catalog::{CatalogCache, CatalogEntry, connection_key};
use crate::db::models::TablePreview;
use crate::services;
use crate::ui;
use crossbeam_channel::{Receiver, Sender};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Text;
use ratatui::widgets::{ScrollbarState, TableState};
use ratatui_textarea::{CursorMove, Input, TextArea};

use crate::{
    config::{AppConfig, ConnectionProfile},
    db::{
        models::{DbObject, ObjectKind},
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
    DiscardEditor,
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
    pub table_preview: Option<TablePreview>,
    pub table_state: TableState,
    pub horizontal_scroll: ScrollbarState,
    pub table_column_index: usize,
    pub table_visible_columns: usize,
    search: Option<SearchSession>,
    active_search: Option<String>,
    catalog: CatalogCache,
    catalog_refresh_pending: HashSet<usize>,
    pending_catalog_target: Option<CatalogEntry>,
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
}

impl App {
    pub fn new(
        config: AppConfig,
        config_path: PathBuf,
        request_tx: Sender<WorkerRequest>,
        response_rx: Receiver<WorkerResponse>,
    ) -> Self {
        let catalog = CatalogCache::load().unwrap_or_default();

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
            table_preview: None,
            table_state: TableState::default(),
            horizontal_scroll: ScrollbarState::new(0),
            table_column_index: 0,
            table_visible_columns: 0,
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
            search: None,
            active_search: None,
            catalog,
            catalog_refresh_pending: HashSet::new(),
            pending_catalog_target: None,
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
                    self.mode = AppMode::Browser;
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
            KeyCode::Char('?') => self.mode = AppMode::Help,
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
        self.search = Some(SearchSession {
            input: TextArea::default(),
            suggestions: Vec::new(),
            selected_suggestion: 0,
        });

        self.mode = AppMode::Search;
        self.refresh_search_suggestions();
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
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            self.table_preview = None;
            self.mode = AppMode::Browser;
            return;
        }

        let Some(preview) = self.table_preview.as_ref() else {
            self.mode = AppMode::Browser;
            return;
        };

        if preview.columns.is_empty() {
            return;
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if preview.rows.is_empty() {
                    return;
                }

                let current = self.table_state.selected().unwrap_or(0);
                let next = (current + 1).min(preview.rows.len() - 1);

                self.table_state.select(Some(next));
            }

            KeyCode::Char('k') | KeyCode::Up => {
                if preview.rows.is_empty() {
                    return;
                }

                let current = self.table_state.selected().unwrap_or(0);
                let previous = current.saturating_sub(1);

                self.table_state.select(Some(previous));
            }
            KeyCode::Char('h') | KeyCode::Left => {
                let previous = self.table_column_index.saturating_sub(1);
                self.table_column_index = previous;

                if previous < self.horizontal_scroll.get_position() {
                    self.horizontal_scroll.prev();
                }
            }

            KeyCode::Char('l') | KeyCode::Right => {
                let last_column = preview.columns.len().saturating_sub(1);
                let next = self.table_column_index.saturating_add(1).min(last_column);
                self.table_column_index = next;

                let viewport_end = self
                    .horizontal_scroll
                    .get_position()
                    .saturating_add(self.table_visible_columns.max(1));
                if next >= viewport_end {
                    self.horizontal_scroll.next();
                }
            }

            KeyCode::Char('g') | KeyCode::Home => {
                if preview.rows.is_empty() {
                    return;
                }

                self.table_state.select(Some(0));
            }

            KeyCode::Char('G') | KeyCode::End => {
                if preview.rows.is_empty() {
                    return;
                }

                self.table_state.select(Some(preview.rows.len() - 1));
            }

            _ => {}
        }
    }
    fn handle_search_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.search = None;
            self.mode = AppMode::Browser;
            return;
        }

        if key.code == KeyCode::Tab {
            self.accept_search_suggestion();
            return;
        }

        if key.code == KeyCode::Up {
            self.move_search_suggestion(-1);
            return;
        }

        if key.code == KeyCode::Down {
            self.move_search_suggestion(1);
            return;
        }

        if key.code == KeyCode::Enter {
            let Some(session) = self.search.take() else {
                self.mode = AppMode::Browser;
                return;
            };

            let query = session.input.lines().join("\n").trim().to_owned();
            self.mode = AppMode::Browser;

            if query.is_empty() {
                return;
            }

            self.active_search = Some(query);
            if let Some(entry) = session
                .suggestions
                .get(session.selected_suggestion)
                .cloned()
            {
                self.navigate_to_catalog_entry(entry);
            } else {
                self.status = "No hay coincidencias en el catálogo".to_owned();
            }
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            if let Some(session) = self.search.as_mut() {
                session.input = TextArea::default();
                session.selected_suggestion = 0;
            }

            self.active_search = None;
            self.refresh_search_suggestions();
            return;
        }

        if let Some(session) = self.search.as_mut() {
            let input: Input = key.into();
            session.input.input(input);
        }

        self.refresh_search_suggestions();
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

    fn search_suggestions(&self, query: &str) -> Vec<CatalogEntry> {
        let query = query.trim();
        self.catalog
            .entries()
            .into_iter()
            .filter(|entry| {
                query.is_empty()
                    || crate::search::matches(&entry.search_text(), query)
                    || crate::search::matches(&entry.path(), query)
            })
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
                    ConfirmAction::DiscardEditor => {
                        self.editor = None;
                        self.mode = AppMode::Browser;
                        self.status = "Cambios descartados".to_owned();
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc | KeyCode::Enter => {
                self.confirm_action = None;
                self.return_to_editor_after_execution = false;
                self.mode = if self.editor.is_some() {
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
        self.send(WorkerRequest::PreviewTable {
            request_id,
            connection_index,
            database,
            object,
            row_limit: 100,
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

    fn handle_worker_response(&mut self, response: WorkerResponse) {
        let request_id = match &response {
            WorkerResponse::ConnectionTested { request_id, .. }
            | WorkerResponse::DatabasesLoaded { request_id, .. }
            | WorkerResponse::ObjectsLoaded { request_id, .. }
            | WorkerResponse::DefinitionLoaded { request_id, .. }
            | WorkerResponse::TablePreviewed { request_id, .. }
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
                        self.table_preview = None;

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
                        self.table_preview = None;
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
                {
                    return;
                }
                match result {
                    Ok(output) => {
                        self.content_title = format!("Datos · {} ", object.qualified_name());
                        self.content_scroll = 0;
                        self.horizontal_scroll = ScrollbarState::new(output.columns.len());
                        self.table_state = TableState::default();
                        self.table_column_index = 0;
                        self.table_visible_columns = 0;
                        if !output.rows.is_empty() {
                            self.table_state.select(Some(0));
                        }
                        self.table_preview = Some(output);
                        self.current_content_object = Some(object);
                        self.highlighted_content = None;
                        self.mode = AppMode::Table;
                        self.status = "Tabla cargada".to_owned();
                    }
                    Err(error) => self.set_error(error),
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
                match result {
                    Ok(output) => {
                        self.content_title = format!("Resultado · {database}");
                        self.content_scroll = 0;
                        self.content = output.combined();
                        self.highlighted_content = None;
                        self.table_preview = None;
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
        self.table_preview = None;
        self.content_title = "Error".to_owned();
        self.content_scroll = 0;
        self.content = message;
    }
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

#[cfg(test)]
mod tests {
    use super::{is_write_sql, normalize_definition_for_edit, shifted_index};

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
}
