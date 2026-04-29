#[derive(Debug)]
pub(super) enum AppEvent {
    SetStatus(String),
    PushError(String),
    PushApproval(String),
    PushPlan(String),
    PushToolCall { label: String, text: String },
    PushFinalSeparator(Option<String>),
}
