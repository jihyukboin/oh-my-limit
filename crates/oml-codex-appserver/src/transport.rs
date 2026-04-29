use std::process::Stdio;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
};

use crate::process::{APP_SERVER_ARGS, APP_SERVER_COMMAND};

#[derive(Debug)]
pub struct StdioJsonlTransport {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl StdioJsonlTransport {
    pub async fn spawn() -> Result<Self> {
        let mut child = Command::new(APP_SERVER_COMMAND)
            .args(APP_SERVER_ARGS)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to spawn `{APP_SERVER_COMMAND}`"))?;

        let stdin = child
            .stdin
            .take()
            .context("codex app-server stdin is unavailable")?;
        let stdout = child
            .stdout
            .take()
            .context("codex app-server stdout is unavailable")?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
            next_id: 0,
        })
    }

    pub async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_request_id();
        self.write(json!({
            "id": id,
            "method": method,
            "params": params,
        }))
        .await?;

        loop {
            let message = self.next_message().await?;

            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }

            if let Some(error) = message.get("error") {
                return Err(anyhow!("app-server request `{method}` failed: {error}"));
            }

            return message
                .get("result")
                .cloned()
                .ok_or_else(|| anyhow!("app-server request `{method}` response missing result"));
        }
    }

    pub async fn notification(&mut self, method: &str, params: Value) -> Result<()> {
        self.write(json!({
            "method": method,
            "params": params,
        }))
        .await
    }

    pub async fn response(&mut self, id: Value, result: Value) -> Result<()> {
        self.write(json!({
            "id": id,
            "result": result,
        }))
        .await
    }

    pub async fn next_message(&mut self) -> Result<Value> {
        let line = self
            .stdout
            .next_line()
            .await
            .context("failed to read app-server stdout")?
            .ok_or_else(|| anyhow!("codex app-server closed stdout"))?;

        serde_json::from_str(&line).with_context(|| format!("invalid app-server JSONL: {line}"))
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        if self.child.id().is_some() {
            let _ = self.child.kill().await;
            let _ = self.child.wait().await;
        }

        Ok(())
    }

    async fn write(&mut self, message: Value) -> Result<()> {
        let mut line = serde_json::to_vec(&message).context("failed to encode app-server JSON")?;
        line.push(b'\n');
        self.stdin
            .write_all(&line)
            .await
            .context("failed to write app-server stdin")?;
        self.stdin
            .flush()
            .await
            .context("failed to flush app-server stdin")
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}
