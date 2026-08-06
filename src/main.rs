mod app;
mod config;
mod db;
mod editor;
mod services;
mod ui;
mod worker;

use std::{io, panic, time::Duration};

use anyhow::{Context, Result};
use app::{App, AppMode};
use config::AppConfig;
use crossterm::{
    cursor::SetCursorStyle,
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

fn main() -> Result<()> {
    install_panic_hook();
    let (config, config_path) = AppConfig::load()?;
    let worker = worker::spawn_worker();
    let mut app = App::new(config, config_path, worker.requests, worker.responses);
    app.bootstrap();

    let mut terminal = init_terminal()?;
    let result = run(&mut terminal, &mut app);
    restore_terminal()?;
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    while !app.should_quit {
        app.poll_worker();
        update_cursor_style(app)?;
        terminal.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(80)).context("No se pudo consultar eventos")? {
            match event::read().context("No se pudo leer el evento del terminal")? {
                Event::Key(key) if key.kind == KeyEventKind::Press => app.handle_key(key),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }
    Ok(())
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode().context("No se pudo activar raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("No se pudo abrir la pantalla alterna")?;
    Terminal::new(CrosstermBackend::new(stdout)).context("No se pudo crear el terminal")
}

fn restore_terminal() -> Result<()> {
    disable_raw_mode().context("No se pudo desactivar raw mode")?;
    execute!(
        io::stdout(),
        SetCursorStyle::DefaultUserShape,
        LeaveAlternateScreen
    )
    .context("No se pudo restaurar el terminal")?;
    Ok(())
}

fn update_cursor_style(app: &App) -> Result<()> {
    let style = if app.mode == AppMode::Editor
        && app
            .editor
            .as_ref()
            .is_some_and(|session| session.editor.mode == editor::VimMode::Insert)
    {
        SetCursorStyle::SteadyBar
    } else {
        SetCursorStyle::SteadyBlock
    };
    let mut stdout = io::stdout();
    execute!(stdout, style).context("No se pudo cambiar el estilo del cursor")?;
    Ok(())
}

fn install_panic_hook() {
    let original = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            SetCursorStyle::DefaultUserShape,
            LeaveAlternateScreen
        );
        original(info);
    }));
}
