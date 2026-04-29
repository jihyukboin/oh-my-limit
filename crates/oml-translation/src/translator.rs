use std::{str::FromStr, time::Duration};

use anyhow::{Result, anyhow};
use async_trait::async_trait;

use crate::providers::{
    local_openai_compatible::LocalOpenAiCompatibleTranslator, noop::NoopTranslator,
    ollama::OllamaTranslator, remote_openai_compatible::RemoteOpenAiCompatibleTranslator,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TranslationDirection {
    KoreanToEnglish,
    EnglishToKorean,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TranslationProviderKind {
    Noop,
    Ollama,
    LocalOpenAiCompatible,
    OpenAi,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TranslatorConfig {
    pub provider: TranslationProviderKind,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TranslationRequest {
    pub direction: TranslationDirection,
    pub text: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TranslationResponse {
    pub text: String,
    pub provider: TranslationProviderKind,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProviderHealth {
    pub provider: TranslationProviderKind,
    pub message: String,
}

#[async_trait]
pub trait Translator: Send + Sync {
    async fn translate(&self, request: TranslationRequest) -> Result<TranslationResponse>;

    async fn health_check(&self) -> Result<ProviderHealth>;
}

impl FromStr for TranslationProviderKind {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "noop" | "off" | "none" => Ok(Self::Noop),
            "ollama" => Ok(Self::Ollama),
            "local-openai-compatible" | "local_openai_compatible" | "local" => {
                Ok(Self::LocalOpenAiCompatible)
            }
            "openai" | "remote-openai-compatible" | "remote_openai_compatible" => Ok(Self::OpenAi),
            other => Err(anyhow!("unknown translation provider: {other}")),
        }
    }
}

impl TranslationProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Noop => "noop",
            Self::Ollama => "ollama",
            Self::LocalOpenAiCompatible => "local-openai-compatible",
            Self::OpenAi => "openai",
        }
    }

    pub fn is_remote(self) -> bool {
        matches!(self, Self::OpenAi)
    }
}

pub fn build_translator(config: TranslatorConfig) -> Box<dyn Translator> {
    match config.provider {
        TranslationProviderKind::Noop => Box::new(NoopTranslator::new()),
        TranslationProviderKind::Ollama => Box::new(OllamaTranslator::new(config)),
        TranslationProviderKind::LocalOpenAiCompatible => {
            Box::new(LocalOpenAiCompatibleTranslator::new(config))
        }
        TranslationProviderKind::OpenAi => Box::new(RemoteOpenAiCompatibleTranslator::new(config)),
    }
}
