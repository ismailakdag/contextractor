use super::helpers::{
    bounded_text, extract_text, first_user_title, is_injected_context, role, string,
    update_timestamp, usage_from, visit_jsonl,
};
use super::ParseError;
use crate::model::{ParsedSession, Provider, Role, SourceCandidate, ToolCall, Turn};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn parse(candidate: &SourceCandidate) -> Result<ParsedSession, ParseError> {
    let mut external_id = None;
    let mut project_path = None;
    let mut created_at = None;
    let mut model = None;
    let mut updated_at = None;
    let mut turns = Vec::new();

    visit_jsonl(&candidate.path, |value| {
        update_timestamp(&mut updated_at, value);
        let record_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let payload = value.get("payload").unwrap_or(&Value::Null);
        match record_type {
            "session_meta" => {
                // Fork rollouts can embed the parent history, including its session_meta.
                // The first metadata row belongs to the file itself; later rows must not
                // collapse the fork back into its parent session.
                if external_id.is_none() {
                    external_id = string(payload, "/id").or_else(|| string(payload, "/session_id"));
                    project_path = string(payload, "/cwd");
                    created_at =
                        string(payload, "/timestamp").or_else(|| string(value, "/timestamp"));
                    model = string(payload, "/model");
                }
            }
            "turn_context" => {
                if model.is_none() {
                    model = string(payload, "/model");
                }
            }
            "response_item" => parse_response_item(payload, value, &mut turns),
            "event_msg" => {
                if payload.get("type").and_then(Value::as_str) == Some("token_count") {
                    let usage_value = payload
                        .pointer("/info/last_token_usage")
                        .or_else(|| payload.get("info"))
                        .unwrap_or(payload);
                    if let Some(usage) = usage_from(usage_value, "codex:event_msg/token_count") {
                        if let Some(turn) = turns
                            .iter_mut()
                            .rev()
                            .find(|turn| turn.role == Role::Assistant)
                        {
                            turn.usage = Some(usage);
                        }
                    }
                }
            }
            _ => {}
        }
    })?;

    let external_id = external_id
        .or_else(|| {
            candidate
                .path
                .file_stem()
                .and_then(|value| value.to_str())
                .and_then(|name| name.rsplit('-').next())
                .map(ToOwned::to_owned)
        })
        .ok_or_else(|| ParseError::MissingSessionId(candidate.path.display().to_string()))?;
    let title = indexed_title(&candidate.path, &external_id).or_else(|| first_user_title(&turns));

    Ok(ParsedSession {
        provider: Provider::Codex,
        source_kind: candidate.kind,
        external_id,
        title,
        project_path,
        source_path: candidate.path.clone(),
        created_at,
        updated_at,
        model,
        archived: candidate.archived,
        summary: None,
        turns,
        metadata_json: None,
    })
}

fn parse_response_item(payload: &Value, envelope: &Value, turns: &mut Vec<Turn>) {
    let item_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let timestamp = string(envelope, "/timestamp");
    let ordinal = turns.len() as i64;
    match item_type {
        "message" => {
            let mut role_value = role(payload.get("role").and_then(Value::as_str));
            let text = payload.get("content").map(extract_text).unwrap_or_default();
            if role_value == Role::User && is_injected_context(&text) {
                role_value = Role::System;
            }
            if !text.trim().is_empty() {
                turns.push(Turn {
                    external_id: string(payload, "/id"),
                    ordinal,
                    prompt_ordinal: None,
                    role: role_value,
                    created_at: timestamp,
                    text,
                    event_type: Some("message".to_string()),
                    model: None,
                    parent_external_id: None,
                    usage: None,
                    tool_calls: Vec::new(),
                    metadata_json: None,
                });
            }
        }
        "custom_tool_call" | "function_call" => {
            let name = string(payload, "/name").unwrap_or_else(|| "tool".to_string());
            let arguments_json = payload
                .get("input")
                .or_else(|| payload.get("arguments"))
                .map(|value| {
                    if value.is_string() {
                        value.as_str().unwrap_or_default().to_string()
                    } else {
                        value.to_string()
                    }
                });
            turns.push(Turn {
                external_id: string(payload, "/id"),
                ordinal,
                prompt_ordinal: None,
                role: Role::Tool,
                created_at: timestamp,
                text: String::new(),
                event_type: Some(item_type.to_string()),
                model: None,
                parent_external_id: None,
                usage: None,
                tool_calls: vec![ToolCall {
                    external_id: string(payload, "/call_id"),
                    name,
                    arguments_json,
                    result_text: None,
                    status: string(payload, "/status"),
                    duration_ms: None,
                }],
                metadata_json: None,
            });
        }
        "custom_tool_call_output" | "function_call_output" => {
            let call_id = string(payload, "/call_id");
            let result = bounded_text(
                payload.get("output").map(extract_text).unwrap_or_default(),
                256 * 1024,
            );
            if let Some(tool) = turns
                .iter_mut()
                .rev()
                .flat_map(|turn| turn.tool_calls.iter_mut())
                .find(|tool| call_id.is_some() && tool.external_id == call_id)
            {
                tool.result_text = Some(result);
            } else {
                turns.push(Turn {
                    external_id: string(payload, "/id"),
                    ordinal,
                    prompt_ordinal: None,
                    role: Role::Tool,
                    created_at: timestamp,
                    text: result,
                    event_type: Some(item_type.to_string()),
                    model: None,
                    parent_external_id: call_id,
                    usage: None,
                    tool_calls: Vec::new(),
                    metadata_json: None,
                });
            }
        }
        _ => {}
    }
}

fn indexed_title(source_path: &Path, session_id: &str) -> Option<String> {
    let mut current = source_path.parent();
    let mut index_path = None;
    while let Some(directory) = current {
        if matches!(
            directory.file_name().and_then(|name| name.to_str()),
            Some("sessions" | "archived_sessions")
        ) {
            index_path = directory
                .parent()
                .map(|root| root.join("session_index.jsonl"));
            break;
        }
        current = directory.parent();
    }
    let file = std::fs::File::open(index_path?).ok()?;
    let mut title = None;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("id").and_then(Value::as_str) == Some(session_id) {
            if let Some(value) = value.get("thread_name").and_then(Value::as_str) {
                if !value.trim().is_empty() {
                    title = Some(value.trim().to_string());
                }
            }
        }
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SourceKind;

    #[test]
    fn fork_keeps_its_first_session_identity() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let sessions = root.join("sessions").join("2026").join("08").join("21");
        std::fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join("rollout-child.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-08-21T17:07:05Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"child-id\",\"cwd\":\"E:\\\\trace analysis\"}}\n",
                "{\"timestamp\":\"2026-08-20T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"parent-id\",\"cwd\":\"E:\\\\old\"}}\n",
                "{\"timestamp\":\"2026-08-21T17:08:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Continue the fork\"}]}}\n"
            ),
        )
        .unwrap();
        std::fs::write(
            root.join("session_index.jsonl"),
            "{\"id\":\"child-id\",\"thread_name\":\"Trace Branch\"}\n",
        )
        .unwrap();
        let candidate = SourceCandidate {
            provider: Provider::Codex,
            kind: SourceKind::CodexRollout,
            path,
            archived: false,
            modified_at_ms: None,
            size_bytes: 1,
        };
        let parsed = parse(&candidate).unwrap();
        assert_eq!(parsed.external_id, "child-id");
        assert_eq!(parsed.project_path.as_deref(), Some("E:\\trace analysis"));
        assert_eq!(parsed.title.as_deref(), Some("Trace Branch"));
    }
}
