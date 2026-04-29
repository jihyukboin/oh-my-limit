use std::{path::PathBuf, time::Instant};

use oml_codex_appserver::client::AccountSummary;
use oml_config::{
    config::AppConfig,
    env_file::load_env_file,
    paths::{config_file, env_file},
};
use serde_json::Value;

use super::model_picker::ModelPicker;
use super::slash_command_popup::SlashCommandPopup;
use super::translator_picker::TranslatorPicker;

#[derive(Debug)]
pub(super) struct TuiState {
    pub(super) cwd: PathBuf,
    pub(super) started_at: Instant,
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
    pub(super) input: String,
    pub(super) input_cursor: usize,
    pub(super) status: String,
    pub(super) transcript: Vec<TranscriptEntry>,
    pub(super) usage: Option<String>,
    pub(super) rate_limits: RateLimitUsage,
    pub(super) should_exit: bool,
    pub(super) pending_approval: Option<PendingApproval>,
    pub(super) slash_popup: Option<SlashCommandPopup>,
    pub(super) model_picker: Option<ModelPicker>,
    pub(super) translator_picker: Option<TranslatorPicker>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct RateLimitUsage {
    pub(super) five_hour_percent: Option<u16>,
    pub(super) weekly_percent: Option<u16>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct TranscriptEntry {
    pub(super) role: TranscriptRole,
    pub(super) text: String,
    pub(super) translated_text: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum TranscriptRole {
    User,
    Assistant,
    System,
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
            started_at: Instant::now(),
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
            input: String::new(),
            input_cursor: 0,
            status,
            transcript: Vec::new(),
            usage: None,
            rate_limits: RateLimitUsage::default(),
            should_exit: false,
            pending_approval: None,
            slash_popup: None,
            model_picker: None,
            translator_picker: None,
        }
    }

    pub(super) fn push_system(&mut self, text: impl Into<String>) {
        self.transcript.push(TranscriptEntry {
            role: TranscriptRole::System,
            text: text.into(),
            translated_text: None,
        });
    }

    pub(super) fn push_user(&mut self, text: String) {
        self.push_user_with_translation(text, None);
    }

    pub(super) fn push_user_with_translation(
        &mut self,
        text: String,
        translated_text: Option<String>,
    ) {
        self.transcript.push(TranscriptEntry {
            role: TranscriptRole::User,
            text,
            translated_text,
        });
    }

    pub(super) fn set_last_user_translation(&mut self, translated_text: String) {
        if let Some(entry) = self
            .transcript
            .iter_mut()
            .rev()
            .find(|entry| entry.role == TranscriptRole::User)
        {
            entry.translated_text = Some(translated_text);
        }
    }

    pub(super) fn start_assistant_message(&mut self) {
        self.transcript.push(TranscriptEntry {
            role: TranscriptRole::Assistant,
            text: String::new(),
            translated_text: None,
        });
    }

    pub(super) fn append_assistant_delta(&mut self, delta: &str) {
        if !matches!(
            self.transcript.last().map(|entry| &entry.role),
            Some(TranscriptRole::Assistant)
        ) {
            self.start_assistant_message();
        }

        if let Some(entry) = self.transcript.last_mut() {
            entry.text.push_str(delta);
        }
    }

    pub(super) fn replace_last_assistant_message(&mut self, text: String) {
        if let Some(entry) = self
            .transcript
            .iter_mut()
            .rev()
            .find(|entry| entry.role == TranscriptRole::Assistant)
        {
            entry.text = text;
            return;
        }

        self.transcript.push(TranscriptEntry {
            role: TranscriptRole::Assistant,
            text,
            translated_text: None,
        });
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
            "translator: {}; model: {model}; base_url: {base_url}; api_key_env: {api_key_env}; api_key: {api_key}; remote_allowed: {}",
            translation.provider, self.config.privacy.remote_translation_allowed
        )
    }

    pub(super) fn sync_slash_popup(&mut self) {
        if self.model_picker.is_some() || self.translator_picker.is_some() {
            self.slash_popup = None;
            return;
        }

        if SlashCommandPopup::should_show(&self.input) {
            if let Some(popup) = self.slash_popup.as_mut() {
                popup.update(&self.input);
            } else {
                self.slash_popup = Some(SlashCommandPopup::new(&self.input));
            }
        } else {
            self.slash_popup = None;
        }
    }
}
