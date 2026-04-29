pub mod app;
pub mod composer;
pub mod event_loop;
mod model_picker;
pub mod panels;

use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use model_picker::{
    ModelPicker, ModelSelection, draw_model_picker, parse_model_options, reasoning_effort_label,
};
use oml_codex_appserver::client::{AccountSummary, AppServerClient};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use serde_json::{Value, json};
use tokio::{runtime::Runtime, time};
use unicode_width::UnicodeWidthStr;

const TICK_RATE: Duration = Duration::from_millis(50);
const EVENT_DRAIN_TIMEOUT: Duration = Duration::from_millis(1);
const USER_MESSAGE_BG: Color = Color::Rgb(31, 31, 31);

#[derive(Debug)]
struct TuiState {
    cwd: PathBuf,
    started_at: Instant,
    account: Option<AccountSummary>,
    thread_id: Option<String>,
    active_turn_id: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    input: String,
    input_cursor: usize,
    status: String,
    transcript: Vec<TranscriptEntry>,
    usage: Option<String>,
    rate_limits: RateLimitUsage,
    should_exit: bool,
    pending_approval: Option<PendingApproval>,
    model_picker: Option<ModelPicker>,
}

#[derive(Debug, Clone, Default)]
struct RateLimitUsage {
    five_hour_percent: Option<u16>,
    weekly_percent: Option<u16>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct TranscriptEntry {
    role: TranscriptRole,
    text: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum TranscriptRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone)]
struct PendingApproval {
    id: Value,
    method: String,
    summary: String,
}

impl TuiState {
    fn new() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            started_at: Instant::now(),
            account: None,
            thread_id: None,
            active_turn_id: None,
            model: None,
            reasoning_effort: None,
            input: String::new(),
            input_cursor: 0,
            status: "Connecting to Codex app-server...".to_owned(),
            transcript: Vec::new(),
            usage: None,
            rate_limits: RateLimitUsage::default(),
            should_exit: false,
            pending_approval: None,
            model_picker: None,
        }
    }

    fn push_system(&mut self, text: impl Into<String>) {
        self.transcript.push(TranscriptEntry {
            role: TranscriptRole::System,
            text: text.into(),
        });
    }

    fn push_user(&mut self, text: String) {
        self.transcript.push(TranscriptEntry {
            role: TranscriptRole::User,
            text,
        });
    }

    fn start_assistant_message(&mut self) {
        self.transcript.push(TranscriptEntry {
            role: TranscriptRole::Assistant,
            text: String::new(),
        });
    }

    fn append_assistant_delta(&mut self, delta: &str) {
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

    fn replace_last_assistant_message(&mut self, text: String) {
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
        });
    }

    fn account_line(&self) -> String {
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
}

pub fn run() -> io::Result<()> {
    enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_with_terminal(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_with_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let runtime = Runtime::new().map_err(io::Error::other)?;
    runtime.block_on(run_loop(terminal))
}

async fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let mut app = TuiState::new();
    terminal.draw(|frame| draw(frame, &app))?;

    let mut client = connect(&mut app)
        .await
        .map_err(|error| io::Error::other(error.to_string()))?;

    loop {
        drain_app_server_events(&mut client, &mut app).await;
        terminal.draw(|frame| draw(frame, &app))?;

        if event::poll(TICK_RATE)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Esc if app.model_picker.is_some() => {
                        if app
                            .model_picker
                            .as_mut()
                            .is_some_and(ModelPicker::cancel_or_back)
                        {
                            app.model_picker = None;
                            app.status = "Model selection canceled.".to_owned();
                        } else {
                            app.status = "Select a model and effort.".to_owned();
                        }
                    }
                    KeyCode::Up if app.model_picker.is_some() => {
                        if let Some(picker) = app.model_picker.as_mut() {
                            picker.select_previous();
                        }
                    }
                    KeyCode::Down if app.model_picker.is_some() => {
                        if let Some(picker) = app.model_picker.as_mut() {
                            picker.select_next();
                        }
                    }
                    KeyCode::Enter if app.model_picker.is_some() => {
                        if let Some(selection) =
                            app.model_picker.as_mut().and_then(ModelPicker::accept)
                        {
                            apply_model_selection(&mut app, selection);
                        }
                    }
                    KeyCode::Char(character) if app.model_picker.is_some() => {
                        if let Some(number) = character.to_digit(10).map(|digit| digit as usize)
                            && let Some(selection) = app
                                .model_picker
                                .as_mut()
                                .and_then(|picker| picker.select_number(number))
                        {
                            apply_model_selection(&mut app, selection);
                        }
                    }
                    _ if app.model_picker.is_some() => {}
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if app.active_turn_id.is_some() {
                            interrupt_turn(&mut client, &mut app).await;
                        } else {
                            break;
                        }
                    }
                    KeyCode::Esc if app.input.is_empty() => break,
                    KeyCode::Esc => {
                        app.input.clear();
                        app.input_cursor = 0;
                    }
                    KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        insert_input(&mut app, '\n');
                    }
                    KeyCode::Enter => {
                        submit_input(&mut client, &mut app).await;
                        if app.should_exit {
                            break;
                        }
                    }
                    KeyCode::Backspace => {
                        backspace_input(&mut app);
                    }
                    KeyCode::Delete => {
                        delete_input(&mut app);
                    }
                    KeyCode::Left => {
                        move_input_cursor_left(&mut app);
                    }
                    KeyCode::Right => {
                        move_input_cursor_right(&mut app);
                    }
                    KeyCode::Home => {
                        move_input_cursor_to_line_start(&mut app);
                    }
                    KeyCode::End => {
                        move_input_cursor_to_line_end(&mut app);
                    }
                    KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        insert_input(&mut app, character);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    let _ = client.shutdown().await;
    Ok(())
}

async fn connect(app: &mut TuiState) -> anyhow::Result<AppServerClient> {
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

async fn submit_input(client: &mut AppServerClient, app: &mut TuiState) {
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

fn apply_model_selection(app: &mut TuiState, selection: ModelSelection) {
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

fn insert_input(app: &mut TuiState, character: char) {
    let cursor = app.input_cursor.min(app.input.len());
    app.input.insert(cursor, character);
    app.input_cursor = cursor + character.len_utf8();
}

fn backspace_input(app: &mut TuiState) {
    let Some(previous) = previous_char_boundary(&app.input, app.input_cursor) else {
        return;
    };
    app.input.drain(previous..app.input_cursor);
    app.input_cursor = previous;
}

fn delete_input(app: &mut TuiState) {
    let Some(next) = next_char_boundary(&app.input, app.input_cursor) else {
        return;
    };
    app.input.drain(app.input_cursor..next);
}

fn move_input_cursor_left(app: &mut TuiState) {
    if let Some(previous) = previous_char_boundary(&app.input, app.input_cursor) {
        app.input_cursor = previous;
    }
}

fn move_input_cursor_right(app: &mut TuiState) {
    if let Some(next) = next_char_boundary(&app.input, app.input_cursor) {
        app.input_cursor = next;
    }
}

fn move_input_cursor_to_line_start(app: &mut TuiState) {
    app.input_cursor = app.input[..app.input_cursor]
        .rfind('\n')
        .map_or(0, |index| index + 1);
}

fn move_input_cursor_to_line_end(app: &mut TuiState) {
    app.input_cursor += app.input[app.input_cursor..]
        .find('\n')
        .unwrap_or_else(|| app.input.len() - app.input_cursor);
}

fn previous_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }

    text[..cursor].char_indices().last().map(|(index, _)| index)
}

fn next_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor >= text.len() {
        return None;
    }

    text[cursor..]
        .char_indices()
        .nth(1)
        .map(|(index, _)| cursor + index)
        .or(Some(text.len()))
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

async fn interrupt_turn(client: &mut AppServerClient, app: &mut TuiState) {
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

async fn drain_app_server_events(client: &mut AppServerClient, app: &mut TuiState) {
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
        app.status =
            "Approval required. Type /approve, /approve-session, /deny, or /cancel.".to_owned();
        app.push_system(format!(
            "Approval required:\n{}\n\nType /approve, /approve-session, /deny, or /cancel.",
            approval.summary
        ));
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
                app.status = "Ready.".to_owned();
            }
        }
        Some("thread/tokenUsage/updated") => {
            app.usage = token_usage_summary(message.get("params").unwrap_or(&Value::Null));
        }
        Some("account/rateLimits/updated") => {
            let params = message.get("params").unwrap_or(&Value::Null);
            if let Some(rate_limits) = rate_limit_usage(params) {
                app.rate_limits = rate_limits;
            }
            app.status = rate_limit_summary(params).unwrap_or_else(|| app.status.clone());
        }
        Some("error") => {
            app.status = format!("Codex error: {}", message["params"]);
        }
        _ => {}
    }
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

fn token_usage_summary(params: &Value) -> Option<String> {
    let total = params.get("tokenUsage")?.get("total")?;
    let input = total.get("inputTokens").and_then(Value::as_u64)?;
    let output = total.get("outputTokens").and_then(Value::as_u64)?;
    let cached = total
        .get("cachedInputTokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    Some(format!("input {input} · cached {cached} · output {output}"))
}

fn rate_limit_summary(params: &Value) -> Option<String> {
    let rate_limits = params.get("rateLimits").unwrap_or(params);
    let usage = rate_limit_usage(params)?;
    let plan = rate_limits
        .get("planType")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    Some(format!(
        "Ready. Codex {plan}; 5h {}; weekly {}.",
        limit_percent_text(usage.five_hour_percent),
        limit_percent_text(usage.weekly_percent)
    ))
}

fn rate_limit_usage(params: &Value) -> Option<RateLimitUsage> {
    let rate_limits = params.get("rateLimits").unwrap_or(params);
    let mut usage = RateLimitUsage::default();

    assign_rate_limit_window(
        &mut usage,
        rate_limits.get("primary"),
        RateLimitFallback::FiveHour,
    );
    assign_rate_limit_window(
        &mut usage,
        rate_limits.get("secondary"),
        RateLimitFallback::Weekly,
    );

    if usage.five_hour_percent.is_some() || usage.weekly_percent.is_some() {
        Some(usage)
    } else {
        None
    }
}

enum RateLimitFallback {
    FiveHour,
    Weekly,
}

fn assign_rate_limit_window(
    usage: &mut RateLimitUsage,
    window: Option<&Value>,
    fallback: RateLimitFallback,
) {
    let Some(window) = window else {
        return;
    };
    let Some(percent) = limit_window_percent(window) else {
        return;
    };

    match window.get("windowDurationMins").and_then(Value::as_u64) {
        Some(minutes) if minutes <= 5 * 60 => usage.five_hour_percent = Some(percent),
        Some(minutes) if minutes >= 7 * 24 * 60 => usage.weekly_percent = Some(percent),
        _ => match fallback {
            RateLimitFallback::FiveHour => usage.five_hour_percent = Some(percent),
            RateLimitFallback::Weekly => usage.weekly_percent = Some(percent),
        },
    }
}

fn limit_window_percent(window: &Value) -> Option<u16> {
    let percent = window.get("usedPercent").and_then(Value::as_u64)?;
    Some(percent.min(100) as u16)
}

fn limit_percent_text(percent: Option<u16>) -> String {
    percent
        .map(|percent| format!("{percent}%"))
        .unwrap_or_else(|| "pending".to_owned())
}

fn draw(frame: &mut Frame<'_>, app: &TuiState) {
    let area = frame.area();
    let composer_height = composer_height(app);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(composer_height),
        ])
        .split(area);

    draw_header(frame, app, rows[0]);
    draw_limit_bar(frame, app, rows[1]);
    draw_transcript(frame, app, rows[2]);
    draw_composer(frame, app, rows[3]);

    if let Some(picker) = app.model_picker.as_ref() {
        draw_model_picker(frame, picker, area);
    }
}

fn composer_height(app: &TuiState) -> u16 {
    let input_lines = app.input.split('\n').count().max(1) as u16;
    input_lines.min(6) + 3
}

fn draw_header(frame: &mut Frame<'_>, app: &TuiState, area: Rect) {
    let account = app
        .account
        .as_ref()
        .and_then(|account| account.plan_type.as_deref())
        .unwrap_or("unknown");
    let usage = app.usage.as_deref().unwrap_or("usage pending");
    let model = app.model.as_deref().unwrap_or("default");
    let reasoning = app.reasoning_effort.as_deref().unwrap_or("default");
    let uptime = app.started_at.elapsed().as_secs();

    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "Oh My Limit",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" for Codex  "),
            Span::styled(app.status.as_str(), Style::default().fg(Color::Gray)),
        ]),
        Line::from(format!(
            "cwd: {} · plan: {} · model: {} · reasoning: {} · {} · {}s",
            app.cwd.display(),
            account,
            model,
            reasoning,
            usage,
            uptime
        )),
    ]);
    frame.render_widget(header, area);
}

fn draw_limit_bar(frame: &mut Frame<'_>, app: &TuiState, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    draw_limit_gauge(
        frame,
        columns[0],
        "5h limit",
        app.rate_limits.five_hour_percent,
    );
    draw_limit_gauge(
        frame,
        columns[1],
        "weekly limit",
        app.rate_limits.weekly_percent,
    );
}

fn draw_limit_gauge(frame: &mut Frame<'_>, area: Rect, title: &'static str, percent: Option<u16>) {
    let percent = percent.map(|percent| percent.min(100));
    let remaining_percent = percent.map(|percent| 100 - percent).unwrap_or_default();
    let title = percent
        .map(|_| format!("{title} {remaining_percent}%"))
        .unwrap_or_else(|| format!("{title} pending"));
    frame.render_widget(Block::default().title(title).borders(Borders::ALL), area);

    let inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    if inner.is_empty() {
        return;
    }

    let fill_width = (u32::from(inner.width) * u32::from(remaining_percent) / 100) as u16;
    let bar_style = Style::default()
        .fg(percent.map(limit_color).unwrap_or(Color::DarkGray))
        .bg(Color::Black)
        .add_modifier(Modifier::BOLD);
    let empty_style = Style::default().fg(Color::Black).bg(Color::Black);
    let buffer = frame.buffer_mut();

    buffer.set_style(inner, empty_style);
    for y in inner.top()..inner.bottom() {
        for x in inner.left()..inner.left().saturating_add(fill_width) {
            buffer[(x, y)]
                .set_symbol(symbols::block::FULL)
                .set_style(bar_style);
        }
    }
}

fn limit_color(percent: u16) -> Color {
    match percent {
        90..=100 => Color::Red,
        70..=89 => Color::Yellow,
        _ => Color::Green,
    }
}

fn draw_transcript(frame: &mut Frame<'_>, app: &TuiState, area: Rect) {
    let height = area.height.saturating_sub(2) as usize;
    let mut lines = Vec::new();

    for entry in &app.transcript {
        let (label, style) = match entry.role {
            TranscriptRole::User => (
                "›",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            TranscriptRole::Assistant => (
                "codex",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            TranscriptRole::System => ("system", Style::default().fg(Color::DarkGray)),
        };
        let line_style = match entry.role {
            TranscriptRole::User => user_message_style(),
            TranscriptRole::Assistant | TranscriptRole::System => Style::default(),
        };

        lines.push(Line::from(Span::styled(label, style)).style(line_style));
        let text = if entry.text.is_empty() {
            "…".to_owned()
        } else {
            entry.text.clone()
        };
        lines.extend(
            text.lines()
                .map(|line| Line::from(format!("  {line}")).style(line_style)),
        );
        lines.push(Line::from(""));
    }

    if lines.is_empty() {
        lines.push(Line::from("Connected TUI will appear here."));
    }

    let visible_lines = if lines.len() > height {
        lines.split_off(lines.len() - height)
    } else {
        lines
    };

    let transcript = Paragraph::new(visible_lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::TOP));
    frame.render_widget(transcript, area);
}

fn draw_composer(frame: &mut Frame<'_>, app: &TuiState, area: Rect) {
    if area.is_empty() {
        return;
    }

    let text_area = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(3).max(1),
    };
    let prompt_style = if app.active_turn_id.is_some() || app.pending_approval.is_some() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    };
    let composer_style = user_message_style();
    let composer_area = Rect {
        x: area.x,
        y: text_area.y,
        width: area.width,
        height: text_area.height,
    };
    frame.render_widget(Block::default().style(composer_style), composer_area);
    frame.render_widget(
        Paragraph::new("›").style(prompt_style.bg(USER_MESSAGE_BG)),
        Rect {
            x: area.x,
            y: text_area.y,
            width: 1,
            height: 1,
        },
    );

    if app.input.is_empty() {
        let placeholder = if app.pending_approval.is_some() {
            "Approval required"
        } else if app.active_turn_id.is_some() {
            "Codex is responding..."
        } else {
            "Ask Codex to do anything"
        };
        frame.render_widget(
            Paragraph::new(placeholder)
                .style(Style::default().fg(Color::DarkGray).bg(USER_MESSAGE_BG)),
            text_area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(app.input.as_str())
                .style(Style::default().fg(Color::White).bg(USER_MESSAGE_BG))
                .wrap(Wrap { trim: false }),
            text_area,
        );
    }

    let footer_y = area.bottom().saturating_sub(1);
    let left_hint = if app.input.is_empty() {
        "? for shortcuts"
    } else if app.active_turn_id.is_some() {
        "enter waits for current turn"
    } else {
        "enter to send"
    };
    frame.render_widget(
        Paragraph::new(format!("  {left_hint}")).style(Style::default().fg(Color::DarkGray)),
        Rect {
            x: area.x,
            y: footer_y,
            width: area.width,
            height: 1,
        },
    );

    let right_hint = limit_footer_hint(app);
    let right_hint_width = right_hint.width() as u16;
    if !right_hint.is_empty() && right_hint_width < area.width {
        let x = area
            .right()
            .saturating_sub(right_hint_width)
            .saturating_sub(2);
        frame.render_widget(
            Paragraph::new(right_hint).style(Style::default().fg(Color::DarkGray)),
            Rect {
                x,
                y: footer_y,
                width: area.right().saturating_sub(x),
                height: 1,
            },
        );
    }

    let (cursor_x, cursor_y) = input_cursor_position(app, text_area);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn limit_footer_hint(app: &TuiState) -> String {
    match (
        app.rate_limits.five_hour_percent,
        app.rate_limits.weekly_percent,
    ) {
        (Some(five_hour), Some(weekly)) => {
            format!(
                "5h {}% left · weekly {}% left",
                100 - five_hour,
                100 - weekly
            )
        }
        (Some(five_hour), None) => format!("5h {}% left", 100 - five_hour),
        (None, Some(weekly)) => format!("weekly {}% left", 100 - weekly),
        (None, None) => String::new(),
    }
}

fn input_cursor_position(app: &TuiState, area: Rect) -> (u16, u16) {
    let before_cursor = &app.input[..app.input_cursor.min(app.input.len())];
    let row = before_cursor.bytes().filter(|byte| *byte == b'\n').count() as u16;
    let column = before_cursor
        .rsplit_once('\n')
        .map_or(before_cursor, |(_, line)| line)
        .width() as u16;

    (
        area.x
            .saturating_add(column.min(area.width.saturating_sub(1))),
        area.y
            .saturating_add(row.min(area.height.saturating_sub(1))),
    )
}

fn user_message_style() -> Style {
    Style::default().bg(USER_MESSAGE_BG)
}
