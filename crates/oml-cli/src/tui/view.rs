use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::{
    app::TuiState, model_picker::draw_model_picker, slash_command_popup::draw_slash_command_popup,
    translator_picker::draw_translator_picker,
};

pub(super) fn draw(frame: &mut Frame<'_>, app: &TuiState) {
    let area = frame.area();
    let bottom_pane_height = app.bottom_pane.desired_height(area.width);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(8),
            Constraint::Length(bottom_pane_height),
        ])
        .split(area);

    draw_dashboard(frame, app, rows[0]);
    draw_transcript(frame, app, rows[1]);
    draw_bottom_pane(frame, app, rows[2]);

    if let Some(popup) = app.bottom_pane.slash_popup() {
        draw_slash_command_popup(frame, popup, rows[2]);
    }

    if let Some(picker) = app.model_picker.as_ref() {
        draw_model_picker(frame, picker, area);
    }

    if let Some(picker) = app.translator_picker.as_ref() {
        draw_translator_picker(frame, picker, area);
    }
}

fn draw_dashboard(frame: &mut Frame<'_>, app: &TuiState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.is_empty() {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "Oh My Limit",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" for Codex  "),
        Span::styled(app.status.as_str(), Style::default().fg(Color::Gray)),
    ]));
    frame.render_widget(header, rows[0]);
    draw_limit_bar(frame, app, rows[1]);
}

fn model_summary(app: &TuiState) -> String {
    match (app.model.as_deref(), app.reasoning_effort.as_deref()) {
        (Some(model), Some(reasoning)) => format!("{model} {reasoning}"),
        (Some(model), None) => model.to_owned(),
        (None, Some(reasoning)) => format!("model pending {reasoning}"),
        (None, None) => "model pending".to_owned(),
    }
}

fn translator_source_label(app: &TuiState) -> String {
    if !app.config.translation.enabled {
        return "translator off".to_owned();
    }

    let provider = app.config.translation.provider.as_str();
    let location = match provider {
        "openai" => "remote",
        "ollama" | "local-openai-compatible" => "local",
        _ => provider,
    };
    let model = app.config.translation.model.as_deref().unwrap_or("default");
    format!("translator {location}-{model}")
}

fn codex_source_label(app: &TuiState) -> String {
    format!("codex {}", model_summary(app))
}

fn format_token_count(value: u64) -> String {
    let value = value.to_string();
    let mut formatted = String::with_capacity(value.len() + value.len() / 3);
    for (index, character) in value.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(character);
    }
    formatted.chars().rev().collect()
}

fn draw_limit_bar(frame: &mut Frame<'_>, app: &TuiState, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(2),
            Constraint::Percentage(50),
        ])
        .split(area);

    draw_limit_gauge(
        frame,
        columns[0],
        "5h limit",
        app.rate_limits.five_hour_percent,
    );
    draw_limit_gauge(
        frame,
        columns[2],
        "weekly limit",
        app.rate_limits.weekly_percent,
    );
}

fn draw_limit_gauge(frame: &mut Frame<'_>, area: Rect, title: &'static str, percent: Option<u16>) {
    let percent = percent.map(|percent| percent.min(100));
    let remaining_percent = percent.map(|percent| 100 - percent).unwrap_or_default();
    let title = percent
        .map(|_| format!("{title} {remaining_percent}%"))
        .unwrap_or_else(|| format!("{title} pending"));
    if area.is_empty() {
        return;
    }

    let label = Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled("remaining", Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(label), area);

    let inner = Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: area.height.saturating_sub(1).min(1),
    };
    if inner.is_empty() {
        return;
    }

    let fill_width = (u32::from(inner.width) * u32::from(remaining_percent) / 100) as u16;
    let bar_style = Style::default()
        .fg(percent.map(limit_color).unwrap_or(Color::DarkGray))
        .bg(Color::Black)
        .add_modifier(Modifier::BOLD);
    let empty_style = Style::default().fg(Color::DarkGray).bg(Color::Black);
    let buffer = frame.buffer_mut();

    buffer.set_style(inner, empty_style);
    for y in inner.top()..inner.bottom() {
        for x in inner.left()..inner.right() {
            buffer[(x, y)].set_symbol("░").set_style(empty_style);
        }
        for x in inner.left()..inner.left().saturating_add(fill_width) {
            buffer[(x, y)]
                .set_symbol(symbols::block::FULL)
                .set_style(bar_style);
        }
    }
}

fn limit_color(percent: u16) -> Color {
    match percent {
        90..=100 => Color::Red,
        70..=89 => Color::Yellow,
        _ => Color::Green,
    }
}

fn draw_transcript(frame: &mut Frame<'_>, app: &TuiState, area: Rect) {
    let transcript = Paragraph::new(app.chat.display_lines(area))
        .wrap(Wrap { trim: false })
        .block(Block::default());
    frame.render_widget(transcript, area);
}

fn draw_bottom_pane(frame: &mut Frame<'_>, app: &TuiState, area: Rect) {
    app.bottom_pane.render(
        frame,
        area,
        &app.cwd,
        footer_status_line(app),
        app.active_turn_id.is_some(),
        app.pending_approval.is_some(),
    );
    let (cursor_x, cursor_y) = app.bottom_pane.cursor_pos(area);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn footer_status_line(app: &TuiState) -> String {
    let translator = translator_source_label(app);
    let codex = codex_source_label(app);
    let translator_tokens = app
        .translator_usage
        .map(|usage| format_token_count(usage.input + usage.output))
        .unwrap_or_else(|| "0".to_owned());
    let codex_tokens = app
        .codex_usage
        .map(|usage| format_token_count(usage.input + usage.output))
        .unwrap_or_else(|| "0".to_owned());
    format!("{codex} | {translator} | tx {translator_tokens} | codex {codex_tokens}")
}
