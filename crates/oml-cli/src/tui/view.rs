use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    app::{TranscriptRole, TuiState},
    model_picker::draw_model_picker,
    slash_command_popup::draw_slash_command_popup,
    translator_picker::draw_translator_picker,
};

const USER_MESSAGE_BG: Color = Color::Rgb(31, 31, 31);

pub(super) fn draw(frame: &mut Frame<'_>, app: &TuiState) {
    let area = frame.area();
    let composer_height = composer_height(app);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9),
            Constraint::Min(8),
            Constraint::Length(composer_height),
        ])
        .split(area);

    draw_dashboard(frame, app, rows[0]);
    draw_transcript(frame, app, rows[1]);
    draw_composer(frame, app, rows[2]);

    if let Some(popup) = app.slash_popup.as_ref() {
        draw_slash_command_popup(frame, popup, rows[2]);
    }

    if let Some(picker) = app.model_picker.as_ref() {
        draw_model_picker(frame, picker, area);
    }

    if let Some(picker) = app.translator_picker.as_ref() {
        draw_translator_picker(frame, picker, area);
    }
}

fn composer_height(app: &TuiState) -> u16 {
    let input_lines = app.input.split('\n').count().max(1) as u16;
    input_lines.min(6) + 3
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
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
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
    draw_usage_table(frame, app, rows[1]);
    draw_limit_bar(frame, app, rows[2]);
}

fn draw_usage_table(frame: &mut Frame<'_>, app: &TuiState, area: Rect) {
    let table = Table::new(
        vec![
            token_usage_row(translator_source_label(app), app.translator_usage),
            token_usage_row(codex_source_label(app), app.codex_usage),
        ],
        [
            Constraint::Min(28),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
        ],
    )
    .header(
        Row::new(vec!["source", "input", "cached", "output"]).style(
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        ),
    );
    frame.render_widget(table, area);
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

fn token_usage_row(label: String, usage: Option<super::app::TokenUsage>) -> Row<'static> {
    let (input, cached, output) = match usage {
        Some(usage) => (
            format_token_count(usage.input),
            format_token_count(usage.cached),
            format_token_count(usage.output),
        ),
        None => ("0".to_owned(), "0".to_owned(), "0".to_owned()),
    };

    Row::new(vec![
        Cell::from(label).style(Style::default().fg(Color::Cyan)),
        Cell::from(input),
        Cell::from(cached),
        Cell::from(output),
    ])
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
    let height = area.height.saturating_sub(2) as usize;
    let mut lines = Vec::new();

    for entry in &app.transcript {
        let (label, style) = match entry.role {
            TranscriptRole::User => (
                "user",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            TranscriptRole::Assistant => (
                "codex",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            TranscriptRole::System => ("system", Style::default().fg(Color::DarkGray)),
        };
        let line_style = match entry.role {
            TranscriptRole::User => user_message_style(),
            TranscriptRole::Assistant | TranscriptRole::System => Style::default(),
        };

        lines.push(Line::from(Span::styled(label, style)).style(line_style));
        let text = if entry.text.is_empty() {
            "…".to_owned()
        } else {
            entry.text.clone()
        };
        lines.extend(
            text.lines()
                .map(|line| Line::from(format!("  {line}")).style(line_style)),
        );
        if entry.role == TranscriptRole::User
            && let Some(translated_text) = entry.translated_text.as_ref()
        {
            lines.push(Line::from(Span::styled(
                "  codex prompt",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.extend(translated_text.lines().map(|line| {
                Line::from(format!("    {line}")).style(Style::default().fg(Color::Gray))
            }));
        }
        lines.push(Line::from(""));
    }

    if lines.is_empty() {
        lines.push(Line::from("Connected TUI will appear here."));
    }

    let visible_lines = if lines.len() > height {
        lines.split_off(lines.len() - height)
    } else {
        lines
    };

    let transcript = Paragraph::new(visible_lines)
        .wrap(Wrap { trim: false })
        .block(Block::default());
    frame.render_widget(transcript, area);
}

fn draw_composer(frame: &mut Frame<'_>, app: &TuiState, area: Rect) {
    if area.is_empty() {
        return;
    }

    let text_area = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(3).max(1),
    };
    let prompt_style = if app.active_turn_id.is_some() || app.pending_approval.is_some() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    };
    let composer_style = user_message_style();
    let composer_area = Rect {
        x: area.x,
        y: text_area.y,
        width: area.width,
        height: text_area.height,
    };
    frame.render_widget(Block::default().style(composer_style), composer_area);
    frame.render_widget(
        Paragraph::new("›").style(prompt_style.bg(USER_MESSAGE_BG)),
        Rect {
            x: area.x,
            y: text_area.y,
            width: 1,
            height: 1,
        },
    );

    if app.input.is_empty() {
        let placeholder = if app.pending_approval.is_some() {
            "Approval required"
        } else if app.active_turn_id.is_some() {
            "Codex is responding..."
        } else {
            "Ask Codex to do anything"
        };
        frame.render_widget(
            Paragraph::new(placeholder)
                .style(Style::default().fg(Color::DarkGray).bg(USER_MESSAGE_BG)),
            text_area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(app.input.as_str())
                .style(Style::default().fg(Color::White).bg(USER_MESSAGE_BG))
                .wrap(Wrap { trim: false }),
            text_area,
        );
    }

    let footer_y = area.bottom().saturating_sub(1);
    let left_text = truncate_to_width(&format!("  {}", app.cwd.display()), area.width as usize);
    frame.render_widget(
        Paragraph::new(left_text).style(Style::default().fg(Color::DarkGray)),
        Rect {
            x: area.x,
            y: footer_y,
            width: area.width,
            height: 1,
        },
    );

    let (cursor_x, cursor_y) = input_cursor_position(app, text_area);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut output = String::new();
    for character in text.chars() {
        let character_width = character.width().unwrap_or_default();
        if width + character_width > max_width {
            break;
        }
        output.push(character);
        width += character_width;
    }
    output
}

fn input_cursor_position(app: &TuiState, area: Rect) -> (u16, u16) {
    let before_cursor = &app.input[..app.input_cursor.min(app.input.len())];
    let row = before_cursor.bytes().filter(|byte| *byte == b'\n').count() as u16;
    let column = before_cursor
        .rsplit_once('\n')
        .map_or(before_cursor, |(_, line)| line)
        .width() as u16;

    (
        area.x
            .saturating_add(column.min(area.width.saturating_sub(1))),
        area.y
            .saturating_add(row.min(area.height.saturating_sub(1))),
    )
}

fn user_message_style() -> Style {
    Style::default().bg(USER_MESSAGE_BG)
}
