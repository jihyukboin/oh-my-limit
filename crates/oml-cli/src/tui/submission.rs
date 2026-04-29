use std::io;

use oml_codex_appserver::client::AppServerClient;
use oml_translation::translator::{
    TranslationDirection, TranslationProviderKind, TranslationRequest, TranslationResponse,
    build_translator,
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::str::FromStr;

use super::{
    app::{TokenUsage, TuiState},
    commands::handle_slash_command,
    translator_settings::translator_config,
    view::draw,
};

pub(super) async fn submit_current_input(
    client: &mut AppServerClient,
    app: &mut TuiState,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
    let prompt = app.bottom_pane.take_trimmed_input();
    if prompt.is_empty() {
        return Ok(());
    }

    if prompt.starts_with('/') {
        handle_slash_command(client, app, &prompt).await;
        return Ok(());
    }

    if app.active_turn_id.is_some() || app.pending_approval.is_some() {
        app.chat.queue_user_input(prompt);
        sync_bottom_pane_queue(app);
        app.status = "Queued follow-up input. It will run when Codex is ready.".to_owned();
        return Ok(());
    }

    submit_prompt(client, app, terminal, prompt).await
}

pub(super) async fn submit_next_queued_input(
    client: &mut AppServerClient,
    app: &mut TuiState,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
    if app.active_turn_id.is_some() || app.pending_approval.is_some() {
        return Ok(());
    }

    let Some(prompt) = app.chat.take_next_queued_input() else {
        return Ok(());
    };
    sync_bottom_pane_queue(app);
    submit_prompt(client, app, terminal, prompt).await
}

async fn submit_prompt(
    client: &mut AppServerClient,
    app: &mut TuiState,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    prompt: String,
) -> io::Result<()> {
    let Some(thread_id) = app.thread_id.clone() else {
        app.status = "Codex thread is not ready yet.".to_owned();
        return Ok(());
    };

    app.push_user(prompt.clone());
    app.start_assistant_message();
    app.status = "Translating prompt...".to_owned();
    terminal.draw(|frame| draw(frame, app))?;

    let translation = match translate_for_codex(app, &prompt).await {
        Ok(translation) => translation,
        Err(error) => {
            app.status = format!("Translation failed: {error}");
            app.replace_last_assistant_message(format!("Translation failed: {error}"));
            return Ok(());
        }
    };
    app.translator_usage = translation.usage.map(|usage| TokenUsage {
        input: usage.input_tokens,
        cached: usage.cached_input_tokens,
        output: usage.output_tokens,
    });
    let codex_prompt = translation.text;

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

async fn translate_for_codex(
    app: &mut TuiState,
    prompt: &str,
) -> anyhow::Result<TranslationResponse> {
    let provider = TranslationProviderKind::from_str(&app.config.translation.provider)?;
    if !app.config.translation.enabled || provider == TranslationProviderKind::Noop {
        return Ok(TranslationResponse {
            text: prompt.to_owned(),
            provider,
            usage: None,
        });
    }

    let translator_config = match translator_config(app) {
        Ok(config) => config,
        Err(error) if app.config.translation.fail_closed => return Err(error),
        Err(error) => {
            app.push_system(format!("Translation skipped: {error}"));
            return Ok(TranslationResponse {
                text: prompt.to_owned(),
                provider,
                usage: None,
            });
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
        Ok(response) => Ok(response),
        Err(error) if app.config.translation.fail_closed => Err(error),
        Err(error) => {
            app.push_system(format!("Translation skipped: {error}"));
            Ok(TranslationResponse {
                text: prompt.to_owned(),
                provider,
                usage: None,
            })
        }
    }
}

fn sync_bottom_pane_queue(app: &mut TuiState) {
    app.bottom_pane
        .set_queued_messages(app.chat.queued_input_lines());
}
