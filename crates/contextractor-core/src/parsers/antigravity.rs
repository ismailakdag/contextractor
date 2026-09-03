use super::helpers::{
    bounded_text, extract_text, first_user_title, role, string, update_timestamp, visit_jsonl,
};
use super::ParseError;
use crate::model::{ParsedSession, Provider, Role, SourceCandidate, ToolCall, Turn};
use serde_json::Value;

pub fn parse(candidate: &SourceCandidate) -> Result<ParsedSession, ParseError> {
    let mut turns = Vec::new();
    let mut created_at = None;
    let mut updated_at = None;

    visit_jsonl(&candidate.path, |value| {
        if created_at.is_none() {
            created_at = string(value, "/created_at");
        }
        update_timestamp(&mut updated_at, value);
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut event_role = role(Some(event_type));
        if matches!(
            event_type,
            "RUN_COMMAND" | "LIST_DIRECTORY" | "VIEW_FILE" | "CODE_ACTION"
        ) {
            event_role = Role::Tool;
        }
        if event_type == "CONVERSATION_HISTORY" || event_type == "CHECKPOINT" {
            return;
        }

        let text = value.get("content").map(extract_text).unwrap_or_default();
        let mut tool_calls = Vec::new();
        if let Some(calls) = value.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                tool_calls.push(ToolCall {
                    external_id: string(call, "/id").or_else(|| string(call, "/tool_call_id")),
                    name: string(call, "/name")
                        .or_else(|| string(call, "/tool_name"))
                        .unwrap_or_else(|| "tool".to_string()),
                    arguments_json: call
                        .get("arguments")
                        .or_else(|| call.get("input"))
                        .map(Value::to_string),
                    result_text: call
                        .get("result")
                        .map(extract_text)
                        .map(|text| bounded_text(text, 256 * 1024)),
                    status: string(call, "/status"),
                    duration_ms: call.get("duration_ms").and_then(Value::as_i64),
                });
            }
        }
        if event_role == Role::Tool && tool_calls.is_empty() {
            tool_calls.push(ToolCall {
                external_id: None,
                name: event_type.to_ascii_lowercase(),
                arguments_json: None,
                result_text: (!text.trim().is_empty())
                    .then(|| bounded_text(text.clone(), 256 * 1024)),
                status: string(value, "/status"),
                duration_ms: None,
            });
        }
        if !text.trim().is_empty() || !tool_calls.is_empty() {
            turns.push(Turn {
                external_id: None,
                ordinal: value
                    .get("step_index")
                    .and_then(Value::as_i64)
                    .unwrap_or(turns.len() as i64),
                prompt_ordinal: None,
                role: if event_role == Role::Unknown {
                    Role::System
                } else {
                    event_role
                },
                created_at: string(value, "/created_at"),
                text: if event_role == Role::Tool {
                    String::new()
                } else {
                    text
                },
                event_type: Some(event_type.to_string()),
                model: None,
                parent_external_id: None,
                usage: None,
                tool_calls,
                metadata_json: None,
            });
        }
    })?;

    turns.sort_by_key(|turn| turn.ordinal);
    for (ordinal, turn) in turns.iter_mut().enumerate() {
        turn.ordinal = ordinal as i64;
    }

    let external_id = candidate
        .path
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .and_then(|path| path.file_name())
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ParseError::MissingSessionId(candidate.path.display().to_string()))?;

    Ok(ParsedSession {
        provider: Provider::Antigravity,
        source_kind: candidate.kind,
        external_id,
        title: first_user_title(&turns),
        project_path: None,
        source_path: candidate.path.clone(),
        created_at,
        updated_at,
        model: None,
        archived: candidate.archived,
        summary: None,
        turns,
        metadata_json: None,
    })
}
