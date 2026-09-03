use super::helpers::{
    bounded_text, extract_text, first_user_title, read_json, role, string, visit_jsonl,
};
use super::ParseError;
use crate::model::{ParsedSession, Provider, Role, SourceCandidate, ToolCall, Turn};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;

pub fn parse(candidate: &SourceCandidate) -> Result<ParsedSession, ParseError> {
    let summary = read_adjacent_summary(candidate);
    let mut values = all_recap_history(candidate);
    let mut current = Vec::new();
    visit_jsonl(&candidate.path, |value| current.push(value.clone()))?;

    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(event_identity(value)));
    for value in current {
        if seen.insert(event_identity(&value)) {
            values.push(value);
        }
    }

    let mut turns: Vec<Turn> = Vec::new();
    let mut model = summary
        .as_ref()
        .and_then(|value| string(value, "/current_model_id"));
    for value in &values {
        parse_event(value, &mut turns, &mut model);
    }
    enrich_turn_timestamps(candidate, &mut turns);

    let external_id = candidate
        .path
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ParseError::MissingSessionId(candidate.path.display().to_string()))?;
    let project_path = candidate
        .path
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.file_name())
        .and_then(|value| value.to_str())
        .and_then(|value| urlencoding::decode(value).ok())
        .map(|value| value.into_owned());
    let created_at = summary
        .as_ref()
        .and_then(|value| string(value, "/created_at"))
        .or_else(|| uuid_v7_timestamp(&external_id));
    let updated_at = summary
        .as_ref()
        .and_then(|value| string(value, "/last_active_at").or_else(|| string(value, "/updated_at")))
        .or_else(|| modified_timestamp(candidate.modified_at_ms));
    let source_turn_count = summary
        .as_ref()
        .and_then(|value| value.get("next_trace_turn"))
        .and_then(Value::as_i64);

    Ok(ParsedSession {
        provider: Provider::Grok,
        source_kind: candidate.kind,
        external_id,
        title: summary
            .as_ref()
            .and_then(|value| {
                string(value, "/generated_title")
                    .or_else(|| string(value, "/session_summary"))
                    .or_else(|| string(value, "/title"))
            })
            .or_else(|| first_user_title(&turns)),
        project_path,
        source_path: candidate.path.clone(),
        created_at,
        updated_at,
        model,
        archived: candidate.archived,
        summary: summary.as_ref().and_then(|value| {
            string(value, "/last_recap")
                .or_else(|| string(value, "/last_turn_summary"))
                .or_else(|| string(value, "/summary"))
                .or_else(|| string(value, "/recap"))
        }),
        turns,
        metadata_json: source_turn_count
            .map(|count| json!({ "source_turn_count": count }).to_string()),
    })
}

fn parse_event(value: &Value, turns: &mut Vec<Turn>, model: &mut Option<String>) {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut event_role = role(Some(event_type));
    let mut text = value.get("content").map(extract_text).unwrap_or_default();
    if event_type == "user" && is_synthetic_context(value, &text) {
        event_role = Role::System;
    } else if event_type == "user" {
        text = unwrap_tag(&text, "user_query");
    }
    let mut tool_calls = Vec::new();

    if let Some(calls) = value.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let function = call.get("function").unwrap_or(call);
            tool_calls.push(ToolCall {
                external_id: string(call, "/id"),
                name: string(function, "/name").unwrap_or_else(|| "tool".to_string()),
                arguments_json: function.get("arguments").map(|arguments| {
                    arguments
                        .as_str()
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| arguments.to_string())
                }),
                result_text: None,
                status: None,
                duration_ms: None,
            });
        }
    }

    if event_type == "tool_result" {
        let call_id = string(value, "/tool_call_id");
        if let Some(tool) = turns
            .iter_mut()
            .rev()
            .flat_map(|turn| turn.tool_calls.iter_mut())
            .find(|tool| call_id.is_some() && tool.external_id == call_id)
        {
            tool.result_text = Some(bounded_text(text, 256 * 1024));
            tool.status = Some("completed".to_string());
            return;
        }
    }

    if event_type == "assistant" && model.is_none() {
        *model = string(value, "/model_id");
    }
    if !text.trim().is_empty() || !tool_calls.is_empty() {
        turns.push(Turn {
            external_id: string(value, "/id"),
            ordinal: turns.len() as i64,
            prompt_ordinal: None,
            role: if event_role == Role::Unknown {
                Role::System
            } else {
                event_role
            },
            created_at: string(value, "/timestamp").or_else(|| string(value, "/created_at")),
            text,
            event_type: Some(if event_role == Role::System && event_type == "user" {
                format!(
                    "context:{}",
                    string(value, "/synthetic_reason").unwrap_or_else(|| "injected".to_string())
                )
            } else {
                event_type.to_string()
            }),
            model: string(value, "/model_id"),
            parent_external_id: None,
            usage: None,
            tool_calls,
            metadata_json: None,
        });
    }
}

fn is_synthetic_context(value: &Value, text: &str) -> bool {
    if value.get("synthetic_reason").is_some() {
        return true;
    }
    let text = text.trim_start().to_ascii_lowercase();
    [
        "<user_info>",
        "<system-reminder>",
        "<environment_context>",
        "<recommended_plugins>",
        "<rules>",
    ]
    .iter()
    .any(|prefix| text.starts_with(prefix))
}

fn unwrap_tag(text: &str, tag: &str) -> String {
    let trimmed = text.trim();
    let opening = format!("<{tag}>");
    let closing = format!("</{tag}>");
    trimmed
        .strip_prefix(&opening)
        .and_then(|value| value.strip_suffix(&closing))
        .map(str::trim)
        .unwrap_or(trimmed)
        .to_string()
}

fn all_recap_history(candidate: &SourceCandidate) -> Vec<Value> {
    let Some(directory) = candidate
        .path
        .parent()
        .map(|path| path.join("recap_requests"))
    else {
        return Vec::new();
    };
    let mut files = fs::read_dir(directory)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    files.sort_by_key(|entry| entry.metadata().and_then(|value| value.modified()).ok());
    let mut history = Vec::new();
    let mut seen = HashSet::new();
    for entry in files {
        let Some(items) = read_json(&entry.path())
            .ok()
            .and_then(|value| value.get("chat_history").and_then(Value::as_array).cloned())
        else {
            continue;
        };
        for item in items {
            if seen.insert(event_identity(&item)) {
                history.push(item);
            }
        }
    }
    history
}

fn event_identity(value: &Value) -> String {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if let Some(id) = string(value, "/id")
        .or_else(|| string(value, "/tool_call_id"))
        .or_else(|| string(value, "/content/0/id"))
    {
        return format!("{event_type}:{id}");
    }
    if let Some(index) = value
        .pointer("/content/0/prompt_index")
        .and_then(Value::as_i64)
    {
        return format!("{event_type}:prompt:{index}");
    }
    let mut hasher = Sha256::new();
    hasher.update(value.to_string().as_bytes());
    format!("{event_type}:{:x}", hasher.finalize())
}

fn read_adjacent_summary(candidate: &SourceCandidate) -> Option<Value> {
    let path = candidate.path.parent()?.join("summary.json");
    path.is_file().then(|| read_json(&path).ok()).flatten()
}

fn uuid_v7_timestamp(value: &str) -> Option<String> {
    let compact = value.replace('-', "");
    let millis = i64::from_str_radix(compact.get(..12)?, 16).ok()?;
    DateTime::<Utc>::from_timestamp_millis(millis).map(|value| value.to_rfc3339())
}

fn modified_timestamp(value: Option<i64>) -> Option<String> {
    DateTime::<Utc>::from_timestamp_millis(value?).map(|value| value.to_rfc3339())
}

fn enrich_turn_timestamps(candidate: &SourceCandidate, turns: &mut [Turn]) {
    let Some(path) = candidate
        .path
        .parent()
        .map(|path| path.join("events.jsonl"))
    else {
        return;
    };
    if !path.is_file() {
        return;
    }
    let mut started = Vec::new();
    let mut ended = Vec::new();
    if visit_jsonl(&path, |value| {
        let timestamp = string(value, "/ts");
        match value.get("type").and_then(Value::as_str) {
            Some("turn_started") => started.push(timestamp),
            Some("turn_ended") => ended.push(timestamp),
            _ => {}
        }
    })
    .is_err()
    {
        return;
    }
    let mut interaction = None;
    for turn in turns {
        if turn.role == Role::User {
            let index = interaction.map_or(0, |value: usize| value + 1);
            interaction = Some(index);
            if turn.created_at.is_none() {
                turn.created_at = started.get(index).cloned().flatten();
            }
        } else if turn.created_at.is_none() {
            if let Some(index) = interaction {
                turn.created_at = ended
                    .get(index)
                    .cloned()
                    .flatten()
                    .or_else(|| started.get(index).cloned().flatten());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SourceKind;

    #[test]
    fn recovers_turn_times_from_grok_event_log() {
        let temp = tempfile::tempdir().unwrap();
        let session_dir = temp
            .path()
            .join("project")
            .join("019fdb9e-b054-7fe0-a154-7403514dacea");
        std::fs::create_dir_all(&session_dir).unwrap();
        let history = session_dir.join("chat_history.jsonl");
        std::fs::write(
            &history,
            concat!(
                "{\"type\":\"user\",\"content\":\"hello\"}\n",
                "{\"type\":\"assistant\",\"content\":\"world\"}\n"
            ),
        )
        .unwrap();
        std::fs::write(
            session_dir.join("events.jsonl"),
            concat!(
                "{\"ts\":\"2026-08-07T09:48:09.328Z\",\"type\":\"turn_started\"}\n",
                "{\"ts\":\"2026-08-07T09:49:11.000Z\",\"type\":\"turn_ended\"}\n"
            ),
        )
        .unwrap();
        let candidate = SourceCandidate {
            provider: Provider::Grok,
            kind: SourceKind::GrokCliHistory,
            path: history,
            archived: false,
            modified_at_ms: None,
            size_bytes: 1,
        };
        let parsed = parse(&candidate).unwrap();
        assert_eq!(
            parsed.turns[0].created_at.as_deref(),
            Some("2026-08-07T09:48:09.328Z")
        );
        assert_eq!(
            parsed.turns[1].created_at.as_deref(),
            Some("2026-08-07T09:49:11.000Z")
        );
    }
}
