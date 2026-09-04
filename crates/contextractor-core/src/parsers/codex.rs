use super::helpers::{
    bounded_text, extract_text, first_user_title, is_injected_context, role, string,
    update_timestamp, usage_from, visit_jsonl,
};
use super::ParseError;
use crate::model::{ParsedSession, Provider, Role, SourceCandidate, ToolCall, Turn};
use serde_json::json;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn parse(candidate: &SourceCandidate) -> Result<ParsedSession, ParseError> {
    let mut external_id = None;
    let mut project_path = None;
    let mut created_at = None;
    let mut model = None;
    let mut updated_at = None;
    let mut forked_from_id = None;
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
                    forked_from_id = string(payload, "/forked_from_id");
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
    let native_user_turn_count = if forked_from_id.is_some() {
        created_at
            .as_deref()
            .and_then(|created| chrono::DateTime::parse_from_rfc3339(created).ok())
            .map(|created| {
                turns
                    .iter()
                    .filter(|turn| turn.role == Role::User)
                    .filter(|turn| provider_turn_time(turn).is_some_and(|value| value >= created))
                    .count()
            })
            .unwrap_or_default()
    } else {
        turns.iter().filter(|turn| turn.role == Role::User).count()
    };

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
        metadata_json: Some(
            json!({
                "forked_from_id": forked_from_id,
                "native_user_turn_count": native_user_turn_count,
                "inherited_fork_snapshot": forked_from_id.is_some() && native_user_turn_count == 0,
            })
            .to_string(),
        ),
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
                let provider_create_time = payload
                    .pointer("/internal_chat_message_metadata_passthrough/create_time")
                    .and_then(Value::as_f64);
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
                    metadata_json: provider_create_time
                        .map(|value| json!({ "provider_create_time": value }).to_string()),
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

fn provider_turn_time(turn: &Turn) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    turn.metadata_json
        .as_deref()
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .and_then(|value| value.get("provider_create_time").and_then(Value::as_f64))
        .and_then(|value| chrono::DateTime::from_timestamp_millis((value * 1000.0) as i64))
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value.to_rfc3339()).ok())
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
                "{\"timestamp\":\"2026-08-21T17:08:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Continue the fork\"}],\"internal_chat_message_metadata_passthrough\":{\"create_time\":1787332080.0}}}\n"
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
        assert_eq!(
            parsed
                .metadata_json
                .as_deref()
                .and_then(|value| serde_json::from_str::<Value>(value).ok())
                .and_then(|value| value
                    .get("inherited_fork_snapshot")
                    .and_then(Value::as_bool)),
            Some(false)
        );
    }

    #[test]
    fn inherited_fork_without_a_new_user_turn_is_marked_as_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let sessions = root.join("sessions").join("2026").join("08").join("21");
        std::fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join("rollout-child.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-08-21T17:07:02Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"child-id\",\"forked_from_id\":\"parent-id\",\"timestamp\":\"2026-08-21T17:07:01Z\",\"cwd\":\"E:\\\\trace analysis\"}}\n",
                "{\"timestamp\":\"2026-08-20T10:00:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Inherited prompt\"}]}}\n"
            ),
        )
        .unwrap();
        let candidate = SourceCandidate {
            provider: Provider::Codex,
            kind: SourceKind::CodexRollout,
            path,
            archived: true,
            modified_at_ms: None,
            size_bytes: 1,
        };
        let parsed = parse(&candidate).unwrap();
        let metadata: Value =
            serde_json::from_str(parsed.metadata_json.as_deref().unwrap()).unwrap();
        assert_eq!(
            metadata
                .get("inherited_fork_snapshot")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            metadata
                .get("native_user_turn_count")
                .and_then(Value::as_u64),
            Some(0)
        );
    }
}
