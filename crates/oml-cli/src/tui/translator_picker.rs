use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph},
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum TranslatorProviderSelection {
    Noop,
    Ollama,
    LocalOpenAiCompatible,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum TranslatorPickerAction {
    SelectedLocal(TranslatorProviderSelection),
    TestOpenAi { api_key: String },
    InvalidOpenAiApiKey,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum TranslatorPickerStage {
    Category,
    LocalProvider,
    RemoteProvider,
    OpenAiApiKey { value: String },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct TranslatorPicker {
    stage: TranslatorPickerStage,
    selected: usize,
}

impl TranslatorPicker {
    pub(crate) fn new() -> Self {
        Self {
            stage: TranslatorPickerStage::Category,
            selected: 0,
        }
    }

    pub(crate) fn select_previous(&mut self) {
        let len = self.visible_len();
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
        let len = self.visible_len();
        if len == 0 {
            return;
        }
        self.selected = (self.selected + 1) % len;
    }

    pub(crate) fn accept(&mut self) -> Option<TranslatorPickerAction> {
        match &mut self.stage {
            TranslatorPickerStage::Category => {
                self.stage = if self.selected == 0 {
                    TranslatorPickerStage::LocalProvider
                } else {
                    TranslatorPickerStage::RemoteProvider
                };
                self.selected = 0;
                None
            }
            TranslatorPickerStage::LocalProvider => {
                let selection = match self.selected {
                    0 => TranslatorProviderSelection::Ollama,
                    1 => TranslatorProviderSelection::LocalOpenAiCompatible,
                    _ => TranslatorProviderSelection::Noop,
                };
                Some(TranslatorPickerAction::SelectedLocal(selection))
            }
            TranslatorPickerStage::RemoteProvider => {
                self.stage = TranslatorPickerStage::OpenAiApiKey {
                    value: String::new(),
                };
                self.selected = 0;
                None
            }
            TranslatorPickerStage::OpenAiApiKey { value } => {
                let api_key = normalized_api_key(value);
                if api_key.is_empty() {
                    None
                } else if api_key.starts_with("sk-") && api_key.len() >= 40 {
                    Some(TranslatorPickerAction::TestOpenAi { api_key })
                } else {
                    Some(TranslatorPickerAction::InvalidOpenAiApiKey)
                }
            }
        }
    }

    pub(crate) fn select_number(&mut self, number: usize) -> Option<TranslatorPickerAction> {
        if number == 0 || number > self.visible_len() {
            return None;
        }
        self.selected = number - 1;
        self.accept()
    }

    pub(crate) fn cancel_or_back(&mut self) -> bool {
        match self.stage {
            TranslatorPickerStage::Category => true,
            TranslatorPickerStage::LocalProvider | TranslatorPickerStage::RemoteProvider => {
                self.stage = TranslatorPickerStage::Category;
                self.selected = 0;
                false
            }
            TranslatorPickerStage::OpenAiApiKey { .. } => {
                self.stage = TranslatorPickerStage::RemoteProvider;
                self.selected = 0;
                false
            }
        }
    }

    pub(crate) fn is_api_key_input(&self) -> bool {
        matches!(self.stage, TranslatorPickerStage::OpenAiApiKey { .. })
    }

    pub(crate) fn push_api_key_char(&mut self, character: char) {
        if let TranslatorPickerStage::OpenAiApiKey { value } = &mut self.stage {
            value.push(character);
        }
    }

    pub(crate) fn push_api_key_text(&mut self, text: &str) {
        if let TranslatorPickerStage::OpenAiApiKey { value } = &mut self.stage {
            value.extend(text.chars().filter(|character| !character.is_control()));
        }
    }

    pub(crate) fn pop_api_key_char(&mut self) {
        if let TranslatorPickerStage::OpenAiApiKey { value } = &mut self.stage {
            value.pop();
        }
    }

    fn visible_len(&self) -> usize {
        match self.stage {
            TranslatorPickerStage::Category => 2,
            TranslatorPickerStage::LocalProvider => 3,
            TranslatorPickerStage::RemoteProvider => 1,
            TranslatorPickerStage::OpenAiApiKey { .. } => 0,
        }
    }
}

pub(crate) fn draw_translator_picker(frame: &mut Frame<'_>, picker: &TranslatorPicker, area: Rect) {
    let area = modal_rect(area);
    let popup = Block::default()
        .title("Translator")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .padding(Padding::new(1, 1, 0, 0));
    frame.render_widget(Clear, area);
    frame.render_widget(popup.clone(), area);
    let area = popup.inner(area);

    let rows = Layout::vertical([
        Constraint::Length(header_height(picker)),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .split(area);

    frame.render_widget(header(picker), rows[0]);

    if picker.is_api_key_input() {
        frame.render_widget(api_key_input(picker), rows[1]);
    } else {
        let items = list_items(picker);
        let mut state = ListState::default();
        state.select(Some(picker.selected));
        let list = List::new(items)
            .highlight_symbol("› ")
            .highlight_style(Style::default().fg(Color::Cyan));
        frame.render_stateful_widget(list, rows[1], &mut state);
    }

    frame.render_widget(
        Paragraph::new("Enter confirm · Esc back").style(Style::default().fg(Color::Gray)),
        rows[2],
    );
}

fn header(picker: &TranslatorPicker) -> Paragraph<'static> {
    let title = match picker.stage {
        TranslatorPickerStage::Category => "Select Translator Type",
        TranslatorPickerStage::LocalProvider => "Select Local Provider",
        TranslatorPickerStage::RemoteProvider => "Select Remote Provider",
        TranslatorPickerStage::OpenAiApiKey { .. } => "OpenAI API Key",
    };
    let subtitle = match picker.stage {
        TranslatorPickerStage::Category => "Choose where prompt translation should run.",
        TranslatorPickerStage::LocalProvider => "Local providers keep prompts on this machine.",
        TranslatorPickerStage::RemoteProvider => {
            "Remote providers send prompts to an external API."
        }
        TranslatorPickerStage::OpenAiApiKey { .. } => {
            "The key is validated with one API call before the provider is enabled."
        }
    };

    Paragraph::new(vec![
        Line::from(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(subtitle, Style::default().fg(Color::Gray))),
        Line::from(""),
    ])
}

fn header_height(_picker: &TranslatorPicker) -> u16 {
    3
}

fn list_items(picker: &TranslatorPicker) -> Vec<ListItem<'static>> {
    match picker.stage {
        TranslatorPickerStage::Category => vec![
            row("1. Local", "Ollama or local OpenAI-compatible server"),
            row("2. Remote", "OpenAI API"),
        ],
        TranslatorPickerStage::LocalProvider => vec![
            row("1. Ollama", "Use a local Ollama model"),
            row(
                "2. OpenAI compatible",
                "Use a local /v1/chat/completions server",
            ),
            row("3. Off", "Disable prompt translation"),
        ],
        TranslatorPickerStage::RemoteProvider => {
            vec![row("1. OpenAI API", "Use OpenAI Responses API")]
        }
        TranslatorPickerStage::OpenAiApiKey { .. } => Vec::new(),
    }
}

fn api_key_input(picker: &TranslatorPicker) -> Paragraph<'static> {
    let value = match &picker.stage {
        TranslatorPickerStage::OpenAiApiKey { value } => value,
        _ => "",
    };
    let masked = if value.is_empty() {
        "Paste API key and press Enter".to_owned()
    } else {
        "*".repeat(value.chars().count().min(64))
    };
    Paragraph::new(vec![
        Line::from(masked),
        Line::from(""),
        Line::from(Span::styled(
            "Whitespace is ignored. The raw key is kept in memory for this TUI session.",
            Style::default().fg(Color::Gray),
        )),
    ])
}

fn row(name: &'static str, description: &'static str) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::styled(
            format!("{name:<24}"),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(description, Style::default().fg(Color::Gray)),
    ]))
}

fn modal_rect(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).clamp(48, 92).min(area.width);
    let height = area.height.saturating_sub(6).clamp(14, 20).min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn normalized_api_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}
