use std::{
    env, io,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    time::Duration,
};

use oml_codex_appserver::client::AppServerClient;
use oml_config::{config::AppConfig, env_file::save_env_value};
use oml_translation::translator::{
    TranslationDirection, TranslationProviderKind, TranslationRequest, TranslatorConfig,
    build_translator,
};
use ratatui::{Terminal, backend::CrosstermBackend};
use serde_json::{Value, json};

use super::{
    app::TuiState,
    limits::{rate_limit_summary, rate_limit_usage},
    model_picker::{ModelPicker, ModelSelection, parse_model_options, reasoning_effort_label},
    translator_picker::{TranslatorPicker, TranslatorPickerAction, TranslatorProviderSelection},
    view::draw,
};

pub(super) async fn connect(app: &mut TuiState) -> anyhow::Result<AppServerClient> {
    let mut client = AppServerClient::spawn().await?;
    client.initialize().await?;

    let account = client.account_read().await?;
    app.account = Some(account);

    if let Ok(result) = client.account_rate_limits_read().await
        && let Some(rate_limits) = rate_limit_usage(&result)
    {
        app.rate_limits = rate_limits;
    }

    let cwd = app.cwd.to_string_lossy().into_owned();
    let thread_id = client.thread_start(&cwd).await?;
    app.thread_id = Some(thread_id.clone());
    app.status = "Connected to Codex. Type a message and press Enter.".to_owned();
    app.push_system(format!("Connected to Codex thread {thread_id}."));
    app.push_system(app.translator_line());

    Ok(client)
}

pub(super) async fn submit_input(
    client: &mut AppServerClient,
    app: &mut TuiState,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
    let prompt = app.input.trim().to_owned();
    if prompt.is_empty() {
        return Ok(());
    }

    if prompt.starts_with('/') {
        app.input.clear();
        app.input_cursor = 0;
        handle_slash_command(client, app, &prompt).await;
        return Ok(());
    }

    if app.active_turn_id.is_some() {
        app.status = "Codex is still responding. Wait for this turn to finish.".to_owned();
        return Ok(());
    }

    let Some(thread_id) = app.thread_id.clone() else {
        app.status = "Codex thread is not ready yet.".to_owned();
        return Ok(());
    };

    app.input.clear();
    app.input_cursor = 0;
    app.push_user(prompt.clone());
    app.start_assistant_message();
    app.status = "Translating prompt...".to_owned();
    terminal.draw(|frame| draw(frame, app))?;

    let codex_prompt = match translate_for_codex(app, &prompt).await {
        Ok(codex_prompt) => codex_prompt,
        Err(error) => {
            app.status = format!("Translation failed: {error}");
            app.replace_last_assistant_message(format!("Translation failed: {error}"));
            return Ok(());
        }
    };

    if codex_prompt != prompt {
        app.set_last_user_translation(codex_prompt.clone());
        app.status = "Sending translated prompt to Codex...".to_owned();
        terminal.draw(|frame| draw(frame, app))?;
    } else {
        app.status = "Sending to Codex...".to_owned();
    }

    let cwd = app.cwd.to_string_lossy().into_owned();
    match client
        .turn_start_with_model(
            &thread_id,
            &cwd,
            &codex_prompt,
            app.model.as_deref(),
            app.reasoning_effort.as_deref(),
        )
        .await
    {
        Ok(turn_id) => {
            app.active_turn_id = Some(turn_id);
            app.status = "Codex is thinking...".to_owned();
        }
        Err(error) => {
            app.status = format!("Failed to start turn: {error}");
            app.replace_last_assistant_message(format!("Failed to start turn: {error}"));
        }
    }

    Ok(())
}

async fn handle_slash_command(client: &mut AppServerClient, app: &mut TuiState, command: &str) {
    match command {
        "/help" => {
            app.push_system(
                "Commands: /help, /status, /account, /usage, /limits, /translator (opens picker), /translator provider <noop|ollama|local-openai-compatible|openai>, /translator model <name>, /translator base-url <url|default>, /translator api-key-env <ENV>, /translator remote <on|off>, /translator test, /diff, /model, /model <name>, /cd <path>, /list, /resume <thread-id>, /review, /compact, /approve, /approve-session, /deny, /cancel, /clear, /new, /interrupt, /exit",
            );
            app.status = "Help shown.".to_owned();
        }
        "/status" => {
            let thread = app.thread_id.as_deref().unwrap_or("none");
            let turn = app.active_turn_id.as_deref().unwrap_or("none");
            app.push_system(format!(
                "{}\n{}\nthread: {thread}\nactive turn: {turn}\nmodel: {}\nreasoning effort: {}",
                app.account_line(),
                app.translator_line(),
                app.model.as_deref().unwrap_or("default"),
                app.reasoning_effort.as_deref().unwrap_or("default")
            ));
            app.status = "Status shown.".to_owned();
        }
        "/translator" => {
            open_translator_picker(app);
        }
        command if command.starts_with("/translator ") => {
            let rest = command.trim_start_matches("/translator").trim();
            if rest.is_empty() {
                open_translator_picker(app);
            } else {
                handle_translator_command(app, command).await;
            }
        }
        "/account" => match client.account_read().await {
            Ok(account) => {
                app.account = Some(account);
                app.push_system(app.account_line());
                app.status = "Account refreshed.".to_owned();
            }
            Err(error) => {
                app.status = format!("Failed to read account: {error}");
            }
        },
        "/usage" | "/limits" => match client.account_rate_limits_read().await {
            Ok(result) => {
                if let Some(rate_limits) = rate_limit_usage(&result) {
                    app.rate_limits = rate_limits;
                }
                let summary = rate_limit_summary(&result)
                    .or_else(|| {
                        rate_limit_summary(result.get("rateLimits").unwrap_or(&Value::Null))
                    })
                    .unwrap_or_else(|| format!("Usage response: {result}"));
                app.push_system(summary.clone());
                app.status = summary;
            }
            Err(error) => {
                app.status = format!("Failed to read usage: {error}");
            }
        },
        "/diff" => {
            let status = run_git(&app.cwd, &["status", "--short"]);
            let diff = run_git(&app.cwd, &["diff", "--stat"]);
            app.push_system(format!(
                "git status --short\n{status}\n\ngit diff --stat\n{diff}"
            ));
            app.status = "Diff shown.".to_owned();
        }
        "/review" => {
            start_review(client, app).await;
        }
        "/compact" => {
            start_compaction(client, app).await;
        }
        "/list" => {
            list_threads(client, app).await;
        }
        "/approve" => {
            respond_to_approval(client, app, ApprovalChoice::Approve).await;
        }
        "/approve-session" => {
            respond_to_approval(client, app, ApprovalChoice::ApproveForSession).await;
        }
        "/deny" => {
            respond_to_approval(client, app, ApprovalChoice::Deny).await;
        }
        "/cancel" => {
            respond_to_approval(client, app, ApprovalChoice::Cancel).await;
        }
        "/clear" => {
            app.transcript.clear();
            app.status = "Cleared.".to_owned();
        }
        "/new" => {
            if app.active_turn_id.is_some() {
                app.status = "Cannot start a new thread while Codex is responding.".to_owned();
                return;
            }

            let cwd = app.cwd.to_string_lossy().into_owned();
            match client
                .thread_start_with_model(&cwd, app.model.as_deref())
                .await
            {
                Ok(thread_id) => {
                    app.thread_id = Some(thread_id.clone());
                    app.transcript.clear();
                    app.push_system(format!("Started new Codex thread {thread_id}."));
                    app.status = "New thread ready.".to_owned();
                }
                Err(error) => {
                    app.status = format!("Failed to start new thread: {error}");
                }
            }
        }
        "/interrupt" => {
            interrupt_turn(client, app).await;
        }
        "/exit" | "/quit" => {
            app.should_exit = true;
        }
        "/model" => {
            open_model_picker(client, app).await;
        }
        command if command.starts_with("/model ") => {
            let model = command.trim_start_matches("/model ").trim();
            if model.is_empty() {
                open_model_picker(client, app).await;
            } else {
                app.model = Some(model.to_owned());
                app.reasoning_effort = None;
                app.push_system(format!("Model set to {model}. Applies to the next turn."));
                app.status = format!("Model set to {model}.");
            }
        }
        command if command.starts_with("/cd ") => {
            let path = command.trim_start_matches("/cd ").trim();
            match resolve_cwd(&app.cwd, path) {
                Ok(cwd) => {
                    app.cwd = cwd;
                    app.push_system(format!("cwd set to {}.", app.cwd.display()));
                    app.status = "cwd changed. Applies to the next turn.".to_owned();
                }
                Err(error) => {
                    app.status = error;
                }
            }
        }
        command if command.starts_with("/resume ") => {
            let thread_id = command.trim_start_matches("/resume ").trim();
            resume_thread(client, app, thread_id).await;
        }
        _ => {
            app.push_system(format!(
                "Unknown command: {command}\nType /help for commands."
            ));
            app.status = "Unknown command.".to_owned();
        }
    }
}

async fn translate_for_codex(app: &mut TuiState, prompt: &str) -> anyhow::Result<String> {
    let provider = TranslationProviderKind::from_str(&app.config.translation.provider)?;
    if provider == TranslationProviderKind::Noop {
        return Ok(prompt.to_owned());
    }

    let translator_config = match translator_config(app) {
        Ok(config) => config,
        Err(error) if app.config.translation.fail_closed => return Err(error),
        Err(error) => {
            app.push_system(format!("Translation skipped: {error}"));
            return Ok(prompt.to_owned());
        }
    };

    let translator = build_translator(translator_config);
    match translator
        .translate(TranslationRequest {
            direction: TranslationDirection::KoreanToEnglish,
            text: prompt.to_owned(),
        })
        .await
    {
        Ok(response) => Ok(response.text),
        Err(error) if app.config.translation.fail_closed => Err(error),
        Err(error) => {
            app.push_system(format!("Translation skipped: {error}"));
            Ok(prompt.to_owned())
        }
    }
}

async fn handle_translator_command(app: &mut TuiState, command: &str) {
    let rest = command.trim_start_matches("/translator ").trim();
    let mut parts = rest.splitn(2, ' ');
    let key = parts.next().unwrap_or_default();
    let value = parts.next().unwrap_or_default().trim();

    let result = match key {
        "provider" => set_translator_provider(app, value),
        "model" => set_optional_translation_value(app, "model", value),
        "base-url" => set_optional_translation_value(app, "base-url", value),
        "api-key-env" => set_optional_translation_value(app, "api-key-env", value),
        "remote" => set_remote_translation(app, value),
        "test" if value.is_empty() => test_translator(app).await,
        _ => Err("Usage: /translator provider <noop|ollama|local-openai-compatible|openai>, /translator model <name>, /translator base-url <url|default>, /translator api-key-env <ENV>, /translator remote <on|off>, /translator test".to_owned()),
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

fn open_translator_picker(app: &mut TuiState) {
    if app.model_picker.is_some() {
        app.model_picker = None;
    }

    app.translator_picker = Some(TranslatorPicker::new());
    app.status = "Select a translator and settings.".to_owned();
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
        TranslatorPickerAction::SelectedLocal(selection) => {
            app.config.translation.provider = match selection {
                TranslatorProviderSelection::Noop => TranslationProviderKind::Noop.as_str(),
                TranslatorProviderSelection::Ollama => TranslationProviderKind::Ollama.as_str(),
                TranslatorProviderSelection::LocalOpenAiCompatible => {
                    TranslationProviderKind::LocalOpenAiCompatible.as_str()
                }
            }
            .to_owned();
            app.config.privacy.remote_translation_allowed = false;
            app.config.translation.api_key_env = None;
            app.openai_api_key = None;
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

    let provider = app.config.translation.provider.clone();
    app.push_system(format!("Translator provider set to {provider}."));
    app.status = format!("Translator provider set to {provider}.");

    Ok(())
}

fn set_translator_provider(app: &mut TuiState, value: &str) -> Result<String, String> {
    let provider = TranslationProviderKind::from_str(value).map_err(|error| error.to_string())?;
    app.config.translation.provider = provider.as_str().to_owned();

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

fn translator_config(app: &TuiState) -> anyhow::Result<TranslatorConfig> {
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

async fn open_model_picker(client: &mut AppServerClient, app: &mut TuiState) {
    match client.model_list().await {
        Ok(result) => {
            let options = parse_model_options(&result);
            let picker = ModelPicker::new(
                options,
                app.model.as_deref(),
                app.reasoning_effort.as_deref(),
            );
            if picker.is_empty() {
                app.status = "No selectable models returned by Codex.".to_owned();
                return;
            }

            app.model_picker = Some(picker);
            app.status = "Select a model and effort.".to_owned();
        }
        Err(error) => {
            app.status = format!("Failed to list models: {error}");
        }
    }
}

pub(super) fn apply_model_selection(app: &mut TuiState, selection: ModelSelection) {
    let ModelSelection { model, effort } = selection;
    app.model = Some(model.clone());
    app.reasoning_effort = effort.clone();
    app.model_picker = None;

    let effort_text = effort
        .as_deref()
        .map(|effort| {
            format!(
                " with {} reasoning",
                reasoning_effort_label(effort).to_lowercase()
            )
        })
        .unwrap_or_default();
    app.push_system(format!("Model changed to {model}{effort_text}."));
    app.status = format!("Model changed to {model}{effort_text}.");
}

fn resolve_cwd(current: &Path, path: &str) -> Result<PathBuf, String> {
    if path.is_empty() {
        return Err("Usage: /cd <path>".to_owned());
    }

    let next = if path == "~" {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "HOME is not set".to_owned())?
    } else if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "HOME is not set".to_owned())?;
        home.join(rest)
    } else {
        let candidate = PathBuf::from(path);
        if candidate.is_absolute() {
            candidate
        } else {
            current.join(candidate)
        }
    };

    if !next.is_dir() {
        return Err(format!("Not a directory: {}", next.display()));
    }

    next.canonicalize()
        .map_err(|error| format!("Failed to resolve cwd: {error}"))
}

fn run_git(cwd: &PathBuf, args: &[&str]) -> String {
    match Command::new("git").args(args).current_dir(cwd).output() {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if text.is_empty() {
                "(no output)".to_owned()
            } else {
                text
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if stderr.is_empty() {
                format!("git exited with {}", output.status)
            } else {
                stderr
            }
        }
        Err(error) => format!("failed to run git: {error}"),
    }
}

async fn start_review(client: &mut AppServerClient, app: &mut TuiState) {
    if app.active_turn_id.is_some() {
        app.status = "Cannot start review while Codex is responding.".to_owned();
        return;
    }

    let Some(thread_id) = app.thread_id.clone() else {
        app.status = "No active Codex thread.".to_owned();
        return;
    };

    app.push_user("/review".to_owned());
    app.start_assistant_message();

    match client.review_start(&thread_id).await {
        Ok(turn_id) => {
            app.active_turn_id = Some(turn_id);
            app.status = "Review started.".to_owned();
        }
        Err(error) => {
            app.status = format!("Failed to start review: {error}");
        }
    }
}

async fn start_compaction(client: &mut AppServerClient, app: &mut TuiState) {
    if app.active_turn_id.is_some() {
        app.status = "Cannot compact while Codex is responding.".to_owned();
        return;
    }

    let Some(thread_id) = app.thread_id.clone() else {
        app.status = "No active Codex thread.".to_owned();
        return;
    };

    match client.compact_start(&thread_id).await {
        Ok(()) => {
            app.push_system("Compaction started.");
            app.status = "Compaction started.".to_owned();
        }
        Err(error) => {
            app.status = format!("Failed to start compaction: {error}");
        }
    }
}

async fn list_threads(client: &mut AppServerClient, app: &mut TuiState) {
    let cwd = app.cwd.to_string_lossy();
    match client.thread_list(Some(cwd.as_ref()), 10).await {
        Ok(result) => {
            let lines = result
                .get("data")
                .and_then(Value::as_array)
                .map(|threads| {
                    threads
                        .iter()
                        .map(thread_list_line)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "(no threads)".to_owned());
            app.push_system(format!("Recent Codex threads:\n{lines}"));
            app.status = "Thread list shown.".to_owned();
        }
        Err(error) => {
            app.status = format!("Failed to list threads: {error}");
        }
    }
}

fn thread_list_line(thread: &Value) -> String {
    let id = thread
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("(no id)");
    let name = thread
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| thread.get("preview").and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .unwrap_or("(untitled)");
    let cwd = thread.get("cwd").and_then(Value::as_str).unwrap_or("");

    format!("{id}  {name}  {cwd}")
}

async fn resume_thread(client: &mut AppServerClient, app: &mut TuiState, thread_id: &str) {
    if thread_id.is_empty() {
        app.status = "Usage: /resume <thread-id>".to_owned();
        return;
    }

    if app.active_turn_id.is_some() {
        app.status = "Cannot resume while Codex is responding.".to_owned();
        return;
    }

    let cwd = app.cwd.to_string_lossy().into_owned();
    match client.thread_resume(thread_id, &cwd).await {
        Ok(resumed_thread_id) => {
            app.thread_id = Some(resumed_thread_id.clone());
            app.transcript.clear();
            app.push_system(format!("Resumed Codex thread {resumed_thread_id}."));
            app.status = "Thread resumed.".to_owned();
        }
        Err(error) => {
            app.status = format!("Failed to resume thread: {error}");
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ApprovalChoice {
    Approve,
    ApproveForSession,
    Deny,
    Cancel,
}

async fn respond_to_approval(
    client: &mut AppServerClient,
    app: &mut TuiState,
    choice: ApprovalChoice,
) {
    let Some(approval) = app.pending_approval.take() else {
        app.status = "No pending approval.".to_owned();
        return;
    };

    let Some(result) = approval_result(&approval.method, choice) else {
        app.status = format!("Unsupported approval request: {}", approval.method);
        app.pending_approval = Some(approval);
        return;
    };

    match client.respond_server_request(approval.id, result).await {
        Ok(()) => {
            app.status = "Approval response sent.".to_owned();
            app.push_system(format!("Approval response sent for {}.", approval.method));
        }
        Err(error) => {
            app.status = format!("Failed to send approval response: {error}");
        }
    }
}

fn approval_result(method: &str, choice: ApprovalChoice) -> Option<Value> {
    let decision = match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => match choice
        {
            ApprovalChoice::Approve => "accept",
            ApprovalChoice::ApproveForSession => "acceptForSession",
            ApprovalChoice::Deny => "decline",
            ApprovalChoice::Cancel => "cancel",
        },
        "execCommandApproval" => match choice {
            ApprovalChoice::Approve => "approved",
            ApprovalChoice::ApproveForSession => "approved_for_session",
            ApprovalChoice::Deny => "denied",
            ApprovalChoice::Cancel => "abort",
        },
        "applyPatchApproval" => match choice {
            ApprovalChoice::Approve | ApprovalChoice::ApproveForSession => "approved",
            ApprovalChoice::Deny => "denied",
            ApprovalChoice::Cancel => "abort",
        },
        _ => return None,
    };

    Some(json!({ "decision": decision }))
}

pub(super) async fn interrupt_turn(client: &mut AppServerClient, app: &mut TuiState) {
    let Some(thread_id) = app.thread_id.clone() else {
        app.status = "No active Codex thread.".to_owned();
        return;
    };
    let Some(turn_id) = app.active_turn_id.clone() else {
        app.status = "No active Codex turn to interrupt.".to_owned();
        return;
    };

    match client.turn_interrupt(&thread_id, &turn_id).await {
        Ok(()) => {
            app.active_turn_id = None;
            app.status = "Interrupted.".to_owned();
            app.push_system("Interrupted active Codex turn.");
        }
        Err(error) => {
            app.status = format!("Failed to interrupt turn: {error}");
        }
    }
}
