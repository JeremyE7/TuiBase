use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Row, Table, Wrap,
    },
};

pub mod syntax;

use crate::{
    app::{App, AppMode, Focus},
    db::models::{ObjectKind, TablePreview},
};

pub use syntax::highlight_sql;

pub fn render(frame: &mut Frame<'_>, app: &App) {
    match app.mode {
        AppMode::Editor => render_editor(frame, app),
        _ => render_browser(frame, app),
    }

    match app.mode {
        AppMode::Confirm => render_confirmation(frame, app),
        AppMode::Help => render_help(frame),
        _ => {}
    }
}

fn render_browser(frame: &mut Frame<'_>, app: &App) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(1)])
        .split(frame.area());

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(18), Constraint::Percentage(82)])
        .split(vertical[0]);

    let sidebar = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(36),
            Constraint::Percentage(30),
        ])
        .split(main[0]);

    let workspace = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(main[1]);

    let connections = app
        .config
        .connections
        .iter()
        .map(|profile| {
            let write = if profile.allow_writes { "RW" } else { "RO" };
            ListItem::new(format!("{}  [{}]", profile.name, write))
        })
        .collect::<Vec<_>>();
    render_list(
        frame,
        sidebar[0],
        "[1] Conexiones ",
        connections,
        app.connection_index,
        app.focus == Focus::Connections,
    );

    let databases = app
        .databases
        .iter()
        .map(|database| ListItem::new(database.as_str()))
        .collect::<Vec<_>>();
    render_list(
        frame,
        sidebar[1],
        "[2] Bases de datos ",
        databases,
        app.database_index,
        app.focus == Focus::Databases,
    );

    let kinds = ObjectKind::ALL
        .iter()
        .map(|kind| ListItem::new(kind.to_string()))
        .collect::<Vec<_>>();
    render_list(
        frame,
        sidebar[2],
        "[3] Objetos ",
        kinds,
        app.kind_index,
        app.focus == Focus::Kinds,
    );

    let objects = app
        .objects
        .iter()
        .map(|object| ListItem::new(format!("{}.{}", object.owner, object.name)))
        .collect::<Vec<_>>();
    render_list(
        frame,
        workspace[0],
        &format!("[4] {} ", app.current_kind()),
        objects,
        app.object_index,
        app.focus == Focus::Objects,
    );

    if let Some(preview) = &app.table_preview {
        render_table(frame, app, preview, workspace[1]);
    } else {
        let displayed_content = app
            .highlighted_content
            .clone()
            .unwrap_or_else(|| Text::from(app.content.clone()));
        let content = Paragraph::new(displayed_content)
            .block(panel_block(
                format!("[5] {} ", app.content_title),
                app.focus == Focus::Content,
            ))
            .wrap(Wrap { trim: false })
            .scroll((app.content_scroll, 0));
        frame.render_widget(content, workspace[1]);
    }

    render_status(frame, vertical[1], app);
}

fn render_table(frame: &mut Frame<'_>, app: &App, preview: &TablePreview, area: Rect) {
    let widths = vec![Constraint::Ratio(1, preview.columns.len() as u32); preview.columns.len()];

    let header = Row::new(preview.columns.iter().map(|column| column.as_str()))
        .style(Style::default().add_modifier(Modifier::BOLD));

    let rows = preview
        .rows
        .iter()
        .map(|values| Row::new(values.iter().map(|value| value.as_str())));

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .block(panel_block(
            format!("[5] {} ", app.content_title),
            app.focus == Focus::Content,
        ));

    frame.render_widget(table, area);
}

fn render_editor(frame: &mut Frame<'_>, app: &App) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(1)])
        .split(frame.area());

    let editor_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(vertical[0]);

    if let Some(session) = &app.editor {
        let title = format!(
            " {} · {}{} ",
            session.title,
            session.editor.mode,
            if session.editor.is_dirty() {
                " [+]"
            } else {
                ""
            }
        );
        let block = panel_block(title, true);
        let inner = block.inner(editor_layout[0]);
        frame.render_widget(block, editor_layout[0]);
        frame.render_widget(&session.editor.textarea, inner);
    } else {
        frame.render_widget(
            Paragraph::new("No hay una sesión de editor activa")
                .block(panel_block(" Editor ", true)),
            editor_layout[0],
        );
    }

    let result = Paragraph::new(app.content.as_str())
        .block(panel_block(format!(" {} ", app.content_title), false))
        .wrap(Wrap { trim: false })
        .scroll((app.content_scroll, 0));
    frame.render_widget(result, editor_layout[1]);

    render_status(frame, vertical[1], app);
}

fn render_list(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    items: Vec<ListItem<'_>>,
    selected: usize,
    active: bool,
) {
    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(selected.min(items.len() - 1)));
    }
    let list = List::new(items)
        .block(panel_block(title, active))
        .highlight_symbol("▸ ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED));
    frame.render_stateful_widget(list, area, &mut state);
}

fn panel_block(title: impl Into<String>, active: bool) -> Block<'static> {
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title.into());
    if active {
        block = block.border_style(
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        );
    }
    block
}

fn render_status(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let last_key_width = app.last_key.chars().count().saturating_add(3).min(28) as u16;
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(last_key_width.max(8)),
        ])
        .split(area);

    let profile = app
        .current_profile()
        .map(|profile| {
            if profile.allow_writes {
                format!("{}:RW", profile.name)
            } else {
                format!("{}:RO", profile.name)
            }
        })
        .unwrap_or_else(|| "sin conexión".to_owned());
    let database = app.active_database().unwrap_or("-");
    let busy = if app.busy_count > 0 {
        format!(" · tareas:{}", app.busy_count)
    } else {
        String::new()
    };
    let editor_mode = app
        .editor
        .as_ref()
        .map(|session| format!(" · {}", session.editor.mode))
        .unwrap_or_default();
    let left = format!(
        " {:?}{} · {} · {}{} · {} ",
        app.mode, editor_mode, profile, database, busy, app.status
    );
    frame.render_widget(Paragraph::new(left), split[0]);
    frame.render_widget(
        Paragraph::new(format!(" {} ", app.last_key))
            .alignment(Alignment::Right)
            .style(Style::default().add_modifier(Modifier::BOLD)),
        split[1],
    );
}

fn render_confirmation(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(72, 24, frame.area());
    frame.render_widget(Clear, area);
    let message = Paragraph::new(vec![
        Line::from(Span::styled(
            "Confirmación requerida",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(app.confirm_message()),
        Line::from(""),
        Line::from("y confirma · n/Esc cancela"),
    ])
    .alignment(Alignment::Center)
    .wrap(Wrap { trim: true })
    .block(panel_block(" Operación sensible ", true));
    frame.render_widget(message, area);
}

fn render_help(frame: &mut Frame<'_>) {
    let area = centered_rect(84, 82, frame.area());
    frame.render_widget(Clear, area);
    let help = [
        "NAVEGADOR",
        "  h/l o Tab     cambiar panel      j/k        mover selección",
        "  Enter         abrir/cargar        r          recargar panel",
        "  R             recargar conexiones c          probar conexión",
        "  p             preview tabla       e          editar SP/función/vista",
        "  E             editar datos T-SQL  :          consulta T-SQL",
        "  ?             ayuda               q          salir",
        "",
        "EDITOR NVIM-LIKE",
        "  i/a/A/I       insertar            Esc        NORMAL / cerrar",
        "  h/j/k/l       mover               w/b        palabra siguiente/anterior",
        "  0/$           inicio/fin línea    gg/G       inicio/fin archivo",
        "  o/O           línea debajo/arriba x          borrar carácter",
        "  dd/yy/p       cortar/copiar/pegar  u/Ctrl+r   deshacer/rehacer",
        "  v             selección visual    Ctrl+S     ejecutar/guardar",
        "",
        "SEGURIDAD",
        "  RO bloquea DDL/DML. En RW, toda escritura requiere confirmación.",
        "  Las credenciales pueden leerse de aseuserstore mediante userstore_key.",
        "",
        "Pulsa Esc, ? o q para cerrar esta ayuda.",
    ]
    .join("\n");
    frame.render_widget(
        Paragraph::new(help)
            .block(panel_block(" Ayuda ", true))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
