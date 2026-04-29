use std::path::PathBuf;

use oml_codex_appserver::client::AccountSummary;
use oml_config::{
    config::AppConfig,
    env_file::load_env_file,
    paths::{config_file, env_file},
};
use serde_json::Value;

use super::app_event::AppEvent;
use super::bottom_pane::BottomPane;
use super::chat::ChatWidget;
use super::model_picker::ModelPicker;
use super::translator_picker::TranslatorPicker;

#[derive(Debug)]
pub(super) struct TuiState {
    pub(super) cwd: PathBuf,
    pub(super) account: Option<AccountSummary>,
    pub(super) thread_id: Option<String>,
    pub(super) active_turn_id: Option<String>,
    pub(super) model: Option<String>,
    pub(super) reasoning_effort: Option<String>,
    pub(super) config_path: PathBuf,
    pub(super) env_path: PathBuf,
    pub(super) config: AppConfig,
    pub(super) env_values: std::collections::HashMap<String, String>,
    pub(super) openai_api_key: Option<String>,
    pub(super) status: String,
    pub(super) chat: ChatWidget,
    pub(super) bottom_pane: BottomPane,
    pub(super) translator_usage: Option<TokenUsage>,
    pub(super) codex_usage: Option<TokenUsage>,
    pub(super) rate_limits: RateLimitUsage,
    pub(super) should_exit: bool,
    pub(super) pending_approval: Option<PendingApproval>,
    pub(super) model_picker: Option<ModelPicker>,
    pub(super) translator_picker: Option<TranslatorPicker>,
    app_events: std::collections::VecDeque<AppEvent>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct RateLimitUsage {
    pub(super) five_hour_percent: Option<u16>,
    pub(super) weekly_percent: Option<u16>,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(super) struct TokenUsage {
    pub(super) input: u64,
    pub(super) cached: u64,
    pub(super) output: u64,
}

#[derive(Debug, Clone)]
pub(super) struct PendingApproval {
    pub(super) id: Value,
    pub(super) method: String,
    pub(super) summary: String,
}

impl TuiState {
    pub(super) fn new() -> Self {
        let config_path = config_file();
        let env_path = env_file();
        let (config, config_status) = match AppConfig::load_or_default(&config_path) {
            Ok(config) => (config, None),
            Err(error) => (
                AppConfig::default(),
                Some(format!("Config load failed: {error}")),
            ),
        };
        let (env_values, env_status) = match load_env_file(&env_path) {
            Ok(env_values) => (env_values, None),
            Err(error) => (
                std::collections::HashMap::new(),
                Some(format!("Env load failed: {error}")),
            ),
        };
        let status = env_status
            .or(config_status)
            .unwrap_or_else(|| "Connecting to Codex app-server...".to_owned());

        Self {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            account: None,
            thread_id: None,
            active_turn_id: None,
            model: None,
            reasoning_effort: None,
            config_path,
            env_path,
            config,
            env_values,
            openai_api_key: None,
            status,
            chat: ChatWidget::default(),
            bottom_pane: BottomPane::default(),
            translator_usage: None,
            codex_usage: None,
            rate_limits: RateLimitUsage::default(),
            should_exit: false,
            pending_approval: None,
            model_picker: None,
            translator_picker: None,
            app_events: std::collections::VecDeque::new(),
        }
    }

    pub(super) fn emit(&mut self, event: AppEvent) {
        self.app_events.push_back(event);
    }

    pub(super) fn process_app_events(&mut self) {
        while let Some(event) = self.app_events.pop_front() {
            match event {
                AppEvent::SetStatus(status) => self.status = status,
                AppEvent::PushError(text) => self.push_error(text),
                AppEvent::PushApproval(text) => self.push_approval(text),
                AppEvent::PushPlan(text) => self.push_plan(text),
                AppEvent::PushToolCall { label, text } => self.push_tool_call(label, text),
                AppEvent::PushFinalSeparator(label) => self.push_final_separator(label),
            }
        }
    }

    pub(super) fn push_system(&mut self, text: impl Into<String>) {
        self.chat.push_system(text);
    }

    pub(super) fn push_error(&mut self, text: impl Into<String>) {
        self.chat.push_error(text);
    }

    pub(super) fn push_approval(&mut self, text: impl Into<String>) {
        self.chat.push_approval(text);
    }

    pub(super) fn push_final_separator(&mut self, label: Option<String>) {
        self.chat.push_final_separator(label);
    }

    pub(super) fn push_tool_call(&mut self, label: String, text: String) {
        self.chat.push_tool_call(label, text);
    }

    pub(super) fn push_plan(&mut self, text: String) {
        self.chat.push_plan(text);
    }

    pub(super) fn set_coding_model(&mut self, model: String, reasoning_effort: Option<String>) {
        self.model = Some(model);
        self.reasoning_effort = reasoning_effort;
    }

    pub(super) fn push_user(&mut self, text: String) {
        self.push_user_with_translation(text, None);
    }

    pub(super) fn push_user_with_translation(
        &mut self,
        text: String,
        translated_text: Option<String>,
    ) {
        self.chat.push_user_with_translation(text, translated_text);
    }

    pub(super) fn set_last_user_translation(&mut self, translated_text: String) {
        self.chat.set_last_user_translation(translated_text);
    }

    pub(super) fn start_assistant_message(&mut self) {
        self.chat.start_assistant_message();
    }

    pub(super) fn append_assistant_delta(&mut self, delta: &str) {
        self.chat.append_assistant_delta(delta);
    }

    pub(super) fn replace_last_assistant_message(&mut self, text: String) {
        self.chat.replace_last_assistant_message(text);
    }

    pub(super) fn account_line(&self) -> String {
        let Some(account) = self.account.as_ref() else {
            return "Codex auth: unknown".to_owned();
        };

        let account_type = account.account_type.as_deref().unwrap_or("unknown");
        let plan = account.plan_type.as_deref().unwrap_or("unknown");
        format!(
            "Codex auth: {account_type}; plan: {plan}; requires OpenAI auth: {}",
            account.requires_openai_auth
        )
    }

    pub(super) fn translator_line(&self) -> String {
        let translation = &self.config.translation;
        let model = translation.model.as_deref().unwrap_or("default");
        let base_url = translation.base_url.as_deref().unwrap_or("default");
        let api_key_env = translation.api_key_env.as_deref().unwrap_or("unset");
        let api_key = if self.openai_api_key.is_some() {
            "session"
        } else {
            "unset"
        };
        format!(
            "translator: {}; enabled: {}; model: {model}; base_url: {base_url}; api_key_env: {api_key_env}; api_key: {api_key}; remote_allowed: {}",
            translation.provider,
            translation.enabled,
            self.config.privacy.remote_translation_allowed
        )
    }

    pub(super) fn sync_slash_popup(&mut self) {
        if self.model_picker.is_some() || self.translator_picker.is_some() {
            self.bottom_pane.dismiss_slash_popup();
        } else {
            self.bottom_pane.sync_slash_popup();
        }
    }
}
