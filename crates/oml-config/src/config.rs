use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub translation: TranslationConfig,
    pub privacy: PrivacyConfig,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TranslationConfig {
    pub provider: String,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    pub input_language: String,
    pub output_language: String,
    pub fail_closed: bool,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacyConfig {
    pub save_prompts: bool,
    pub redact_secrets: bool,
    pub remote_translation_allowed: bool,
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            provider: "noop".to_owned(),
            model: None,
            base_url: None,
            api_key_env: None,
            input_language: "ko".to_owned(),
            output_language: "ko".to_owned(),
            fail_closed: true,
            timeout_ms: 30_000,
        }
    }
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            save_prompts: false,
            redact_secrets: true,
            remote_translation_allowed: false,
        }
    }
}

impl AppConfig {
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("failed to parse config {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create config dir {}", parent.display()))?;
        }

        let text = toml::to_string_pretty(self).context("failed to encode config")?;
        fs::write(path, text).with_context(|| format!("failed to write config {}", path.display()))
    }
}
