use std::any::Any;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::stream::AssistantStreamingCell;
use crate::tui::{
    width::usable_content_width_u16,
    wrapping::{WrapOptions, adaptive_wrap_lines},
};

pub(super) trait HistoryCell: std::fmt::Debug + Send + Sync + Any {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>>;

    fn desired_height(&self, width: u16) -> u16 {
        self.display_lines(width).len().try_into().unwrap_or(0)
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.display_lines(width)
    }

    fn desired_transcript_height(&self, width: u16) -> u16 {
        self.transcript_lines(width).len().try_into().unwrap_or(0)
    }

    fn is_stream_continuation(&self) -> bool {
        false
    }

    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[derive(Debug)]
pub(super) struct UserPromptCell {
    text: String,
    translated_text: Option<String>,
}

impl UserPromptCell {
    pub(super) fn new(text: String, translated_text: Option<String>) -> Self {
        Self {
            text,
            translated_text,
        }
    }

    pub(super) fn set_translated_text(&mut self, translated_text: String) {
        self.translated_text = Some(translated_text);
    }
}

impl HistoryCell for UserPromptCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = prefixed_lines("› ".into(), "  ".into(), &self.text, width);
        if let Some(translated_text) = self.translated_text.as_ref() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "codex prompt",
                    ratatui::style::Style::default()
                        .fg(ratatui::style::Color::DarkGray)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
            ]));
            lines.extend(prefixed_lines(
                "    ".into(),
                "    ".into(),
                translated_text,
                width,
            ));
        }
        lines
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Debug)]
pub(super) struct AssistantMarkdownCell {
    source: String,
}

impl AssistantMarkdownCell {
    pub(super) fn new(source: String) -> Self {
        Self { source }
    }

    pub(super) fn set_source(&mut self, source: String) {
        self.source = source;
    }
}

impl HistoryCell for AssistantMarkdownCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.source.is_empty() {
            return vec![Line::from("• ...")];
        }
        prefixed_lines("• ".into(), "  ".into(), &self.source, width)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Debug)]
pub(super) struct SystemCell {
    text: String,
}

impl SystemCell {
    pub(super) fn new(text: String) -> Self {
        Self { text }
    }
}

impl HistoryCell for SystemCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        prefixed_lines("system  ".into(), "        ".into(), &self.text, width)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl HistoryCell for AssistantStreamingCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.source().is_empty() {
            return vec![Line::from("• ...")];
        }
        prefixed_lines("• ".into(), "  ".into(), self.source(), width)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Debug)]
pub(super) struct ErrorCell {
    text: String,
}

impl ErrorCell {
    pub(super) fn new(text: String) -> Self {
        Self { text }
    }
}

impl HistoryCell for ErrorCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        prefixed_lines(
            Line::from(Span::styled("error  ", Style::default().fg(Color::Red))),
            "       ".into(),
            &self.text,
            width,
        )
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Debug)]
pub(super) struct ApprovalRequestCell {
    text: String,
}

impl ApprovalRequestCell {
    pub(super) fn new(text: String) -> Self {
        Self { text }
    }
}

impl HistoryCell for ApprovalRequestCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        prefixed_lines(
            Line::from(Span::styled(
                "approve  ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            "         ".into(),
            &self.text,
            width,
        )
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Debug)]
pub(super) struct ToolCallCell {
    label: String,
    text: String,
}

impl ToolCallCell {
    pub(super) fn new(label: String, text: String) -> Self {
        Self { label, text }
    }
}

impl HistoryCell for ToolCallCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let prefix = format!("{}  ", self.label);
        prefixed_lines(
            prefix.clone().into(),
            " ".repeat(prefix.len()).into(),
            &self.text,
            width,
        )
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Debug)]
pub(super) struct PlanCell {
    text: String,
}

impl PlanCell {
    pub(super) fn new(text: String) -> Self {
        Self { text }
    }
}

impl HistoryCell for PlanCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        prefixed_lines("plan  ".into(), "      ".into(), &self.text, width)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Debug)]
pub(super) struct FinalMessageSeparatorCell {
    label: Option<String>,
}

impl FinalMessageSeparatorCell {
    pub(super) fn new(label: Option<String>) -> Self {
        Self { label }
    }
}

impl HistoryCell for FinalMessageSeparatorCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let label = self
            .label
            .as_ref()
            .map_or_else(|| "─".repeat(width as usize), |label| format!("─ {label} "));
        let mut text = label;
        if text.width() < width as usize {
            text.push_str(&"─".repeat(width as usize - text.width()));
        }
        vec![Line::from(Span::styled(
            text,
            Style::default().fg(Color::DarkGray),
        ))]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub(super) fn render_history_tail(
    mut lines: Vec<Line<'static>>,
    height: usize,
) -> Vec<Line<'static>> {
    if height == 0 || lines.len() <= height {
        return lines;
    }

    lines.split_off(lines.len() - height)
}

fn prefixed_lines(
    initial_prefix: Line<'static>,
    subsequent_prefix: Line<'static>,
    text: &str,
    width: u16,
) -> Vec<Line<'static>> {
    let Some(wrap_width) = usable_content_width_u16(
        width,
        initial_prefix.width().max(subsequent_prefix.width()) as u16,
    ) else {
        return vec![initial_prefix];
    };
    let source = if text.is_empty() {
        vec![Line::default()]
    } else {
        text.lines().map(Line::from).collect()
    };

    adaptive_wrap_lines(
        source,
        WrapOptions::new(wrap_width)
            .initial_indent(initial_prefix)
            .subsequent_indent(subsequent_prefix),
    )
}
