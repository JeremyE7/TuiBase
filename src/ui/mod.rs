use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    symbols::scrollbar::Set,
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Table, Wrap,
    },
};

pub mod syntax;

use crate::{
    app::{App, AppMode, Focus, TableCopyStage},
    db::models::ObjectKind,
};

pub use syntax::highlight_sql;

const PIN_ICON: &str = "\u{f08d}";
const TABLE_COLUMN_WIDTH: usize = 18;
const TABLE_COLUMN_SPACING: usize = 1;

pub fn render(frame: &mut Frame<'_>, app: &mut App) {
    match app.mode {
        AppMode::Editor => render_editor(frame, app),
        AppMode::Table => render_full_table(frame, app),
        _ => render_browser(frame, app),
    }

    if app.mode == AppMode::Table && app.current_table_sql_preview().is_some() {
        render_table_sql_preview(frame, app);
    } else if app.mode == AppMode::Table && app.current_table_changes_summary().is_some() {
        render_table_changes_summary(frame, app);
    } else if app.mode == AppMode::Table && app.current_table_date_time_picker().is_some() {
        render_table_date_time_picker(frame, app);
    } else if app.mode == AppMode::Table && app.current_table_cell_editor().is_some() {
        render_table_cell_editor(frame, app);
    } else if app.mode == AppMode::Table && app.current_table_value_modal().is_some() {
        render_table_value_modal(frame, app);
    } else if app.mode == AppMode::Table && app.current_filter_session().is_some() {
        render_table_filter_overlay(frame, app);
    } else if app.mode == AppMode::Table && app.current_sort_session().is_some() {
        render_table_sort_overlay(frame, app);
    } else if app.mode == AppMode::Table && app.current_column_search_session().is_some() {
        render_table_column_search_overlay(frame, app);
    } else if app.mode == AppMode::Table && app.table_copy_stage.is_some() {
        render_table_copy_overlay(frame, app);
    }

    if app.mode == AppMode::Table && app.current_execution_error_modal().is_some() {
        render_execution_error_modal(frame, app);
    }

    match app.mode {
        AppMode::Confirm => render_confirmation(frame, app),
        AppMode::Help => render_help(frame),
        AppMode::Search => render_search_overlay(frame, app),
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

    render_status(frame, vertical[1], app);
}

fn render_full_table(frame: &mut Frame<'_>, app: &mut App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(1)])
        .split(frame.area());
    let table_area = areas[0];
    let scrollbar_area = areas[1];

    if app.table_show_metadata {
        render_table_metadata(frame, table_area, app);
        render_status(frame, areas[1], app);
        return;
    }

    let Some(page) = app.table_page.as_ref() else {
        frame.render_widget(
            Paragraph::new("Cargando metadata y datos de la tabla...")
                .block(panel_block(" Tabla ", true)),
            frame.area(),
        );
        return;
    };

    if page.columns.is_empty() {
        frame.render_widget(
            Paragraph::new("La tabla no tiene columnas").block(panel_block(" Tabla ", true)),
            frame.area(),
        );
        return;
    }

    let total_columns = page.columns.len();
    let available_width = table_area.width.saturating_sub(2) as usize;
    let visible_columns = ((available_width + TABLE_COLUMN_SPACING)
        / (TABLE_COLUMN_WIDTH + TABLE_COLUMN_SPACING))
        .max(1)
        .min(total_columns);
    let max_offset = total_columns.saturating_sub(visible_columns);
    let position = app.horizontal_scroll.get_position().min(max_offset);
    let selected_column = app
        .table_column_index
        .saturating_sub(position)
        .min(visible_columns.saturating_sub(1));

    app.horizontal_scroll = ScrollbarState::new(total_columns)
        .position(position)
        .viewport_content_length(visible_columns);
    app.table_visible_columns = visible_columns;
    app.table_state.select_column(Some(selected_column));

    let column_range = position..position + visible_columns;
    let widths = vec![Constraint::Length(TABLE_COLUMN_WIDTH as u16); visible_columns];
    let pinned_columns = app.active_table_pinned_columns();

    let header = Row::new(column_range.clone().map(|index| {
        let column = &page.columns[index];
        if pinned_columns
            .iter()
            .any(|pinned| pinned.eq_ignore_ascii_case(column))
        {
            format!("{PIN_ICON} {column}")
        } else {
            column.clone()
        }
    }))
    .style(Style::default().add_modifier(Modifier::BOLD));

    let selected_row = app.table_state.selected();
    let visual_range = app.table_visual_anchor.map(|(anchor_row, anchor_column)| {
        let row = selected_row.unwrap_or(anchor_row);
        (
            anchor_row.min(row),
            anchor_row.max(row),
            anchor_column.min(app.table_column_index),
            anchor_column.max(app.table_column_index),
        )
    });
    let row_visual_range = app.table_row_visual_anchor.map(|anchor_row| {
        let row = selected_row.unwrap_or(anchor_row);
        (anchor_row.min(row), anchor_row.max(row))
    });
    let drafts = app.table_cell_drafts.clone();
    let row_deletions = app.table_row_deletions.clone();
    let new_rows = app.table_new_rows.clone();
    let rows = page.rows.iter().enumerate().map(|(row_index, values)| {
        Row::new(column_range.clone().map(|index| {
            let column = &page.columns[index];
            let draft = drafts.iter().find(|draft| {
                draft.row_index == row_index && draft.column.eq_ignore_ascii_case(column)
            });
            let value = draft
                .map(|draft| draft.value.as_str())
                .or_else(|| values.get(index).map(|value| value.as_str()))
                .unwrap_or("");
            let selected_row = row_visual_range
                .is_some_and(|(row_start, row_end)| row_index >= row_start && row_index <= row_end);
            let selected_cell =
                visual_range.is_some_and(|(row_start, row_end, column_start, column_end)| {
                    row_index >= row_start
                        && row_index <= row_end
                        && index >= column_start
                        && index <= column_end
                });
            let marked_for_deletion = row_deletions.contains(&row_index);
            let is_new_row = new_rows.contains(&row_index);
            if marked_for_deletion {
                Span::styled(
                    value,
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::Red)
                        .add_modifier(Modifier::CROSSED_OUT),
                )
            } else if is_new_row {
                Span::styled(
                    value,
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
            } else if selected_row || selected_cell {
                Span::styled(value, Style::default().bg(Color::Blue).fg(Color::White))
            } else if draft.is_some() {
                Span::styled(value, Style::default().fg(Color::Yellow))
            } else {
                Span::raw(value)
            }
        }))
    });

    let filter_label = app
        .active_table_filter()
        .map(|filter| format!(" · filtro: {filter}"))
        .unwrap_or_default();
    let sort_label = app
        .active_table_sort()
        .map(|sort| format!(" · orden: {sort}"))
        .unwrap_or_default();
    let draft_label = if app.table_cell_drafts.is_empty() {
        String::new()
    } else {
        format!(" · borradores: {}", app.table_cell_drafts.len())
    };
    let deletion_label = if app.table_row_deletions.is_empty() {
        String::new()
    } else {
        format!(" · borrar: {}", app.table_row_deletions.len())
    };
    let new_row_label = if app.table_new_rows.is_empty() {
        String::new()
    } else {
        format!(" · nuevas: {}", app.table_new_rows.len())
    };
    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(TABLE_COLUMN_SPACING as u16)
        .highlight_symbol("▸ ")
        .row_highlight_style(Style::default().bg(Color::Black).fg(Color::Yellow))
        .cell_highlight_style(Style::default().bg(Color::LightYellow).fg(Color::Black))
        .block(panel_block(
            format!(
                " Tabla · {} · {} cols · {} índices{}{}{}{}{}{} · Esc salir ",
                app.content_title,
                app.table_metadata
                    .as_ref()
                    .map_or(0, |metadata| metadata.columns.len()),
                app.table_metadata
                    .as_ref()
                    .map_or(0, |metadata| metadata.indexes.len()),
                if app.table_loading_more {
                    " · cargando"
                } else if page.has_more {
                    " · más filas disponibles"
                } else {
                    ""
                },
                filter_label,
                sort_label,
                draft_label,
                deletion_label,
                new_row_label,
            ),
            true,
        ));

    frame.render_stateful_widget(table, table_area, &mut app.table_state);
    render_pinned_column_boundary(
        frame,
        table_area,
        position,
        visible_columns,
        app.pinned_table_column_count(),
    );

    let scrollbar = Scrollbar::new(ScrollbarOrientation::HorizontalBottom).symbols(Set {
        track: "─",
        thumb: "━",
        begin: "‹",
        end: "›",
    });
    frame.render_stateful_widget(
        scrollbar,
        scrollbar_area.inner(Margin {
            vertical: 0,
            horizontal: 1,
        }),
        &mut app.horizontal_scroll,
    );
}

fn render_pinned_column_boundary(
    frame: &mut Frame<'_>,
    table_area: Rect,
    position: usize,
    visible_columns: usize,
    pinned_count: usize,
) {
    if pinned_count == 0
        || pinned_count <= position
        || pinned_count >= position.saturating_add(visible_columns)
    {
        return;
    }

    let relative_boundary = pinned_count - position;
    let inner = table_area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    let boundary_x = inner
        .x
        .saturating_add((relative_boundary * (TABLE_COLUMN_WIDTH + TABLE_COLUMN_SPACING)) as u16)
        .saturating_sub(TABLE_COLUMN_SPACING as u16);
    let border_area = Rect {
        x: boundary_x,
        y: inner.y,
        width: 1,
        height: inner.height,
    };
    let border = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::Yellow));
    frame.render_widget(border, border_area);
}

fn render_table_copy_overlay(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(64, 42, frame.area());
    frame.render_widget(Clear, area);
    let Some(stage) = app.table_copy_stage else {
        return;
    };
    let content = match stage {
        TableCopyStage::Menu => {
            let options = [
                "Datos cargados actualmente",
                "Fila actual",
                "Columna actual",
            ];
            let lines = options
                .iter()
                .enumerate()
                .map(|(index, option)| {
                    let marker = if index == app.table_copy_menu_index {
                        "▸"
                    } else {
                        " "
                    };
                    Line::from(format!("{marker} {option}"))
                })
                .collect::<Vec<_>>();
            Paragraph::new(Text::from(lines))
                .block(panel_block(" Copiar · j/k · Enter · Esc ", true))
        }
        TableCopyStage::HeaderChoice => {
            Paragraph::new("¿Copiar también la cabecera?\n\ny: sí\nn/Enter: no\nEsc: cancelar")
                .block(panel_block(" Copiar ", true))
        }
    };
    frame.render_widget(content, area);
}

fn render_table_changes_summary(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(84, 72, frame.area());
    frame.render_widget(Clear, area);

    let Some(summary) = app.current_table_changes_summary() else {
        return;
    };
    let block = panel_block(" Cambios pendientes · Enter SQL · Esc cerrar ", true);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![Line::from(Span::styled(
        "Resumen staged · no se ejecuta SQL",
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::from(""));

    lines.push(Line::from(format!(
        "Celdas editadas: {}",
        summary.edited_cells.len()
    )));
    if summary.edited_cells.is_empty() {
        lines.push(Line::from("  (ninguna)"));
    } else {
        lines.extend(
            summary
                .edited_cells
                .iter()
                .map(|change| Line::from(format!("  {change}"))),
        );
    }

    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "Filas nuevas: {}",
        summary.new_row_count
    )));
    lines.push(Line::from(format!(
        "Filas marcadas para borrar: {}",
        summary.deleted_rows.len()
    )));
    if !summary.deleted_rows.is_empty() {
        let rows = summary
            .deleted_rows
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(Line::from(format!("  Filas: {rows}")));
    }

    lines.push(Line::from(""));
    if summary.identity_columns.is_empty() {
        lines.push(Line::from("Identidad detectada: ninguna"));
    } else {
        lines.push(Line::from(format!(
            "Identidad detectada: {}",
            summary.identity_columns.join(", ")
        )));
    }
    if let Some(warning) = summary.identity_warning.as_deref() {
        lines.push(Line::from(Span::styled(
            format!("Advertencia: {warning}"),
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter abre la vista previa SQL; todavía no se ejecuta nada.",
        Style::default().fg(Color::Yellow),
    )));

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .scroll((app.table_changes_summary_scroll, 0)),
        inner,
    );
}

fn render_table_sql_preview(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(90, 82, frame.area());
    frame.render_widget(Clear, area);

    let Some(preview) = app.current_table_sql_preview() else {
        return;
    };
    let block = panel_block(
        " SQL staged · Ctrl+S ejecutar · Esc vuelve al resumen ",
        true,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    if preview.blockers.is_empty() {
        lines.push(Line::from(Span::styled(
            "No hay bloqueos de seguridad detectados.",
            Style::default().fg(Color::LightGreen),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "BLOQUEOS: estas operaciones no tienen SQL seguro generado",
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        )));
        lines.extend(preview.blockers.iter().map(|blocker| {
            Line::from(Span::styled(
                format!("  • {blocker}"),
                Style::default().fg(Color::LightRed),
            ))
        }));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "SQL generado para revisión:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "La ejecución está protegida por commit/rollback transaccional.",
        Style::default().fg(Color::LightCyan),
    )));
    lines.extend(
        preview
            .sql
            .lines()
            .map(|line| Line::from(Span::styled(line, Style::default().fg(Color::Gray)))),
    );

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .scroll((app.table_sql_preview_scroll, 0)),
        inner,
    );
}

fn render_table_date_time_picker(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(72, 44, frame.area());
    frame.render_widget(Clear, area);

    let Some(session) = app.current_table_date_time_picker() else {
        return;
    };
    let outer = panel_block(
        format!(
            " Picker · {} · fila {} · Enter guarda · Esc cancela ",
            session.column, session.row_number
        ),
        true,
    );
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(7),
            Constraint::Length(3),
        ])
        .split(inner);
    let metadata = Paragraph::new(format!(
        "Tipo: {} · NULL permitido: {}\nComponente: {}",
        session.data_type,
        if session.nullable { "sí" } else { "no" },
        table_date_time_component_name(session),
    ))
    .block(panel_block(" Metadata ", false));
    frame.render_widget(metadata, sections[0]);

    let selected_style = Style::default()
        .bg(Color::LightYellow)
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD);
    let component = |label: String, index: usize| {
        if session.selected_component == index && !session.is_null {
            Span::styled(label, selected_style)
        } else {
            Span::raw(label)
        }
    };
    let mut lines = Vec::new();
    if session.is_null {
        lines.push(Line::from(Span::styled(
            "Valor: <NULL>",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
    } else {
        if session.show_date {
            lines.push(Line::from(vec![
                Span::raw("Fecha: "),
                component(format!("{:04}", session.year), 0),
                Span::raw("-"),
                component(format!("{:02}", session.month), 1),
                Span::raw("-"),
                component(format!("{:02}", session.day), 2),
            ]));
        }
        if session.show_time {
            let offset = if session.show_date { 3 } else { 0 };
            let mut time = vec![
                Span::raw("Hora: "),
                component(format!("{:02}", session.hour), offset),
                Span::raw(":"),
                component(format!("{:02}", session.minute), offset + 1),
                Span::raw(":"),
                component(format!("{:02}", session.second), offset + 2),
            ];
            if !session.fractional_seconds.is_empty() {
                time.push(Span::raw(format!(".{}", session.fractional_seconds)));
            }
            lines.push(Line::from(time));
        }
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(panel_block(" Valor ", false)),
        sections[1],
    );

    frame.render_widget(
        Paragraph::new("Tab/Shift+Tab o ←/→ cambia · ↑/↓ ajusta · Space alterna NULL")
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(Color::Yellow)),
        sections[2],
    );
}

fn table_date_time_component_name(
    session: &crate::app::TableDateTimePickerSession,
) -> &'static str {
    if session.is_null {
        return "NULL";
    }
    let names = if session.show_date && session.show_time {
        ["año", "mes", "día", "hora", "minuto", "segundo"]
    } else if session.show_date {
        ["año", "mes", "día", "", "", ""]
    } else {
        ["hora", "minuto", "segundo", "", "", ""]
    };
    names[session.selected_component]
}

fn render_table_cell_editor(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(76, 46, frame.area());
    frame.render_widget(Clear, area);

    let Some(session) = app.current_table_cell_editor() else {
        return;
    };
    let outer = panel_block(
        format!(
            " Editar celda · {} · fila {} · Enter/Ctrl+S guarda · Esc cancela ",
            session.column, session.row_number
        ),
        true,
    );
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(inner);
    let metadata = Paragraph::new(format!(
        "Tipo: {} · NULL permitido: {}\nValor original: {}",
        session.data_type,
        if session.nullable { "sí" } else { "no" },
        session.original_value
    ))
    .block(panel_block(" Metadata ", false));
    frame.render_widget(metadata, sections[0]);

    let input_block = panel_block(" Valor borrador ", false);
    let input_inner = input_block.inner(sections[1]);
    frame.render_widget(input_block, sections[1]);
    frame.render_widget(&session.input, input_inner);

    frame.render_widget(
        Paragraph::new(app.status.as_str()).style(Style::default().fg(Color::Yellow)),
        sections[2],
    );
}

fn render_table_value_modal(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(78, 60, frame.area());
    frame.render_widget(Clear, area);

    let Some(modal) = app.current_table_value_modal() else {
        return;
    };
    let block = panel_block(
        format!(
            " Valor · {} · fila {} · Enter/Esc cerrar ",
            modal.column, modal.row_number
        ),
        true,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(4)])
        .split(inner);
    let metadata = Paragraph::new(format!(
        "Columna: {}\nTipo: {}",
        modal.column, modal.data_type
    ))
    .block(panel_block(" Metadata ", false));
    frame.render_widget(metadata, sections[0]);

    let value = Paragraph::new(modal.value.as_str())
        .block(panel_block(" Valor completo · j/k o ↑/↓ desplaza ", false))
        .wrap(Wrap { trim: false })
        .scroll((app.table_value_scroll, 0));
    frame.render_widget(value, sections[1]);
}

fn render_table_filter_overlay(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(82, 68, frame.area());
    frame.render_widget(Clear, area);

    let outer = panel_block(
        " Filtrar tabla · Tab completa · Enter aplica · Esc cancela ",
        true,
    );
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Min(3),
        ])
        .split(inner);
    let Some(session) = app.current_filter_session() else {
        return;
    };

    let input_block = panel_block(" Expresión ", false);
    frame.render_widget(input_block.clone(), sections[0]);
    frame.render_widget(&session.input, input_block.inner(sections[0]));

    let items = session
        .suggestions
        .iter()
        .map(|suggestion| ListItem::new(suggestion.label.clone()))
        .collect::<Vec<_>>();
    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(
            session
                .selected_suggestion
                .min(items.len().saturating_sub(1)),
        ));
    }
    let suggestions = List::new(items)
        .block(panel_block(" Sugerencias ", false))
        .highlight_symbol("▸ ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED));
    frame.render_stateful_widget(suggestions, sections[1], &mut state);

    let mut lines = vec![Line::from(Span::styled(
        "Condiciones interpretadas",
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    if session.preview.is_empty() {
        lines.push(Line::from(
            session
                .parse_error
                .as_deref()
                .unwrap_or("Escribe una condición para comenzar"),
        ));
    } else {
        lines.extend(
            session
                .preview
                .iter()
                .map(|condition| Line::from(format!("✓ {condition}"))),
        );
    }
    let preview = Paragraph::new(Text::from(lines))
        .block(panel_block(" Vista previa ", false))
        .wrap(Wrap { trim: true });
    frame.render_widget(preview, sections[2]);
}

fn render_table_sort_overlay(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(82, 64, frame.area());
    frame.render_widget(Clear, area);

    let outer = panel_block(
        " Ordenar tabla · Tab completa · Enter aplica · Esc cancela ",
        true,
    );
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Min(3),
        ])
        .split(inner);
    let Some(session) = app.current_sort_session() else {
        return;
    };

    let input_block = panel_block(" Expresión ", false);
    frame.render_widget(input_block.clone(), sections[0]);
    frame.render_widget(&session.input, input_block.inner(sections[0]));

    let items = session
        .suggestions
        .iter()
        .map(|suggestion| ListItem::new(suggestion.label.clone()))
        .collect::<Vec<_>>();
    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(
            session
                .selected_suggestion
                .min(items.len().saturating_sub(1)),
        ));
    }
    let suggestions = List::new(items)
        .block(panel_block(" Sugerencias ", false))
        .highlight_symbol("▸ ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED));
    frame.render_stateful_widget(suggestions, sections[1], &mut state);

    let mut lines = vec![Line::from(Span::styled(
        "Orden interpretado",
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    if session.preview.is_empty() {
        lines.push(Line::from(
            session
                .parse_error
                .as_deref()
                .unwrap_or("Escribe una columna para comenzar"),
        ));
    } else {
        lines.extend(
            session
                .preview
                .iter()
                .map(|sort| Line::from(format!("✓ {sort}"))),
        );
    }
    let preview = Paragraph::new(Text::from(lines))
        .block(panel_block(" Vista previa ", false))
        .wrap(Wrap { trim: true });
    frame.render_widget(preview, sections[2]);
}

fn render_table_column_search_overlay(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(72, 46, frame.area());
    frame.render_widget(Clear, area);

    let outer = panel_block(" Buscar columna · Enter salta · Esc cancela ", true);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3)])
        .split(inner);
    let Some(session) = app.current_column_search_session() else {
        return;
    };

    let input_block = panel_block(" Consulta ", false);
    frame.render_widget(input_block.clone(), sections[0]);
    frame.render_widget(&session.input, input_block.inner(sections[0]));

    let items = session
        .suggestions
        .iter()
        .map(|column| ListItem::new(column.clone()))
        .collect::<Vec<_>>();
    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(
            session
                .selected_suggestion
                .min(items.len().saturating_sub(1)),
        ));
    }
    let suggestions = List::new(items)
        .block(panel_block(" Columnas ", false))
        .highlight_symbol("▸ ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED));
    frame.render_stateful_widget(suggestions, sections[1], &mut state);
}

fn render_table_metadata(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let Some(metadata) = app.table_metadata.as_ref() else {
        frame.render_widget(
            Paragraph::new("La metadata todavía no está disponible")
                .block(panel_block(" Metadata · i volver ", true)),
            area,
        );
        return;
    };

    let mut lines = Vec::with_capacity(metadata.columns.len() + metadata.indexes.len() + 6);
    lines.push(Line::from("COLUMNAS"));
    lines.push(Line::from(
        "#   Nombre                         Tipo                 Null",
    ));
    lines.push(Line::from(
        "────────────────────────────────────────────────────────────",
    ));
    for column in &metadata.columns {
        lines.push(Line::from(format!(
            "{:<3} {:<30} {:<20} {}",
            column.ordinal_position,
            column.name,
            format_column_type(column),
            if column.nullable { "NULL" } else { "NOT NULL" }
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from("ÍNDICES"));
    if metadata.indexes.is_empty() {
        lines.push(Line::from("  (ninguno)"));
    } else {
        for index in &metadata.indexes {
            let mut flags = Vec::new();
            if index.is_unique {
                flags.push("unique");
            }
            if index.is_primary {
                flags.push("primary");
            }
            let suffix = if flags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", flags.join(", "))
            };
            lines.push(Line::from(format!(
                "  {}{} — {}",
                index.name,
                suffix,
                if index.columns.is_empty() {
                    "(columnas no informadas)".to_owned()
                } else {
                    index.columns.join(", ")
                }
            )));
        }
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(panel_block(
                format!(" Metadata · {} · i volver ", app.content_title),
                true,
            ))
            .scroll((app.content_scroll, 0))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn format_column_type(column: &crate::db::models::ColumnMetadata) -> String {
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

fn render_editor(frame: &mut Frame<'_>, app: &mut App) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(1)])
        .split(frame.area());

    let editor_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(vertical[0]);

    if let Some(session) = app.editor.as_mut() {
        let cursor = session.editor.textarea.cursor();
        let (cursor_row, cursor_col) = (cursor.0, cursor.1);
        let title = format!(
            " {} · {} · {}:{} {}",
            session.title,
            session.editor.mode,
            cursor_row + 1,
            cursor_col + 1,
            if session.editor.is_dirty() {
                "[+]"
            } else {
                ""
            }
        );
        let block = panel_block(title, true);
        let inner = block.inner(editor_layout[0]);
        frame.render_widget(block, editor_layout[0]);

        if inner.width > 0 && inner.height > 0 {
            let height = inner.height as usize;
            let width = inner.width as usize;
            {
                let scroll = &mut session.editor.scroll;
                let r = cursor_row as u16;
                if r < scroll.0 {
                    scroll.0 = r;
                } else if r >= scroll.0 + height as u16 {
                    scroll.0 = r - height as u16 + 1;
                }
                let c = cursor_col as u16;
                if c < scroll.1 {
                    scroll.1 = c;
                } else if c >= scroll.1 + width as u16 {
                    scroll.1 = c - width as u16 + 1;
                }
            }
            let scroll = session.editor.scroll;
            let content = session.editor.text();
            let base = crate::ui::syntax::highlight_sql(&content);
            let selection = session.editor.textarea.selection_range();
            let orig_lines: Vec<String> = session.editor.textarea.lines().iter().cloned().collect();
            let highlighted = apply_editor_selection(base, &orig_lines, selection);
            let paragraph = Paragraph::new(highlighted)
                .wrap(Wrap { trim: false })
                .scroll(scroll);
            frame.render_widget(paragraph, inner);
            let cursor_x = inner.x.saturating_add((cursor_col as u16).saturating_sub(scroll.1));
            let cursor_y = inner.y.saturating_add((cursor_row as u16).saturating_sub(scroll.0));
            if cursor_x < inner.x + inner.width && cursor_y < inner.y + inner.height {
                frame.set_cursor_position((cursor_x, cursor_y));
            }
        }
    } else {
        frame.render_widget(
            Paragraph::new("No hay una sesión de editor activa")
                .block(panel_block(" Editor ", true)),
            editor_layout[0],
        );
    }

    let console = console_text(app);
    let result = Paragraph::new(console)
        .block(panel_block(format!(" {} ", app.content_title), false))
        .wrap(Wrap { trim: false })
        .scroll((app.content_scroll, 0));
    frame.render_widget(result, editor_layout[1]);

    render_status(frame, vertical[1], app);
}

fn console_text(app: &App) -> Text<'static> {
    if app.content.is_empty() {
        return Text::from(Line::from(Span::styled(
            "Consola lista · Ctrl+S ejecuta · PgUp/PgDn desplaza · Ctrl+y copia · Ctrl+Shift+L limpia",
            Style::default().fg(Color::DarkGray),
        )));
    }
    let mut lines: Vec<Line<'static>> = Vec::new();
    for line in app.content.lines() {
        let lower = line.to_ascii_lowercase();
        let style = if lower.contains("msg ") && lower.contains("level")
            || lower.contains("server message")
            || lower.trim_start().starts_with("stderr")
            || lower.trim_start().starts_with("msg ")
        {
            Style::default().fg(Color::LightRed)
        } else if lower.contains("error") && app.console_success == Some(false) {
            Style::default().fg(Color::LightRed)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(line.to_owned(), style)));
    }
    Text::from(lines)
}

fn apply_editor_selection(
    base: Text<'static>,
    orig_lines: &[String],
    selection: Option<((usize, usize), (usize, usize))>,
) -> Text<'static> {
    let Some(((sr, sc), (er, ec))) = selection else {
        return base;
    };
    let mut base_lines = base.lines;
    if base_lines.len() < orig_lines.len() {
        base_lines.resize_with(orig_lines.len(), Line::default);
    } else if base_lines.len() > orig_lines.len() {
        base_lines.truncate(orig_lines.len());
    }
    let mut out_lines: Vec<Line<'static>> = Vec::with_capacity(base_lines.len());
    for (row, (mut base_line, orig_line)) in base_lines.into_iter().zip(orig_lines.iter()).enumerate() {
        if row < sr || row > er {
            out_lines.push(base_line);
            continue;
        }
        let line_start = if row == sr { sc } else { 0 };
        let line_end = if row == er {
            ec
        } else {
            orig_line.chars().count()
        };
        if line_start >= line_end {
            out_lines.push(base_line);
            continue;
        }
        if orig_line.is_empty() && base_line.width() == 0 {
            out_lines.push(Line::from(Span::styled(
                " ",
                Style::default().add_modifier(Modifier::REVERSED),
            )));
            continue;
        }
        if base_line.spans.is_empty() {
            out_lines.push(base_line);
            continue;
        }
        let mut new_spans: Vec<Span<'static>> = Vec::new();
        let mut col = 0usize;
        for span in base_line.spans.drain(..) {
            let span_content = span.content.into_owned();
            let span_style = span.style;
            let span_char_len = span_content.chars().count();
            let span_end = col + span_char_len;
            if span_end <= line_start || col >= line_end {
                new_spans.push(Span::styled(span_content, span_style));
            } else if col >= line_start && span_end <= line_end {
                new_spans.push(Span::styled(
                    span_content,
                    span_style.add_modifier(Modifier::REVERSED),
                ));
            } else {
                let mut chunk = String::new();
                let mut chunk_selected: Option<bool> = None;
                let mut cur_col = col;
                for ch in span_content.chars() {
                    let is_selected = cur_col >= line_start && cur_col < line_end;
                    match chunk_selected {
                        None => {
                            chunk_selected = Some(is_selected);
                            chunk.push(ch);
                        }
                        Some(prev) if prev == is_selected => {
                            chunk.push(ch);
                        }
                        Some(prev) => {
                            let style = if prev {
                                span_style.add_modifier(Modifier::REVERSED)
                            } else {
                                span_style
                            };
                            new_spans.push(Span::styled(std::mem::take(&mut chunk), style));
                            chunk.push(ch);
                            chunk_selected = Some(is_selected);
                        }
                    }
                    cur_col += 1;
                }
                if !chunk.is_empty() {
                    let is_selected = chunk_selected.unwrap_or(false);
                    let style = if is_selected {
                        span_style.add_modifier(Modifier::REVERSED)
                    } else {
                        span_style
                    };
                    new_spans.push(Span::styled(chunk, style));
                }
            }
            col = span_end;
        }
        out_lines.push(Line::from(new_spans));
    }
    Text::from(out_lines)
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
    let search = app
        .active_search_label()
        .map(|label| format!(" · {label}"))
        .unwrap_or_default();
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

    let status_style = if app.status.starts_with("ERROR") || app.status.starts_with("No se pudo") {
        Style::default().fg(Color::LightRed)
    } else if app.busy_count > 0 || app.status.contains("Actualizando") {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::LightGreen)
    };
    let left = Line::from(vec![
        Span::styled(
            format!(" {:?}{} ", app.mode, editor_mode),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled("· ", Style::default().fg(Color::DarkGray)),
        Span::styled(profile, Style::default().fg(Color::LightBlue)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled(database, Style::default().fg(Color::White)),
        Span::styled(search, Style::default().fg(Color::Magenta)),
        Span::styled(busy, Style::default().fg(Color::Yellow)),
        Span::styled(format!(" · {} ", app.status), status_style),
    ]);
    frame.render_widget(Paragraph::new(left), split[0]);
    frame.render_widget(
        Paragraph::new(format!(" {} ", app.last_key))
            .alignment(Alignment::Right)
            .style(
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
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

fn render_execution_error_modal(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(86, 60, frame.area());
    frame.render_widget(Clear, area);

    let mut lines = vec![
        Line::from(Span::styled(
            "ASE rechazó la ejecución",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    if let Some(error) = app.current_execution_error_modal() {
        lines.extend(error.lines().map(|line| Line::from(line.to_owned())));
    }
    lines.extend([
        Line::from(""),
        Line::from("Los cambios staged se conservaron."),
        Line::from("Enter/Esc cierra · j/k desplaza"),
    ]);

    let message = Paragraph::new(Text::from(lines))
        .block(panel_block(" Error de ejecución ", true))
        .wrap(Wrap { trim: false })
        .scroll((app.execution_error_scroll, 0));
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
        "  Enter         tabla/detalle       e          editar SP/función/vista",
        "  E             editar datos T-SQL  :          consulta T-SQL",
        "\nTABLA",
        "  Enter         ver valor completo de la celda",
        "  e             editar celda/picker y guardar borrador local",
        "  i             metadata de columnas e índices",
        "  y             copiar celda/metadata       Y menú de copia",
        "  v             selección visual            Esc cancela",
        "  Shift+V       selección visual de filas",
        "  d             marcar fila/rango para borrar",
        "  dd            copiar y marcar fila        u deshacer/descartar",
        "  +             fila nueva                 Shift+= clonar fila",
        "  j/k           desplazarse                h/l mover columna",
        "  c             buscar columna             p fijar/desfijar",
        "  f             filtro con autocompletado  F limpiar filtro",
        "  o             ordenar columnas           O limpiar orden",
        "  r             recargar datos (respeta orden/filtro)",
        "  Ctrl+S        resumen de cambios staged",
        "  /             búsqueda global             ? ayuda · q salir",
        "",
        "EDITOR NVIM-LIKE",
        "  i/a/A/I       insertar            Esc        NORMAL / cerrar",
        "  h/j/k/l       mover               w/b        palabra siguiente/anterior",
        "  0/$           inicio/fin línea    gg/G       inicio/fin archivo",
        "  o/O           línea debajo/arriba x          borrar carácter",
        "  dd/yy/p       cortar/copiar/pegar  u/Ctrl+r   deshacer/rehacer",
        "  v             selección visual    Ctrl+S     ejecutar/guardar",
        "  PgUp/PgDn/Home/End  desplazar consola  Ctrl+y copiar · Ctrl+Shift+L limpiar",
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

fn render_search_overlay(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(72, 42, frame.area());

    frame.render_widget(Clear, area);

    let outer_block = panel_block(" Buscar ", true);
    let inner = outer_block.inner(area);

    frame.render_widget(outer_block, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(inner);

    let Some(session) = app.current_search_session() else {
        return;
    };

    let input_block = panel_block(" Consulta ", false);
    let input_inner = input_block.inner(sections[0]);

    frame.render_widget(input_block, sections[0]);
    frame.render_widget(&session.input, input_inner);

    let items = session
        .suggestions
        .iter()
        .map(|suggestion| ListItem::new(suggestion.display_line()))
        .collect::<Vec<_>>();

    let mut state = ListState::default();

    if !items.is_empty() {
        state.select(Some(
            session
                .selected_suggestion
                .min(items.len().saturating_sub(1)),
        ));
    }

    let suggestions = List::new(items)
        .block(panel_block(" Sugerencias ", false))
        .highlight_symbol("▸ ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED));

    frame.render_stateful_widget(suggestions, sections[1], &mut state)
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
