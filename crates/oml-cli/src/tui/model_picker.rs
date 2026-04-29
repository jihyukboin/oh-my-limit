use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, List, ListItem, ListState, Paragraph},
};
use serde_json::Value;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ModelOption {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) is_default: bool,
    pub(crate) default_reasoning_effort: Option<String>,
    pub(crate) supported_reasoning_efforts: Vec<ReasoningEffortOption>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ReasoningEffortOption {
    pub(crate) effort: String,
    pub(crate) description: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ModelSelection {
    pub(crate) model: String,
    pub(crate) effort: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum PickerStage {
    Model,
    Reasoning { model_index: usize },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ModelPicker {
    options: Vec<ModelOption>,
    selected: usize,
    stage: PickerStage,
    active_model: Option<String>,
    active_effort: Option<String>,
}

impl ModelPicker {
    pub(crate) fn new(
        options: Vec<ModelOption>,
        active_model: Option<&str>,
        active_effort: Option<&str>,
    ) -> Self {
        let selected = active_model
            .and_then(|model| options.iter().position(|option| option.id == model))
            .or_else(|| options.iter().position(|option| option.is_default))
            .unwrap_or(0);

        Self {
            options,
            selected,
            stage: PickerStage::Model,
            active_model: active_model.map(str::to_owned),
            active_effort: active_effort.map(str::to_owned),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.options.is_empty()
    }

    pub(crate) fn select_previous(&mut self) {
        let len = self.visible_len();
        if len == 0 {
            return;
        }

        if self.selected == 0 {
            self.selected = len - 1;
        } else {
            self.selected -= 1;
        }
    }

    pub(crate) fn select_next(&mut self) {
        let len = self.visible_len();
        if len == 0 {
            return;
        }

        self.selected = (self.selected + 1) % len;
    }

    pub(crate) fn select_number(&mut self, number: usize) -> Option<ModelSelection> {
        if number == 0 || number > self.visible_len() {
            return None;
        }

        self.selected = number - 1;
        self.accept()
    }

    pub(crate) fn accept(&mut self) -> Option<ModelSelection> {
        match self.stage {
            PickerStage::Model => {
                let model_index = self.selected;
                let option = self.options.get(model_index)?;
                if option.supported_reasoning_efforts.len() <= 1 {
                    return Some(ModelSelection {
                        model: option.id.clone(),
                        effort: option
                            .supported_reasoning_efforts
                            .first()
                            .map(|effort| effort.effort.clone())
                            .or_else(|| option.default_reasoning_effort.clone()),
                    });
                }

                self.stage = PickerStage::Reasoning { model_index };
                self.selected = self.initial_reasoning_index(model_index);
                None
            }
            PickerStage::Reasoning { model_index } => {
                let option = self.options.get(model_index)?;
                let effort = option
                    .supported_reasoning_efforts
                    .get(self.selected)
                    .map(|effort| effort.effort.clone())
                    .or_else(|| option.default_reasoning_effort.clone());
                Some(ModelSelection {
                    model: option.id.clone(),
                    effort,
                })
            }
        }
    }

    pub(crate) fn cancel_or_back(&mut self) -> bool {
        match self.stage {
            PickerStage::Model => true,
            PickerStage::Reasoning { model_index } => {
                self.stage = PickerStage::Model;
                self.selected = model_index;
                false
            }
        }
    }

    fn visible_len(&self) -> usize {
        match self.stage {
            PickerStage::Model => self.options.len(),
            PickerStage::Reasoning { model_index } => self
                .options
                .get(model_index)
                .map(|option| option.supported_reasoning_efforts.len())
                .unwrap_or(0),
        }
    }

    fn initial_reasoning_index(&self, model_index: usize) -> usize {
        let Some(option) = self.options.get(model_index) else {
            return 0;
        };
        let target = if self.is_current_model(option) {
            self.active_effort
                .as_deref()
                .or(option.default_reasoning_effort.as_deref())
        } else {
            option.default_reasoning_effort.as_deref()
        };

        target
            .and_then(|target| {
                option
                    .supported_reasoning_efforts
                    .iter()
                    .position(|effort| effort.effort == target)
            })
            .unwrap_or(0)
    }

    fn is_current_model(&self, option: &ModelOption) -> bool {
        self.active_model.as_deref() == Some(option.id.as_str())
            || (self.active_model.is_none() && option.is_default)
    }
}

pub(crate) fn parse_model_options(value: &Value) -> Vec<ModelOption> {
    value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_model_option)
        .collect()
}

pub(crate) fn draw_model_picker(frame: &mut Frame<'_>, picker: &ModelPicker, area: Rect) {
    let area = centered_rect(96, 76, area);
    frame.render_widget(Clear, area);
    let content_width = area.width.saturating_sub(4) as usize;
    let rows = Layout::vertical([
        Constraint::Length(header_height(picker)),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .split(area);

    frame.render_widget(header(picker), rows[0]);

    let items = list_items(picker, content_width);
    let mut state = ListState::default();
    state.select(Some(picker.selected));
    let list = List::new(items)
        .highlight_symbol("› ")
        .highlight_style(Style::default().fg(Color::Cyan));
    frame.render_stateful_widget(list, rows[1], &mut state);

    frame.render_widget(
        Paragraph::new("Press enter to confirm or esc to go back")
            .style(Style::default().fg(Color::Gray)),
        rows[2],
    );
}

pub(crate) fn reasoning_effort_label(effort: &str) -> &'static str {
    match effort {
        "none" => "None",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra high",
        _ => "Custom",
    }
}

fn header(picker: &ModelPicker) -> Paragraph<'static> {
    let lines = match picker.stage {
        PickerStage::Model => vec![
            Line::from(Span::styled(
                "Select Model and Effort",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(
                "Access legacy models by running codex -m <model_name> or in your config.toml",
            )
            .style(Style::default().fg(Color::Gray)),
            Line::from(""),
        ],
        PickerStage::Reasoning { model_index } => {
            let model = picker
                .options
                .get(model_index)
                .map(|option| option.id.as_str())
                .unwrap_or("model");
            vec![
                Line::from(Span::styled(
                    format!("Select Reasoning Level for {model}"),
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
            ]
        }
    };
    Paragraph::new(lines)
}

fn header_height(picker: &ModelPicker) -> u16 {
    match picker.stage {
        PickerStage::Model => 3,
        PickerStage::Reasoning { .. } => 2,
    }
}

fn list_items(picker: &ModelPicker, content_width: usize) -> Vec<ListItem<'static>> {
    match picker.stage {
        PickerStage::Model => {
            let labels: Vec<String> = picker
                .options
                .iter()
                .map(|option| {
                    let mut label = option.id.clone();
                    if picker.is_current_model(option) {
                        label.push_str(" (current)");
                    } else if option.is_default {
                        label.push_str(" (default)");
                    }
                    label
                })
                .collect();
            let name_width = name_width(&labels);

            picker
                .options
                .iter()
                .zip(labels)
                .enumerate()
                .map(|(index, (option, label))| {
                    ListItem::new(row_lines(
                        index + 1,
                        label,
                        option.description.as_str(),
                        None,
                        name_width,
                        content_width,
                    ))
                })
                .collect()
        }
        PickerStage::Reasoning { model_index } => {
            let Some(option) = picker.options.get(model_index) else {
                return Vec::new();
            };
            let current_effort = if picker.is_current_model(option) {
                picker
                    .active_effort
                    .as_deref()
                    .or(option.default_reasoning_effort.as_deref())
            } else {
                None
            };
            let labels: Vec<String> = option
                .supported_reasoning_efforts
                .iter()
                .map(|effort| {
                    let mut label = reasoning_effort_label(&effort.effort).to_owned();
                    if Some(effort.effort.as_str()) == current_effort {
                        label.push_str(" (current)");
                    } else if Some(effort.effort.as_str())
                        == option.default_reasoning_effort.as_deref()
                    {
                        label.push_str(" (default)");
                    }
                    label
                })
                .collect();
            let name_width = name_width(&labels);
            let warning_effort = warning_effort(option);

            option
                .supported_reasoning_efforts
                .iter()
                .zip(labels)
                .enumerate()
                .map(|(index, (effort, label))| {
                    let selected_warning =
                        (index == picker.selected && Some(effort.effort.as_str()) == warning_effort)
                            .then_some(
                                "⚠ Extra high reasoning effort can quickly consume Plus plan rate limits.",
                            );
                    ListItem::new(row_lines(
                        index + 1,
                        label,
                        effort.description.as_str(),
                        selected_warning,
                        name_width,
                        content_width,
                    ))
                })
                .collect()
        }
    }
}

fn warning_effort(option: &ModelOption) -> Option<&str> {
    if !(option.id.starts_with("gpt-5.1-codex")
        || option.id.starts_with("gpt-5.1-codex-max")
        || option.id.starts_with("gpt-5.2"))
    {
        return None;
    }

    if option
        .supported_reasoning_efforts
        .iter()
        .any(|effort| effort.effort == "xhigh")
    {
        Some("xhigh")
    } else if option
        .supported_reasoning_efforts
        .iter()
        .any(|effort| effort.effort == "high")
    {
        Some("high")
    } else {
        None
    }
}

fn name_width(labels: &[String]) -> usize {
    labels
        .iter()
        .map(|label| label.chars().count())
        .max()
        .unwrap_or(0)
        .clamp(12, 32)
}

fn row_lines(
    index: usize,
    label: String,
    description: &str,
    selected_description: Option<&str>,
    name_width: usize,
    content_width: usize,
) -> Vec<Line<'static>> {
    let numbered_label = format!("{index}. {label}");
    let description_width = content_width.saturating_sub(name_width + 5).max(16);
    let mut lines = Vec::new();
    let descriptions = selected_description
        .map(|selected| {
            if description.is_empty() {
                selected.to_owned()
            } else {
                format!("{description}\n{selected}")
            }
        })
        .unwrap_or_else(|| description.to_owned());
    let mut wrapped = descriptions
        .lines()
        .flat_map(|line| wrap_words(line, description_width))
        .collect::<Vec<_>>();

    if wrapped.is_empty() {
        lines.push(Line::from(numbered_label));
        return lines;
    }

    let first = wrapped.remove(0);
    lines.push(Line::from(vec![
        Span::styled(
            pad_or_truncate(&numbered_label, name_width + 3),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(first),
    ]));

    let continuation_indent = " ".repeat(name_width + 5);
    lines.extend(
        wrapped
            .into_iter()
            .map(|line| Line::from(format!("{continuation_indent}{line}"))),
    );
    lines
}

fn parse_model_option(value: &Value) -> Option<ModelOption> {
    let hidden = value
        .get("hidden")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if hidden {
        return None;
    }

    let id = value.get("model").or_else(|| value.get("id"))?.as_str()?;
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let default_reasoning_effort = value
        .get("defaultReasoningEffort")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let supported_reasoning_efforts = value
        .get("supportedReasoningEfforts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_reasoning_effort)
        .collect();

    Some(ModelOption {
        id: id.to_owned(),
        description: description.to_owned(),
        is_default: value
            .get("isDefault")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        default_reasoning_effort,
        supported_reasoning_efforts,
    })
}

fn parse_reasoning_effort(value: &Value) -> Option<ReasoningEffortOption> {
    Some(ReasoningEffortOption {
        effort: value
            .get("reasoningEffort")
            .or_else(|| value.get("effort"))?
            .as_str()?
            .to_owned(),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
    })
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

fn pad_or_truncate(value: &str, width: usize) -> String {
    let count = value.chars().count();
    if count > width {
        let mut truncated = value
            .chars()
            .take(width.saturating_sub(3))
            .collect::<String>();
        truncated.push_str("...");
        return truncated;
    }

    format!("{value:<width$}")
}

fn wrap_words(value: &str, width: usize) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    for word in value.split_whitespace() {
        let separator = usize::from(!current.is_empty());
        if !current.is_empty() && current.chars().count() + separator + word.chars().count() > width
        {
            lines.push(current);
            current = String::new();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
