mod textarea;

use std::collections::VecDeque;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph, Wrap},
};

use self::textarea::TextArea;
use super::slash_command_popup::SlashCommandPopup;

const USER_MESSAGE_BG: Color = Color::Rgb(31, 31, 31);

#[derive(Debug, Default)]
pub(super) struct BottomPane {
    textarea: TextArea,
    slash_popup: Option<SlashCommandPopup>,
    queued_messages: VecDeque<String>,
}

impl BottomPane {
    pub(super) fn set_input(&mut self, input: String) {
        self.textarea.set_text(input);
        self.sync_slash_popup();
    }

    pub(super) fn clear_input(&mut self) {
        self.textarea.clear();
        self.sync_slash_popup();
    }

    pub(super) fn take_trimmed_input(&mut self) -> String {
        let input = self.textarea.text().trim().to_owned();
        self.clear_input();
        input
    }

    pub(super) fn is_input_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    pub(super) fn slash_popup(&self) -> Option<&SlashCommandPopup> {
        self.slash_popup.as_ref()
    }

    pub(super) fn slash_popup_mut(&mut self) -> Option<&mut SlashCommandPopup> {
        self.slash_popup.as_mut()
    }

    pub(super) fn dismiss_slash_popup(&mut self) {
        self.slash_popup = None;
    }

    pub(super) fn sync_slash_popup(&mut self) {
        if SlashCommandPopup::should_show(self.textarea.text()) {
            if let Some(popup) = self.slash_popup.as_mut() {
                popup.update(self.textarea.text());
            } else {
                self.slash_popup = Some(SlashCommandPopup::new(self.textarea.text()));
            }
        } else {
            self.slash_popup = None;
        }
    }

    pub(super) fn complete_slash_command(&mut self) {
        let Some(completion) = self
            .slash_popup
            .as_ref()
            .and_then(SlashCommandPopup::completion_text)
        else {
            return;
        };

        self.set_input(completion);
    }

    pub(super) fn accept_slash_command(&mut self) -> bool {
        let Some(command) = self
            .slash_popup
            .as_ref()
            .and_then(SlashCommandPopup::selected_command)
        else {
            return false;
        };

        self.textarea.set_text(format!("/{command}"));
        self.slash_popup = None;
        true
    }

    pub(super) fn set_queued_messages(&mut self, queued_messages: Vec<String>) {
        self.queued_messages = queued_messages.into();
    }

    pub(super) fn insert_char(&mut self, character: char) {
        self.textarea.insert_char(character);
        self.sync_slash_popup();
    }

    pub(super) fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.textarea.insert_text(&text);
        self.sync_slash_popup();
    }

    pub(super) fn backspace(&mut self) {
        self.textarea.backspace();
        self.sync_slash_popup();
    }

    pub(super) fn delete(&mut self) {
        self.textarea.delete();
        self.sync_slash_popup();
    }

    pub(super) fn move_cursor_left(&mut self) {
        self.textarea.move_cursor_left();
    }

    pub(super) fn move_cursor_right(&mut self) {
        self.textarea.move_cursor_right();
    }

    pub(super) fn move_cursor_to_line_start(&mut self) {
        self.textarea.move_cursor_to_line_start();
    }

    pub(super) fn move_cursor_to_line_end(&mut self) {
        self.textarea.move_cursor_to_line_end();
    }

    pub(super) fn desired_height(&self, width: u16) -> u16 {
        let queued = self.queued_messages.len().min(3) as u16;
        let input_width = width.saturating_sub(2).max(1);
        let input_rows = self.textarea.visual_line_count(input_width).clamp(1, 6);
        queued + input_rows + 3
    }

    pub(super) fn render(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        cwd: &std::path::Path,
        status_line: String,
        task_running: bool,
        approval_required: bool,
    ) {
        if area.is_empty() {
            return;
        }

        let queued_count = self.queued_messages.len().min(3) as u16;
        let queue_area = Rect {
            x: area.x.saturating_add(2),
            y: area.y.saturating_add(1),
            width: area.width.saturating_sub(2),
            height: queued_count,
        };
        let text_area = Rect {
            x: area.x.saturating_add(2),
            y: area.y.saturating_add(1 + queued_count),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(3 + queued_count).max(1),
        };
        let input_background_area = Rect {
            x: area.x,
            y: text_area.y,
            width: area.width,
            height: text_area.height,
        };
        frame.render_widget(
            Block::default().style(user_message_style()),
            input_background_area,
        );

        let prompt_style = if task_running || approval_required {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        };
        frame.render_widget(
            Paragraph::new("›").style(prompt_style.bg(USER_MESSAGE_BG)),
            Rect {
                x: area.x,
                y: text_area.y,
                width: 1,
                height: 1,
            },
        );

        if queued_count > 0 {
            let queued = self
                .queued_messages
                .iter()
                .take(3)
                .map(|message| Line::from(format!("queued  {message}")))
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(queued).style(Style::default().fg(Color::DarkGray)),
                queue_area,
            );
        }

        if self.textarea.is_empty() {
            let placeholder = if approval_required {
                "Approval required"
            } else if task_running {
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
            let visible = self
                .textarea
                .visible_lines(text_area.width, text_area.height)
                .into_iter()
                .map(Line::from)
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(visible)
                    .style(Style::default().fg(Color::White).bg(USER_MESSAGE_BG))
                    .wrap(Wrap { trim: false }),
                text_area,
            );
        }

        let footer_y = area.bottom().saturating_sub(1);
        let left_text = truncate_to_width(&format!("  {}", cwd.display()), area.width as usize);
        let right_text = truncate_to_width(&status_line, area.width.saturating_sub(2) as usize);
        frame.render_widget(
            Paragraph::new(left_text).style(Style::default().fg(Color::DarkGray)),
            Rect {
                x: area.x,
                y: footer_y,
                width: area.width,
                height: 1,
            },
        );
        let right_width = right_text.chars().count() as u16;
        if right_width > 0 && right_width.saturating_add(2) < area.width {
            frame.render_widget(
                Paragraph::new(right_text).style(Style::default().fg(Color::DarkGray)),
                Rect {
                    x: area.right().saturating_sub(right_width + 1),
                    y: footer_y,
                    width: right_width,
                    height: 1,
                },
            );
        }
    }

    pub(super) fn cursor_pos(&self, area: Rect) -> (u16, u16) {
        let queued_count = self.queued_messages.len().min(3) as u16;
        let text_area = Rect {
            x: area.x.saturating_add(2),
            y: area.y.saturating_add(1 + queued_count),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(3 + queued_count).max(1),
        };
        self.textarea.cursor_position(text_area)
    }
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut output = String::new();
    for character in text.chars() {
        let character_width = unicode_width::UnicodeWidthChar::width(character).unwrap_or_default();
        if width + character_width > max_width {
            break;
        }
        output.push(character);
        width += character_width;
    }
    output
}

fn user_message_style() -> Style {
    Style::default().bg(USER_MESSAGE_BG)
}
