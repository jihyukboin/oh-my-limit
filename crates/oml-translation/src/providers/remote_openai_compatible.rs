use anyhow::{Context, Result, anyhow};
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
pub struct RemoteOpenAiCompatibleTranslator {
    client: Client,
    config: TranslatorConfig,
}

impl RemoteOpenAiCompatibleTranslator {
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
            .unwrap_or("https://api.openai.com/v1")
            .trim_end_matches('/')
    }

    fn model(&self) -> &str {
        self.config.model.as_deref().unwrap_or("gpt-4.1-mini")
    }

    fn api_key(&self) -> Result<&str> {
        self.config
            .api_key
            .as_deref()
            .filter(|key| !key.trim().is_empty())
            .ok_or_else(|| anyhow!("OpenAI provider requires an API key env var"))
    }
}

#[async_trait::async_trait]
impl Translator for RemoteOpenAiCompatibleTranslator {
    async fn translate(&self, request: TranslationRequest) -> Result<TranslationResponse> {
        let response = self
            .client
            .post(format!("{}/responses", self.base_url()))
            .bearer_auth(self.api_key()?)
            .json(&json!({
                "model": self.model(),
                "instructions": system_prompt(request.direction),
                "input": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": user_prompt(request.direction, &request.text),
                            }
                        ],
                    }
                ],
                "text": {
                    "format": {
                        "type": "json_schema",
                        "name": "translation_result",
                        "strict": true,
                        "schema": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": {
                                "translated_text": {
                                    "type": "string",
                                    "description": "The translated text only.",
                                }
                            },
                            "required": ["translated_text"]
                        }
                    }
                }
            }))
            .send()
            .await
            .context("failed to call OpenAI translation provider")?
            .error_for_status()
            .context("OpenAI translation provider returned an error")?;

        let body = response
            .json::<Value>()
            .await
            .context("failed to parse OpenAI translation response")?;
        let text = translated_text_from_response(&body)?;
        validate_non_empty_translation(&text)?;

        Ok(TranslationResponse {
            text,
            provider: TranslationProviderKind::OpenAi,
        })
    }

    async fn health_check(&self) -> Result<ProviderHealth> {
        self.client
            .get(format!("{}/models", self.base_url()))
            .bearer_auth(self.api_key()?)
            .send()
            .await
            .context("failed to reach OpenAI")?
            .error_for_status()
            .context("OpenAI health check returned an error")?;

        Ok(ProviderHealth {
            provider: TranslationProviderKind::OpenAi,
            message: "OpenAI API reachable".to_owned(),
        })
    }
}

fn translated_text_from_response(body: &Value) -> Result<String> {
    if let Some(output_text) = body.get("output_text").and_then(Value::as_str)
        && let Some(text) = translated_text_from_json(output_text)?
    {
        return Ok(text);
    }

    let output = body
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("OpenAI response missing output"))?;

    for item in output {
        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };

        for part in content {
            if let Some(refusal) = part.get("refusal").and_then(Value::as_str) {
                return Err(anyhow!("OpenAI refused translation: {refusal}"));
            }

            let Some(text) = part.get("text").and_then(Value::as_str) else {
                continue;
            };
            if let Some(translated_text) = translated_text_from_json(text)? {
                return Ok(translated_text);
            }
        }
    }

    Err(anyhow!("OpenAI response did not contain translated_text"))
}

fn translated_text_from_json(text: &str) -> Result<Option<String>> {
    let value = serde_json::from_str::<Value>(text).with_context(|| {
        format!(
            "OpenAI structured translation response was not JSON: {}",
            text.chars().take(120).collect::<String>()
        )
    })?;

    Ok(value
        .get("translated_text")
        .and_then(Value::as_str)
        .map(str::to_owned))
}
