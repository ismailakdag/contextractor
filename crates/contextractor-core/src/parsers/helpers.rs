use super::ParseError;
use crate::model::{Role, TokenUsage, UsageConfidence};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn visit_jsonl<F>(path: &Path, mut visitor: F) -> Result<(), ParseError>
where
    F: FnMut(&Value),
{
    let file = File::open(path).map_err(|source| ParseError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut index = 0_usize;
    let mut parsed = 0_usize;
    let mut first_error = None;
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|source| ParseError::Io {
                path: path.display().to_string(),
                source,
            })?;
        if read == 0 {
            break;
        }
        index += 1;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(&line) {
            Ok(value) => {
                parsed += 1;
                visitor(&value);
            }
            Err(error) => {
                if !line.ends_with('\n') {
                    break;
                }
                first_error.get_or_insert_with(|| format!("line {index}: {error}"));
            }
        }
    }
    if parsed == 0 {
        if let Some(message) = first_error {
            return Err(ParseError::Json {
                path: path.display().to_string(),
                message,
            });
        }
    }
    Ok(())
}

pub fn read_json(path: &Path) -> Result<Value, ParseError> {
    let text = std::fs::read_to_string(path).map_err(|source| ParseError::Io {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|error| ParseError::Json {
        path: path.display().to_string(),
        message: error.to_string(),
    })
}

pub fn string(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer)?.as_str().map(ToOwned::to_owned)
}

pub fn boolean(value: &Value, pointer: &str) -> Option<bool> {
    value.pointer(pointer)?.as_bool()
}

pub fn role(value: Option<&str>) -> Role {
    match value.unwrap_or_default().to_ascii_lowercase().as_str() {
        "user" | "user_input" => Role::User,
        "assistant" | "planner_response" | "model" => Role::Assistant,
        "system" | "system_message" | "developer" => Role::System,
        "tool" | "tool_result" | "function" => Role::Tool,
        "reasoning" | "thinking" => Role::Reasoning,
        _ => Role::Unknown,
    }
}

pub fn extract_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(extract_text)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => {
            for key in [
                "text",
                "input_text",
                "output_text",
                "content",
                "message",
                "output",
            ] {
                if let Some(value) = map.get(key) {
                    let text = extract_text(value);
                    if !text.trim().is_empty() {
                        return text;
                    }
                }
            }
            String::new()
        }
        _ => String::new(),
    }
}

pub fn first_user_title(turns: &[crate::model::Turn]) -> Option<String> {
    turns
        .iter()
        .find(|turn| turn.role == Role::User && !turn.text.trim().is_empty())
        .map(|turn| compact_title(&user_request_text(&turn.text)))
}

pub fn is_injected_context(text: &str) -> bool {
    let normalized = text
        .trim_start()
        .trim_start_matches('#')
        .trim_start()
        .trim_start_matches('\\');
    let lower = normalized.to_ascii_lowercase();
    [
        "<system-reminder>",
        "<environment_context>",
        "<recommended_plugins>",
        "<additional_metadata>",
        "<developer_instructions>",
        "<app-context>",
        "<skills_instructions>",
        "<permissions instructions>",
        "<plugins_instructions>",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
}

pub fn user_request_text(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if let Some(start) = lower.find("<user_request>") {
        let content_start = start + "<user_request>".len();
        let tail = &text[content_start..];
        let tail_lower = &lower[content_start..];
        let end = tail_lower
            .find("</user_request>")
            .or_else(|| tail_lower.find("<additional_metadata>"))
            .unwrap_or(tail.len());
        return tail[..end].trim().to_string();
    }
    if let Some(end) = lower.find("<additional_metadata>") {
        return text[..end].trim().to_string();
    }
    text.trim().to_string()
}

pub fn compact_title(text: &str) -> String {
    let single_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = single_line.chars();
    let title: String = chars.by_ref().take(88).collect();
    if chars.next().is_some() {
        format!("{title}…")
    } else {
        title
    }
}

pub fn usage_from(value: &Value, source: &str) -> Option<TokenUsage> {
    let input = find_i64(value, &["input_tokens", "inputTokens"]);
    let output = find_i64(value, &["output_tokens", "outputTokens"]);
    let cache_read = find_i64(
        value,
        &[
            "cached_input_tokens",
            "cache_read_input_tokens",
            "cacheReadInputTokens",
        ],
    );
    let cache_write = find_i64(
        value,
        &["cache_creation_input_tokens", "cache_write_input_tokens", "cacheWriteInputTokens"],
    );
    let reasoning = find_i64(
        value,
        &[
            "reasoning_tokens",
            "reasoning_output_tokens",
            "reasoningTokens",
        ],
    );
    let total =
        find_i64(value, &["total_tokens", "totalTokens"]).or_else(|| match (input, output) {
            (Some(input), Some(output)) => Some(input + output),
            _ => None,
        });

    if input.is_none()
        && output.is_none()
        && cache_read.is_none()
        && cache_write.is_none()
        && reasoning.is_none()
        && total.is_none()
    {
        return None;
    }

    Some(TokenUsage {
        input_tokens: input,
        output_tokens: output,
        cached_input_tokens: cache_read,
        cache_write_input_tokens: cache_write,
        reasoning_tokens: reasoning,
        total_tokens: total,
        confidence: Some(UsageConfidence::Observed),
        source: Some(source.to_string()),
    })
}

fn find_i64(value: &Value, keys: &[&str]) -> Option<i64> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(number) = map.get(*key).and_then(Value::as_i64) {
                    return Some(number);
                }
            }
            map.values().find_map(|child| find_i64(child, keys))
        }
        Value::Array(items) => items.iter().find_map(|child| find_i64(child, keys)),
        _ => None,
    }
}

pub fn update_timestamp(current: &mut Option<String>, value: &Value) {
    if let Some(timestamp) = string(value, "/timestamp").or_else(|| string(value, "/created_at")) {
        *current = Some(timestamp);
    }
}

pub fn bounded_text(text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut boundary = max_bytes;
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let omitted = text.len() - boundary;
    format!(
        "{}\n\n[Contextractor preview truncated; {omitted} source bytes omitted]",
        &text[..boundary]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_user_request_without_metadata_wrapper() {
        let source = "<USER_REQUEST>Gerçek istek</USER_REQUEST>\n<ADDITIONAL_METADATA>gizli</ADDITIONAL_METADATA>";
        assert_eq!(user_request_text(source), "Gerçek istek");
        assert!(is_injected_context(
            "<recommended_plugins>liste</recommended_plugins>"
        ));
    }

    #[test]
    fn keeps_cache_reads_and_writes_separate() {
        let value = serde_json::json!({
            "usage": {
                "input_tokens": 100,
                "cache_read_input_tokens": 70,
                "cache_creation_input_tokens": 20,
                "output_tokens": 10
            }
        });
        let usage = usage_from(&value, "test").unwrap();
        assert_eq!(usage.cached_input_tokens, Some(70));
        assert_eq!(usage.cache_write_input_tokens, Some(20));
    }
}
