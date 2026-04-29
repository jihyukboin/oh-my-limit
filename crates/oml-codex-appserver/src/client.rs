use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::transport::StdioJsonlTransport;

#[derive(Debug)]
pub struct AppServerClient {
    transport: StdioJsonlTransport,
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub prompt: String,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RunResult {
    pub answer: String,
    pub thread_id: String,
    pub turn_id: String,
    pub account: AccountSummary,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AccountSummary {
    pub account_type: Option<String>,
    pub plan_type: Option<String>,
    pub requires_openai_auth: bool,
}

impl AppServerClient {
    pub async fn spawn() -> Result<Self> {
        Ok(Self {
            transport: StdioJsonlTransport::spawn().await?,
        })
    }

    pub async fn initialize(&mut self) -> Result<Value> {
        let result = self
            .transport
            .request(
                "initialize",
                json!({
                    "clientInfo": {
                        "name": "oh_my_limit",
                        "title": "Oh My Limit for Codex",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "capabilities": {
                        "experimentalApi": false,
                    },
                }),
            )
            .await?;

        self.transport
            .notification("initialized", json!({}))
            .await?;

        Ok(result)
    }

    pub async fn account_read(&mut self) -> Result<AccountSummary> {
        let result = self
            .transport
            .request("account/read", json!({ "refreshToken": false }))
            .await?;

        let account = result.get("account");
        Ok(AccountSummary {
            account_type: account
                .and_then(|account| account.get("type"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            plan_type: account
                .and_then(|account| account.get("planType"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            requires_openai_auth: result
                .get("requiresOpenaiAuth")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    pub async fn thread_start(&mut self, cwd: &str) -> Result<String> {
        self.thread_start_with_model(cwd, None).await
    }

    pub async fn thread_start_with_model(
        &mut self,
        cwd: &str,
        model: Option<&str>,
    ) -> Result<String> {
        let result = self
            .transport
            .request(
                "thread/start",
                json!({
                    "cwd": cwd,
                    "ephemeral": true,
                    "serviceName": "oh-my-limit",
                    "model": model,
                }),
            )
            .await?;

        json_string_at(&result, &["thread", "id"])
            .context("thread/start response missing thread.id")
    }

    pub async fn turn_start(&mut self, thread_id: &str, cwd: &str, prompt: &str) -> Result<String> {
        self.turn_start_with_model(thread_id, cwd, prompt, None, None)
            .await
    }

    pub async fn turn_start_with_model(
        &mut self,
        thread_id: &str,
        cwd: &str,
        prompt: &str,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<String> {
        let result = self
            .transport
            .request(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "cwd": cwd,
                    "input": [
                        {
                            "type": "text",
                            "text": prompt,
                        }
                    ],
                    "model": model,
                    "effort": effort,
                }),
            )
            .await?;

        json_string_at(&result, &["turn", "id"]).context("turn/start response missing turn.id")
    }

    pub async fn turn_interrupt(&mut self, thread_id: &str, turn_id: &str) -> Result<()> {
        self.transport
            .request(
                "turn/interrupt",
                json!({
                    "threadId": thread_id,
                    "turnId": turn_id,
                }),
            )
            .await?;

        Ok(())
    }

    pub async fn account_rate_limits_read(&mut self) -> Result<Value> {
        self.transport
            .request("account/rateLimits/read", Value::Null)
            .await
    }

    pub async fn model_list(&mut self) -> Result<Value> {
        self.transport
            .request(
                "model/list",
                json!({
                    "includeHidden": false,
                }),
            )
            .await
    }

    pub async fn compact_start(&mut self, thread_id: &str) -> Result<()> {
        self.transport
            .request("thread/compact/start", json!({ "threadId": thread_id }))
            .await?;
        Ok(())
    }

    pub async fn review_start(&mut self, thread_id: &str) -> Result<String> {
        let result = self
            .transport
            .request(
                "review/start",
                json!({
                    "threadId": thread_id,
                    "delivery": "inline",
                    "target": {
                        "type": "uncommittedChanges",
                    },
                }),
            )
            .await?;

        json_string_at(&result, &["turn", "id"]).context("review/start response missing turn.id")
    }

    pub async fn thread_list(&mut self, cwd: Option<&str>, limit: u64) -> Result<Value> {
        self.transport
            .request(
                "thread/list",
                json!({
                    "cwd": cwd,
                    "limit": limit,
                    "archived": false,
                    "sortDirection": "desc",
                    "sortKey": "updated_at",
                }),
            )
            .await
    }

    pub async fn thread_resume(&mut self, thread_id: &str, cwd: &str) -> Result<String> {
        let result = self
            .transport
            .request(
                "thread/resume",
                json!({
                    "threadId": thread_id,
                    "cwd": cwd,
                    "excludeTurns": true,
                }),
            )
            .await?;

        json_string_at(&result, &["thread", "id"])
            .context("thread/resume response missing thread.id")
    }

    pub async fn respond_server_request(&mut self, id: Value, result: Value) -> Result<()> {
        self.transport.response(id, result).await
    }

    pub async fn wait_for_turn_completed(&mut self, turn_id: &str) -> Result<String> {
        let mut answer = String::new();
        let mut completed_item_answer: Option<String> = None;

        loop {
            let message = self.transport.next_message().await?;
            let method = message.get("method").and_then(Value::as_str);

            match method {
                Some("item/agentMessage/delta") => {
                    let params = message.get("params").unwrap_or(&Value::Null);
                    if params.get("turnId").and_then(Value::as_str) == Some(turn_id)
                        && let Some(delta) = params.get("delta").and_then(Value::as_str)
                    {
                        answer.push_str(delta);
                    }
                }
                Some("item/completed") => {
                    let params = message.get("params").unwrap_or(&Value::Null);
                    if params.get("turnId").and_then(Value::as_str) == Some(turn_id)
                        && let Some(item_answer) = completed_agent_answer(params)
                    {
                        completed_item_answer = Some(item_answer);
                    }
                }
                Some("turn/completed") => {
                    let params = message.get("params").unwrap_or(&Value::Null);
                    if params
                        .get("turn")
                        .and_then(|turn| turn.get("id"))
                        .and_then(Value::as_str)
                        != Some(turn_id)
                    {
                        continue;
                    }

                    let status = params
                        .get("turn")
                        .and_then(|turn| turn.get("status"))
                        .and_then(Value::as_str);

                    if status == Some("failed") {
                        let error = params
                            .get("turn")
                            .and_then(|turn| turn.get("error"))
                            .and_then(|error| error.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("turn failed without error message");
                        return Err(anyhow!(error.to_owned()));
                    }

                    if answer.is_empty() {
                        return Ok(completed_item_answer.unwrap_or_default());
                    }

                    return Ok(answer);
                }
                Some("error") => {
                    return Err(anyhow!(
                        "app-server error notification: {}",
                        message["params"]
                    ));
                }
                _ => {}
            }
        }
    }

    pub async fn next_message(&mut self) -> Result<Value> {
        self.transport.next_message().await
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        self.transport.shutdown().await
    }

    pub async fn run_prompt(mut self, options: RunOptions) -> Result<RunResult> {
        let cwd = options.cwd.to_string_lossy().into_owned();

        self.initialize().await?;
        let account = self.account_read().await?;
        let thread_id = self.thread_start(&cwd).await?;
        let turn_id = self
            .turn_start(&thread_id, &cwd, options.prompt.as_str())
            .await?;
        let answer = self.wait_for_turn_completed(&turn_id).await?;
        self.shutdown().await?;

        Ok(RunResult {
            answer,
            thread_id,
            turn_id,
            account,
        })
    }
}

pub async fn run_prompt(options: RunOptions) -> Result<RunResult> {
    AppServerClient::spawn().await?.run_prompt(options).await
}

fn json_string_at(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }

    current.as_str().map(str::to_owned)
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
