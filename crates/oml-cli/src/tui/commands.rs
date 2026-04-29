use std::{
    path::{Path, PathBuf},
    process::Command,
};

use oml_codex_appserver::client::AppServerClient;
use serde_json::{Value, json};

use super::{
    app::TuiState,
    limits::{rate_limit_summary, rate_limit_usage},
    model_picker::{ModelPicker, ModelSelection, parse_model_options, reasoning_effort_label},
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

    Ok(client)
}

pub(super) async fn submit_input(client: &mut AppServerClient, app: &mut TuiState) {
    let prompt = app.input.trim().to_owned();
    if prompt.is_empty() {
        return;
    }

    if prompt.starts_with('/') {
        app.input.clear();
        app.input_cursor = 0;
        handle_slash_command(client, app, &prompt).await;
        return;
    }

    if app.active_turn_id.is_some() {
        app.status = "Codex is still responding. Wait for this turn to finish.".to_owned();
        return;
    }

    let Some(thread_id) = app.thread_id.clone() else {
        app.status = "Codex thread is not ready yet.".to_owned();
        return;
    };

    app.input.clear();
    app.input_cursor = 0;
    app.push_user(prompt.clone());
    app.start_assistant_message();
    app.status = "Sending to Codex...".to_owned();

    let cwd = app.cwd.to_string_lossy().into_owned();
    match client
        .turn_start_with_model(
            &thread_id,
            &cwd,
            &prompt,
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
        }
    }
}

async fn handle_slash_command(client: &mut AppServerClient, app: &mut TuiState, command: &str) {
    match command {
        "/help" => {
            app.push_system(
                "Commands: /help, /status, /account, /usage, /limits, /diff, /model, /model <name>, /cd <path>, /list, /resume <thread-id>, /review, /compact, /approve, /approve-session, /deny, /cancel, /clear, /new, /interrupt, /exit",
            );
            app.status = "Help shown.".to_owned();
        }
        "/status" => {
            let thread = app.thread_id.as_deref().unwrap_or("none");
            let turn = app.active_turn_id.as_deref().unwrap_or("none");
            app.push_system(format!(
                "{}\nthread: {thread}\nactive turn: {turn}\nmodel: {}\nreasoning effort: {}",
                app.account_line(),
                app.model.as_deref().unwrap_or("default"),
                app.reasoning_effort.as_deref().unwrap_or("default")
            ));
            app.status = "Status shown.".to_owned();
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
