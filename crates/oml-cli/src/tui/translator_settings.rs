use std::{env, path::Path, str::FromStr, time::Duration};

use oml_config::{config::AppConfig, env_file::save_env_value};
use oml_translation::translator::{TranslationProviderKind, TranslatorConfig, build_translator};

use super::{
    app::TuiState,
    translator_picker::{TranslatorPicker, TranslatorPickerAction, TranslatorProviderSelection},
};

pub(super) async fn handle_translator_command(app: &mut TuiState, command: &str) {
    let rest = command.trim_start_matches("/translator ").trim();
    let mut parts = rest.splitn(2, ' ');
    let key = parts.next().unwrap_or_default();
    let value = parts.next().unwrap_or_default().trim();

    let result = match key {
        "on" if value.is_empty() => {
            open_translator_provider_picker(app);
            return;
        }
        "off" if value.is_empty() => set_translation_enabled(app, false),
        "provider" => set_translator_provider(app, value),
        "model" => set_optional_translation_value(app, "model", value),
        "base-url" => set_optional_translation_value(app, "base-url", value),
        "api-key-env" => set_optional_translation_value(app, "api-key-env", value),
        "remote" => set_remote_translation(app, value),
        "test" if value.is_empty() => test_translator(app).await,
        _ => Err("Usage: /translator on, /translator off, /translator provider <noop|ollama|local-openai-compatible|openai>, /translator model <name>, /translator base-url <url|default>, /translator api-key-env <ENV>, /translator remote <on|off>, /translator test".to_owned()),
    };

    match result {
        Ok(message) => {
            app.push_system(message.clone());
            app.status = message;
        }
        Err(error) => {
            app.status = error;
        }
    }
}

pub(super) fn open_translator_picker(app: &mut TuiState) {
    if app.model_picker.is_some() {
        app.model_picker = None;
    }

    app.translator_picker = Some(TranslatorPicker::new());
    app.status = "Select a translator and settings.".to_owned();
}

fn open_translator_provider_picker(app: &mut TuiState) {
    if app.model_picker.is_some() {
        app.model_picker = None;
    }

    app.translator_picker = Some(TranslatorPicker::provider_list(
        &app.config.translation.provider,
        has_openai_api_key(app),
    ));
    app.status = "Select a translator provider to enable.".to_owned();
}

pub(super) async fn apply_translator_selection(
    app: &mut TuiState,
    selection: TranslatorPickerAction,
) -> Result<(), String> {
    let test_on_apply = matches!(selection, TranslatorPickerAction::TestOpenAi { .. });
    match selection {
        TranslatorPickerAction::InvalidOpenAiApiKey => {
            app.status =
                "OpenAI API key looks incomplete. Paste the full key and press Enter.".to_owned();
            return Ok(());
        }
        TranslatorPickerAction::SelectedLocal(selection) => match selection {
            TranslatorProviderSelection::Noop => app.config.translation.enabled = false,
            TranslatorProviderSelection::Ollama => {
                app.config.translation.enabled = true;
                app.config.translation.provider =
                    TranslationProviderKind::Ollama.as_str().to_owned();
                app.config.privacy.remote_translation_allowed = false;
                app.config.translation.api_key_env = None;
                app.openai_api_key = None;
            }
            TranslatorProviderSelection::LocalOpenAiCompatible => {
                app.config.translation.enabled = true;
                app.config.translation.provider = TranslationProviderKind::LocalOpenAiCompatible
                    .as_str()
                    .to_owned();
                app.config.privacy.remote_translation_allowed = false;
                app.config.translation.api_key_env = None;
                app.openai_api_key = None;
            }
        },
        TranslatorPickerAction::SelectedOpenAi => {
            app.config.translation.enabled = true;
            app.config.translation.provider = TranslationProviderKind::OpenAi.as_str().to_owned();
            app.config.privacy.remote_translation_allowed = true;
            app.config.translation.model = app
                .config
                .translation
                .model
                .clone()
                .or_else(|| Some("gpt-5.4-mini".to_owned()));
            app.config.translation.base_url = app
                .config
                .translation
                .base_url
                .clone()
                .or_else(|| Some("https://api.openai.com/v1".to_owned()));
            app.config.translation.api_key_env = app
                .config
                .translation
                .api_key_env
                .clone()
                .or_else(|| Some("OPENAI_API_KEY".to_owned()));
        }
        TranslatorPickerAction::TestOpenAi { api_key } => {
            let model = app
                .config
                .translation
                .model
                .clone()
                .or_else(|| Some("gpt-5.4-mini".to_owned()));
            let base_url = app
                .config
                .translation
                .base_url
                .clone()
                .or_else(|| Some("https://api.openai.com/v1".to_owned()));
            test_openai_api_key(app, api_key.clone(), model.clone(), base_url.clone()).await?;

            app.config.translation.provider = TranslationProviderKind::OpenAi.as_str().to_owned();
            app.config.translation.enabled = true;
            app.config.privacy.remote_translation_allowed = true;
            app.config.translation.model = model;
            app.config.translation.base_url = base_url;
            app.config.translation.api_key_env = None;
            app.openai_api_key = Some(api_key);
        }
    }

    if test_on_apply {
        if let Some(api_key) = app.openai_api_key.as_deref() {
            save_env_value(&app.env_path, "OPENAI_API_KEY", api_key)
                .map_err(|error| format!("Failed to save env file: {error}"))?;
            app.env_values
                .insert("OPENAI_API_KEY".to_owned(), api_key.to_owned());
            app.config.translation.api_key_env = Some("OPENAI_API_KEY".to_owned());
            app.openai_api_key = None;
        }
        save_config(&app.config, &app.config_path)?;
        app.translator_picker = None;
        let message = "Translator test passed: openai (OpenAI API reachable)".to_owned();
        app.push_system(message.clone());
        app.status = message;
        return Ok(());
    }

    save_config(&app.config, &app.config_path)?;
    app.translator_picker = None;

    let message = if app.config.translation.enabled {
        format!(
            "Prompt translation enabled with {}.",
            app.config.translation.provider
        )
    } else {
        "Prompt translation disabled.".to_owned()
    };
    app.push_system(message.clone());
    app.status = message;

    Ok(())
}

fn set_translator_provider(app: &mut TuiState, value: &str) -> Result<String, String> {
    let provider = TranslationProviderKind::from_str(value).map_err(|error| error.to_string())?;
    app.config.translation.provider = provider.as_str().to_owned();
    app.config.translation.enabled = provider != TranslationProviderKind::Noop;

    if provider == TranslationProviderKind::OpenAi {
        app.config.translation.model = app
            .config
            .translation
            .model
            .clone()
            .or_else(|| Some("gpt-5.4-mini".to_owned()));
        app.config.translation.base_url = app
            .config
            .translation
            .base_url
            .clone()
            .or_else(|| Some("https://api.openai.com/v1".to_owned()));
        app.config.translation.api_key_env = app
            .config
            .translation
            .api_key_env
            .clone()
            .or_else(|| Some("OPENAI_API_KEY".to_owned()));
    }

    save_config(&app.config, &app.config_path)?;
    Ok(format!(
        "Translator provider set to {}.",
        app.config.translation.provider
    ))
}

fn set_translation_enabled(app: &mut TuiState, enabled: bool) -> Result<String, String> {
    app.config.translation.enabled = enabled;
    save_config(&app.config, &app.config_path)?;
    Ok(format!(
        "Prompt translation {}.",
        if enabled { "enabled" } else { "disabled" }
    ))
}

fn set_optional_translation_value(
    app: &mut TuiState,
    key: &str,
    value: &str,
) -> Result<String, String> {
    if value.is_empty() {
        return Err(format!("Usage: /translator {key} <value|default>"));
    }

    let normalized = if value == "default" {
        None
    } else {
        Some(value.to_owned())
    };
    match key {
        "model" => app.config.translation.model = normalized,
        "base-url" => app.config.translation.base_url = normalized,
        "api-key-env" => app.config.translation.api_key_env = normalized,
        _ => return Err(format!("Unknown translator setting: {key}")),
    }

    save_config(&app.config, &app.config_path)?;
    Ok(format!("Translator {key} updated."))
}

fn set_remote_translation(app: &mut TuiState, value: &str) -> Result<String, String> {
    let enabled = match value {
        "on" | "true" | "yes" => true,
        "off" | "false" | "no" => false,
        _ => return Err("Usage: /translator remote <on|off>".to_owned()),
    };

    app.config.privacy.remote_translation_allowed = enabled;
    save_config(&app.config, &app.config_path)?;
    Ok(format!(
        "Remote translation {}.",
        if enabled { "enabled" } else { "disabled" }
    ))
}

fn has_openai_api_key(app: &TuiState) -> bool {
    app.openai_api_key.is_some()
        || app
            .config
            .translation
            .api_key_env
            .as_ref()
            .is_some_and(|name| app.env_values.contains_key(name))
}

async fn test_translator(app: &mut TuiState) -> Result<String, String> {
    let config = translator_config(app).map_err(|error| error.to_string())?;
    let translator = build_translator(config);
    let health = translator
        .health_check()
        .await
        .map_err(|error| format!("Translator test failed: {error}"))?;

    Ok(format!(
        "Translator test passed: {} ({})",
        health.provider.as_str(),
        health.message
    ))
}

async fn test_openai_api_key(
    app: &TuiState,
    api_key: String,
    model: Option<String>,
    base_url: Option<String>,
) -> Result<(), String> {
    let translator = build_translator(TranslatorConfig {
        provider: TranslationProviderKind::OpenAi,
        model,
        base_url,
        api_key: Some(api_key),
        timeout: Duration::from_millis(app.config.translation.timeout_ms),
    });

    translator
        .health_check()
        .await
        .map(|_| ())
        .map_err(|error| format!("Translator test failed: {error}"))
}

pub(super) fn translator_config(app: &TuiState) -> anyhow::Result<TranslatorConfig> {
    let provider = TranslationProviderKind::from_str(&app.config.translation.provider)?;
    if provider.is_remote() && !app.config.privacy.remote_translation_allowed {
        anyhow::bail!("remote translation is disabled; run /translator remote on first");
    }

    let api_key = match (
        provider,
        app.openai_api_key.as_ref(),
        app.config.translation.api_key_env.as_ref(),
    ) {
        (TranslationProviderKind::OpenAi, Some(key), _) => Some(key.clone()),
        (TranslationProviderKind::OpenAi, None, Some(name))
            if app.env_values.contains_key(name) =>
        {
            app.env_values.get(name).cloned()
        }
        (TranslationProviderKind::OpenAi, None, Some(name)) => Some(
            env::var(name)
                .map_err(|_| anyhow::anyhow!("environment variable {name} is not set"))?,
        ),
        (TranslationProviderKind::OpenAi, None, None) => {
            anyhow::bail!("OpenAI provider requires an API key from /translator or api-key-env")
        }
        (_, _, Some(name)) => app
            .env_values
            .get(name)
            .cloned()
            .or_else(|| env::var(name).ok()),
        (_, _, None) => None,
    };

    Ok(TranslatorConfig {
        provider,
        model: app.config.translation.model.clone(),
        base_url: app.config.translation.base_url.clone(),
        api_key,
        timeout: Duration::from_millis(app.config.translation.timeout_ms),
    })
}

fn save_config(config: &AppConfig, path: &Path) -> Result<(), String> {
    config
        .save(path)
        .map_err(|error| format!("Failed to save config: {error}"))
}
