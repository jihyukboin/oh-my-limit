use oml_codex_appserver::client::AppServerClient;
use serde_json::Value;
use tokio::time;

use super::{
    EVENT_DRAIN_TIMEOUT,
    app::{PendingApproval, TokenUsage, TuiState},
    app_event::AppEvent,
    limits::{rate_limit_summary, rate_limit_usage},
};

pub(super) async fn drain_app_server_events(client: &mut AppServerClient, app: &mut TuiState) {
    loop {
        match time::timeout(EVENT_DRAIN_TIMEOUT, client.next_message()).await {
            Ok(Ok(message)) => handle_app_server_message(app, &message),
            Ok(Err(error)) => {
                app.status = format!("Codex app-server error: {error}");
                break;
            }
            Err(_) => break,
        }
    }
}

fn handle_app_server_message(app: &mut TuiState, message: &Value) {
    if let Some(approval) = pending_approval_from_message(message) {
        app.emit(AppEvent::SetStatus(
            "Approval required. Type /approve, /approve-session, /deny, or /cancel.".to_owned(),
        ));
        app.emit(AppEvent::PushApproval(format!(
            "Approval required:\n{}\n\nType /approve, /approve-session, /deny, or /cancel.",
            approval.summary
        )));
        app.pending_approval = Some(approval);
        return;
    }

    match message.get("method").and_then(Value::as_str) {
        Some("item/agentMessage/delta") => {
            let params = message.get("params").unwrap_or(&Value::Null);
            if belongs_to_active_turn(app, params)
                && let Some(delta) = params.get("delta").and_then(Value::as_str)
            {
                app.append_assistant_delta(delta);
            }
        }
        Some("item/completed") => {
            let params = message.get("params").unwrap_or(&Value::Null);
            if belongs_to_active_turn(app, params)
                && let Some(text) = completed_agent_answer(params)
            {
                app.replace_last_assistant_message(text);
            } else if belongs_to_active_turn(app, params)
                && let Some((label, text)) = completed_tool_summary(params)
            {
                app.emit(AppEvent::PushToolCall { label, text });
            }
        }
        Some("item/started") => {
            let params = message.get("params").unwrap_or(&Value::Null);
            if belongs_to_active_turn(app, params)
                && let Some((label, text)) = started_tool_summary(params)
            {
                app.emit(AppEvent::PushToolCall { label, text });
            }
        }
        Some("item/plan/delta") => {
            let params = message.get("params").unwrap_or(&Value::Null);
            if belongs_to_active_turn(app, params)
                && let Some(delta) = params.get("delta").and_then(Value::as_str)
            {
                app.emit(AppEvent::PushPlan(delta.to_owned()));
            }
        }
        Some("turn/plan/updated") => {
            let params = message.get("params").unwrap_or(&Value::Null);
            if belongs_to_active_turn(app, params)
                && let Some(plan) = format_plan(params)
            {
                app.emit(AppEvent::PushPlan(plan));
            }
        }
        Some("turn/completed") => {
            let params = message.get("params").unwrap_or(&Value::Null);
            let turn_id = params
                .get("turn")
                .and_then(|turn| turn.get("id"))
                .and_then(Value::as_str);

            if turn_id == app.active_turn_id.as_deref() {
                app.active_turn_id = None;
                app.emit(AppEvent::SetStatus("Ready.".to_owned()));
                app.emit(AppEvent::PushFinalSeparator(None));
            }
        }
        Some("thread/tokenUsage/updated") => {
            app.codex_usage =
                token_usage_from_params(message.get("params").unwrap_or(&Value::Null));
        }
        Some("account/rateLimits/updated") => {
            let params = message.get("params").unwrap_or(&Value::Null);
            if let Some(rate_limits) = rate_limit_usage(params) {
                app.rate_limits = rate_limits;
            }
            app.status = rate_limit_summary(params).unwrap_or_else(|| app.status.clone());
        }
        Some("error") => {
            let error = format!("Codex error: {}", message["params"]);
            app.emit(AppEvent::SetStatus(error.clone()));
            app.emit(AppEvent::PushError(error));
        }
        _ => {}
    }
}

fn started_tool_summary(params: &Value) -> Option<(String, String)> {
    let item = params.get("item")?;
    match item.get("type").and_then(Value::as_str)? {
        "commandExecution" => Some((
            "exec".to_owned(),
            item.get("command")
                .and_then(Value::as_str)
                .unwrap_or("command started")
                .to_owned(),
        )),
        "fileChange" => Some(("patch".to_owned(), "file change started".to_owned())),
        "tool" | "mcpToolCall" | "dynamicToolCall" => Some((
            "tool".to_owned(),
            item.get("tool")
                .or_else(|| item.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("tool call started")
                .to_owned(),
        )),
        _ => None,
    }
}

fn completed_tool_summary(params: &Value) -> Option<(String, String)> {
    let item = params.get("item")?;
    match item.get("type").and_then(Value::as_str)? {
        "commandExecution" => Some((
            "exec".to_owned(),
            item.get("command")
                .and_then(Value::as_str)
                .unwrap_or("command completed")
                .to_owned(),
        )),
        "fileChange" => Some(("patch".to_owned(), "file change completed".to_owned())),
        "tool" | "mcpToolCall" | "dynamicToolCall" => Some((
            "tool".to_owned(),
            item.get("tool")
                .or_else(|| item.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("tool call completed")
                .to_owned(),
        )),
        _ => None,
    }
}

fn format_plan(params: &Value) -> Option<String> {
    let plan = params.get("plan")?.as_array()?;
    let lines = plan
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let text = step
                .get("text")
                .or_else(|| step.get("step"))
                .and_then(Value::as_str)
                .unwrap_or("(step)");
            let status = step
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("pending");
            format!("{}. [{status}] {text}", index + 1)
        })
        .collect::<Vec<_>>();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn pending_approval_from_message(message: &Value) -> Option<PendingApproval> {
    let id = message.get("id")?.clone();
    let method = message.get("method")?.as_str()?.to_owned();
    let params = message.get("params").unwrap_or(&Value::Null);

    match method.as_str() {
        "item/commandExecution/requestApproval" => Some(PendingApproval {
            id,
            method,
            summary: command_approval_summary(params),
        }),
        "item/fileChange/requestApproval" => Some(PendingApproval {
            id,
            method,
            summary: file_approval_summary(params),
        }),
        "execCommandApproval" => Some(PendingApproval {
            id,
            method,
            summary: legacy_exec_approval_summary(params),
        }),
        "applyPatchApproval" => Some(PendingApproval {
            id,
            method,
            summary: format!("Patch approval requested: {params}"),
        }),
        _ => None,
    }
}

fn command_approval_summary(params: &Value) -> String {
    let command = params
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("(command unavailable)");
    let cwd = params
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or("(cwd unknown)");
    let reason = params.get("reason").and_then(Value::as_str).unwrap_or("");

    if reason.is_empty() {
        format!("command: {command}\ncwd: {cwd}")
    } else {
        format!("command: {command}\ncwd: {cwd}\nreason: {reason}")
    }
}

fn file_approval_summary(params: &Value) -> String {
    let item_id = params
        .get("itemId")
        .and_then(Value::as_str)
        .unwrap_or("(item unknown)");
    let reason = params.get("reason").and_then(Value::as_str).unwrap_or("");
    let grant_root = params
        .get("grantRoot")
        .and_then(Value::as_str)
        .unwrap_or("");

    format!("file change item: {item_id}\nreason: {reason}\ngrant root: {grant_root}")
}

fn legacy_exec_approval_summary(params: &Value) -> String {
    let cwd = params
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or("(cwd unknown)");
    let command = params
        .get("command")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|| "(command unavailable)".to_owned());

    format!("command: {command}\ncwd: {cwd}")
}

fn belongs_to_active_turn(app: &TuiState, params: &Value) -> bool {
    params.get("turnId").and_then(Value::as_str) == app.active_turn_id.as_deref()
}

fn completed_agent_answer(params: &Value) -> Option<String> {
    let item = params.get("item")?;

    if item.get("type").and_then(Value::as_str) != Some("agentMessage") {
        return None;
    }

    let phase = item.get("phase").and_then(Value::as_str);
    if phase.is_some_and(|phase| phase != "final_answer") {
        return None;
    }

    item.get("text").and_then(Value::as_str).map(str::to_owned)
}

fn token_usage_from_params(params: &Value) -> Option<TokenUsage> {
    let total = params.get("tokenUsage")?.get("total")?;
    let input = total.get("inputTokens").and_then(Value::as_u64)?;
    let output = total.get("outputTokens").and_then(Value::as_u64)?;
    let cached = total
        .get("cachedInputTokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    Some(TokenUsage {
        input,
        cached,
        output,
    })
}
