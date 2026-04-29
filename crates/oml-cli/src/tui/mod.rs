pub mod app;
pub mod composer;
pub mod event_loop;
pub mod panels;

use std::{
    io,
    path::PathBuf,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

const TICK_RATE: Duration = Duration::from_millis(250);

#[derive(Debug, Default)]
struct TuiState {
    cwd: PathBuf,
    started_at: Option<Instant>,
}

impl TuiState {
    fn new() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            started_at: Some(Instant::now()),
        }
    }
}

pub fn run() -> io::Result<()> {
    enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_loop(&mut terminal, TuiState::new());

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: TuiState,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| draw(frame, &app))?;

        if event::poll(TICK_RATE)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    _ => {}
                },
                _ => {}
            }
        }
    }

    Ok(())
}

fn draw(frame: &mut Frame<'_>, app: &TuiState) {
    let area = frame.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(5),
        ])
        .split(area);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "Oh My Limit",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" for Codex"),
    ]))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, rows[0]);

    let uptime = app
        .started_at
        .map(|started_at| started_at.elapsed().as_secs())
        .unwrap_or_default();
    let body = Paragraph::new(vec![
        Line::from(format!("cwd: {}", app.cwd.display())),
        Line::from("status: TUI shell ready"),
        Line::from("runner: codex app-server integration pending"),
        Line::from(""),
        Line::from("This is the Oh My Limit TUI entrypoint."),
        Line::from("Press q, Esc, or Ctrl+C to exit."),
        Line::from(format!("uptime: {uptime}s")),
    ])
    .wrap(Wrap { trim: true })
    .block(Block::default().title("Session").borders(Borders::ALL));
    frame.render_widget(body, rows[1]);

    let composer = Paragraph::new("oml> ")
        .style(Style::default().fg(Color::Green))
        .block(Block::default().title("Composer").borders(Borders::ALL));
    frame.render_widget(composer, rows[2]);
}
