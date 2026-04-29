use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{Value, json};

use crate::{
    prompts::{system_prompt, user_prompt},
    translator::{
        ProviderHealth, TranslationProviderKind, TranslationRequest, TranslationResponse,
        Translator, TranslatorConfig,
    },
    validate::validate_non_empty_translation,
};

#[derive(Debug, Clone)]
pub struct OllamaTranslator {
    client: Client,
    config: TranslatorConfig,
}

impl OllamaTranslator {
    pub fn new(config: TranslatorConfig) -> Self {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self { client, config }
    }

    fn base_url(&self) -> &str {
        self.config
            .base_url
            .as_deref()
            .unwrap_or("http://localhost:11434")
            .trim_end_matches('/')
    }

    fn model(&self) -> &str {
        self.config.model.as_deref().unwrap_or("qwen2.5-coder:7b")
    }
}

#[async_trait::async_trait]
impl Translator for OllamaTranslator {
    async fn translate(&self, request: TranslationRequest) -> Result<TranslationResponse> {
        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url()))
            .json(&json!({
                "model": self.model(),
                "stream": false,
                "messages": [
                    {
                        "role": "system",
                        "content": system_prompt(request.direction),
                    },
                    {
                        "role": "user",
                        "content": user_prompt(request.direction, &request.text),
                    }
                ]
            }))
            .send()
            .await
            .context("failed to call Ollama translation provider")?
            .error_for_status()
            .context("Ollama translation provider returned an error")?;

        let body = response
            .json::<Value>()
            .await
            .context("failed to parse Ollama translation response")?;
        let text = body
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        validate_non_empty_translation(&text)?;

        Ok(TranslationResponse {
            text,
            provider: TranslationProviderKind::Ollama,
        })
    }

    async fn health_check(&self) -> Result<ProviderHealth> {
        self.client
            .get(format!("{}/api/tags", self.base_url()))
            .send()
            .await
            .context("failed to reach Ollama")?
            .error_for_status()
            .context("Ollama health check returned an error")?;

        Ok(ProviderHealth {
            provider: TranslationProviderKind::Ollama,
            message: format!("Ollama reachable at {}", self.base_url()),
        })
    }
}
