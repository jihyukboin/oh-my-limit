use std::{collections::HashMap, fs, path::Path};

use anyhow::{Context, Result};

pub fn load_env_file(path: &Path) -> Result<HashMap<String, String>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read env file {}", path.display()))?;
    Ok(parse_env_file(&text))
}

pub fn save_env_value(path: &Path, key: &str, value: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create env dir {}", parent.display()))?;
    }

    let mut values = load_env_file(path)?;
    values.insert(key.to_owned(), value.to_owned());

    let mut entries = values.into_iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let text = entries
        .into_iter()
        .map(|(key, value)| format!("{key}={}", quote_env_value(&value)))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, format!("{text}\n"))
        .with_context(|| format!("failed to write env file {}", path.display()))
}

fn parse_env_file(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(parse_env_line)
        .collect::<HashMap<_, _>>()
}

fn parse_env_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }

    Some((key.to_owned(), unquote_env_value(value.trim())))
}

fn unquote_env_value(value: &str) -> String {
    let quoted = (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''));
    if quoted && value.len() >= 2 {
        value[1..value.len() - 1].to_owned()
    } else {
        value.to_owned()
    }
}

fn quote_env_value(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
    {
        return value.to_owned();
    }

    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
