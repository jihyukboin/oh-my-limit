use crate::translator::TranslationDirection;

pub const INPUT_TRANSLATION_SYSTEM_PROMPT: &str = "You convert Korean developer instructions into compact English instructions for a coding agent. Preserve protected placeholders, file paths, code identifiers, commands, flags, JSON, diffs, logs, and error messages exactly. Return only the requested structured result.";

pub const OUTPUT_TRANSLATION_SYSTEM_PROMPT: &str = "You translate a coding agent's final English answer into natural Korean for a developer. Preserve protected placeholders, file paths, code identifiers, commands, flags, JSON, diffs, logs, and error messages exactly. Return only the requested structured result.";

pub fn system_prompt(direction: TranslationDirection) -> &'static str {
    match direction {
        TranslationDirection::KoreanToEnglish => INPUT_TRANSLATION_SYSTEM_PROMPT,
        TranslationDirection::EnglishToKorean => OUTPUT_TRANSLATION_SYSTEM_PROMPT,
    }
}

pub fn user_prompt(direction: TranslationDirection, text: &str) -> String {
    let task = match direction {
        TranslationDirection::KoreanToEnglish => {
            "Translate the following user instruction into compact English optimized for an LLM coding agent."
        }
        TranslationDirection::EnglishToKorean => {
            "Translate the following coding-agent final answer into Korean."
        }
    };

    format!("{task}\n\nInput:\n{text}")
}
