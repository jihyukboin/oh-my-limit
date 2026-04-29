mod history_cell;
mod reflow;
mod stream;

use std::collections::VecDeque;

use ratatui::{layout::Rect, text::Line};

use self::{
    history_cell::{
        ApprovalRequestCell, AssistantMarkdownCell, ErrorCell, FinalMessageSeparatorCell,
        HistoryCell, PlanCell, SystemCell, ToolCallCell, UserPromptCell, render_history_tail,
    },
    reflow::TranscriptReflow,
    stream::AssistantStream,
};

#[derive(Debug, Default)]
pub(super) struct ChatWidget {
    history: Vec<Box<dyn HistoryCell>>,
    stream: AssistantStream,
    queued_inputs: VecDeque<String>,
    reflow: TranscriptReflow,
}

impl ChatWidget {
    pub(super) fn push_system(&mut self, text: impl Into<String>) {
        self.history.push(Box::new(SystemCell::new(text.into())));
    }

    pub(super) fn push_error(&mut self, text: impl Into<String>) {
        self.flush_assistant_stream();
        self.history.push(Box::new(ErrorCell::new(text.into())));
    }

    pub(super) fn push_approval(&mut self, text: impl Into<String>) {
        self.flush_assistant_stream();
        self.history
            .push(Box::new(ApprovalRequestCell::new(text.into())));
    }

    pub(super) fn push_tool_call(&mut self, label: String, text: String) {
        self.flush_assistant_stream();
        self.history.push(Box::new(ToolCallCell::new(label, text)));
    }

    pub(super) fn push_plan(&mut self, text: String) {
        self.flush_assistant_stream();
        self.history.push(Box::new(PlanCell::new(text)));
    }

    pub(super) fn push_final_separator(&mut self, label: Option<String>) {
        self.flush_assistant_stream();
        self.history
            .push(Box::new(FinalMessageSeparatorCell::new(label)));
    }

    pub(super) fn push_user_with_translation(
        &mut self,
        text: String,
        translated_text: Option<String>,
    ) {
        self.flush_assistant_stream();
        self.history
            .push(Box::new(UserPromptCell::new(text, translated_text)));
    }

    pub(super) fn set_last_user_translation(&mut self, translated_text: String) {
        if let Some(user) = self
            .history
            .iter_mut()
            .rev()
            .find_map(|cell| cell.as_any_mut().downcast_mut::<UserPromptCell>())
        {
            user.set_translated_text(translated_text);
        }
    }

    pub(super) fn start_assistant_message(&mut self) {
        self.stream.start();
    }

    pub(super) fn append_assistant_delta(&mut self, delta: &str) {
        self.stream.push_delta(delta);
    }

    pub(super) fn commit_stream(&mut self) -> bool {
        self.stream.commit_pending()
    }

    pub(super) fn replace_last_assistant_message(&mut self, text: String) {
        if self.stream.is_active() {
            self.stream.finish_with(text);
            self.flush_assistant_stream();
            return;
        }

        if let Some(assistant) = self
            .history
            .iter_mut()
            .rev()
            .find_map(|cell| cell.as_any_mut().downcast_mut::<AssistantMarkdownCell>())
        {
            assistant.set_source(text);
            return;
        }

        self.history
            .push(Box::new(AssistantMarkdownCell::new(text)));
    }

    pub(super) fn clear(&mut self) {
        self.history.clear();
        self.stream.clear();
        self.queued_inputs.clear();
    }

    pub(super) fn queue_user_input(&mut self, text: String) {
        self.queued_inputs.push_back(text);
    }

    pub(super) fn take_next_queued_input(&mut self) -> Option<String> {
        self.queued_inputs.pop_front()
    }

    pub(super) fn queued_input_lines(&self) -> Vec<String> {
        self.queued_inputs.iter().cloned().collect()
    }

    pub(super) fn display_lines(&self, area: Rect) -> Vec<Line<'static>> {
        let width = area.width;
        let height = area.height as usize;
        let mut lines = Vec::new();

        for cell in &self.history {
            if !lines.is_empty() {
                lines.push(Line::from(""));
            }
            let _ = cell.desired_height(width);
            let _ = cell.transcript_lines(width);
            let _ = cell.desired_transcript_height(width);
            let _ = cell.is_stream_continuation();
            let _ = cell.as_any();
            lines.extend(cell.display_lines(width));
        }

        if let Some(active) = self.stream.active_cell() {
            if !lines.is_empty() {
                lines.push(Line::from(""));
            }
            lines.extend(active.display_lines(width));
        }

        if lines.is_empty() {
            lines.push(Line::from("Connected TUI will appear here."));
        }

        render_history_tail(lines, height)
    }

    pub(super) fn prepare_reflow(&mut self, width: u16) -> bool {
        self.reflow.observe_width(width)
    }

    fn flush_assistant_stream(&mut self) {
        if let Some(source) = self.stream.take_finished_or_active()
            && !source.is_empty()
        {
            self.history
                .push(Box::new(AssistantMarkdownCell::new(source)));
        }
    }
}
