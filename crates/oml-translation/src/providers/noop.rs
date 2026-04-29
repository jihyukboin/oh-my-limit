#[derive(Debug, Default)]
pub struct NoopTranslator;

impl NoopTranslator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl crate::translator::Translator for NoopTranslator {
    async fn translate(
        &self,
        request: crate::translator::TranslationRequest,
    ) -> anyhow::Result<crate::translator::TranslationResponse> {
        Ok(crate::translator::TranslationResponse {
            text: request.text,
            provider: crate::translator::TranslationProviderKind::Noop,
        })
    }

    async fn health_check(&self) -> anyhow::Result<crate::translator::ProviderHealth> {
        Ok(crate::translator::ProviderHealth {
            provider: crate::translator::TranslationProviderKind::Noop,
            message: "translation disabled".to_owned(),
        })
    }
}
