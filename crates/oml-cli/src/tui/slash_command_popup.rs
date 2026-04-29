use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding},
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SlashCommand {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SlashCommandPopup {
    commands: Vec<SlashCommand>,
    filter: String,
    selected: usize,
}

impl SlashCommandPopup {
    pub(crate) fn new(input: &str) -> Self {
        let mut popup = Self {
            commands: commands(),
            filter: String::new(),
            selected: 0,
        };
        popup.update(input);
        popup
    }

    pub(crate) fn update(&mut self, input: &str) {
        self.filter = slash_filter(input).unwrap_or_default();
        let len = self.filtered().len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    pub(crate) fn should_show(input: &str) -> bool {
        slash_filter(input).is_some()
    }

    pub(crate) fn select_previous(&mut self) {
        let len = self.filtered().len();
        if len == 0 {
            return;
        }
        self.selected = if self.selected == 0 {
            len - 1
        } else {
            self.selected - 1
        };
    }

    pub(crate) fn select_next(&mut self) {
        let len = self.filtered().len();
        if len == 0 {
            return;
        }
        self.selected = (self.selected + 1) % len;
    }

    pub(crate) fn selected_command(&self) -> Option<&'static str> {
        self.filtered()
            .get(self.selected)
            .map(|command| command.name)
    }

    pub(crate) fn completion_text(&self) -> Option<String> {
        self.selected_command()
            .map(|command| format!("/{command} "))
    }

    fn filtered(&self) -> Vec<&SlashCommand> {
        if self.filter.is_empty() {
            return self.commands.iter().collect();
        }

        self.commands
            .iter()
            .filter(|command| command.name.starts_with(&self.filter))
            .collect()
    }
}

pub(crate) fn draw_slash_command_popup(
    frame: &mut Frame<'_>,
    popup_state: &SlashCommandPopup,
    composer_area: Rect,
) {
    let width = composer_area.width.min(64);
    let filtered = popup_state.filtered();
    let visible_len = filtered.len().min(8) as u16;
    let height = visible_len.saturating_add(2).max(3);
    let x = composer_area.x;
    let y = composer_area.y.saturating_sub(height);
    let area = Rect {
        x,
        y,
        width,
        height,
    };

    let popup_window = Block::default()
        .title("Commands")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .padding(Padding::new(1, 1, 0, 0))
        .style(Style::default().bg(Color::Rgb(24, 24, 24)));
    frame.render_widget(Clear, area);
    frame.render_widget(popup_window.clone(), area);
    let area = popup_window.inner(area);

    let items = filtered
        .iter()
        .map(|command| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("/{:<14}", command.name),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(command.description, Style::default().fg(Color::Gray)),
            ]))
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default();
    if !filtered.is_empty() {
        state.select(Some(popup_state.selected.min(filtered.len() - 1)));
    }
    let list = List::new(items)
        .highlight_symbol("› ")
        .highlight_style(Style::default().fg(Color::Cyan).bg(Color::Rgb(24, 24, 24)))
        .style(Style::default().bg(Color::Rgb(24, 24, 24)));
    frame.render_stateful_widget(list, area, &mut state);
}

fn slash_filter(input: &str) -> Option<String> {
    let first_line = input.lines().next().unwrap_or("");
    let rest = first_line.strip_prefix('/')?;
    if rest.contains(' ') {
        return None;
    }
    Some(rest.to_owned())
}

fn commands() -> Vec<SlashCommand> {
    vec![
        SlashCommand {
            name: "translator",
            description: "configure prompt translation provider",
        },
        SlashCommand {
            name: "model",
            description: "choose model and reasoning effort",
        },
        SlashCommand {
            name: "status",
            description: "show current session status",
        },
        SlashCommand {
            name: "account",
            description: "refresh Codex account information",
        },
        SlashCommand {
            name: "usage",
            description: "show current usage and rate limits",
        },
        SlashCommand {
            name: "limits",
            description: "show current usage and rate limits",
        },
        SlashCommand {
            name: "diff",
            description: "show repository diff summary",
        },
        SlashCommand {
            name: "review",
            description: "review current changes",
        },
        SlashCommand {
            name: "compact",
            description: "compact the current thread",
        },
        SlashCommand {
            name: "list",
            description: "show recent Codex threads",
        },
        SlashCommand {
            name: "resume",
            description: "resume a Codex thread",
        },
        SlashCommand {
            name: "new",
            description: "start a new Codex thread",
        },
        SlashCommand {
            name: "clear",
            description: "clear the transcript",
        },
        SlashCommand {
            name: "interrupt",
            description: "interrupt the active Codex turn",
        },
        SlashCommand {
            name: "help",
            description: "show command help",
        },
        SlashCommand {
            name: "exit",
            description: "exit Oh My Limit",
        },
    ]
}
