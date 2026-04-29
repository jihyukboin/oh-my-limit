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
pub struct LocalOpenAiCompatibleTranslator {
    client: Client,
    config: TranslatorConfig,
}

impl LocalOpenAiCompatibleTranslator {
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
            .unwrap_or("http://localhost:1234/v1")
            .trim_end_matches('/')
    }

    fn model(&self) -> &str {
        self.config.model.as_deref().unwrap_or("local-model")
    }
}

#[async_trait::async_trait]
impl Translator for LocalOpenAiCompatibleTranslator {
    async fn translate(&self, request: TranslationRequest) -> Result<TranslationResponse> {
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url()))
            .json(&json!({
                "model": self.model(),
                "temperature": 0,
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
            .context("failed to call local OpenAI-compatible translation provider")?
            .error_for_status()
            .context("local OpenAI-compatible provider returned an error")?;

        let body = response
            .json::<Value>()
            .await
            .context("failed to parse local OpenAI-compatible response")?;
        let text = body
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        validate_non_empty_translation(&text)?;

        Ok(TranslationResponse {
            text,
            provider: TranslationProviderKind::LocalOpenAiCompatible,
            usage: None,
        })
    }

    async fn health_check(&self) -> Result<ProviderHealth> {
        self.client
            .get(format!("{}/models", self.base_url()))
            .send()
            .await
            .context("failed to reach local OpenAI-compatible provider")?
            .error_for_status()
            .context("local OpenAI-compatible health check returned an error")?;

        Ok(ProviderHealth {
            provider: TranslationProviderKind::LocalOpenAiCompatible,
            message: format!(
                "local OpenAI-compatible provider reachable at {}",
                self.base_url()
            ),
        })
    }
}
