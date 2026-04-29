#[derive(Debug, Default)]
pub struct PlaceholderValidation;

pub fn validate_non_empty_translation(text: &str) -> anyhow::Result<()> {
    if text.trim().is_empty() {
        anyhow::bail!("translation provider returned empty text");
    }

    Ok(())
}
