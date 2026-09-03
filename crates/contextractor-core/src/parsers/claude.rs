use super::helpers::{
    boolean, bounded_text, extract_text, first_user_title, is_injected_context, read_json, role,
    string, update_timestamp, usage_from, visit_jsonl,
};
use super::ParseError;
use crate::model::{ParsedSession, Provider, Role, SourceCandidate, SourceKind, ToolCall, Turn};
use serde_json::Value;

pub fn parse(candidate: &SourceCandidate) -> Result<ParsedSession, ParseError> {
    match candidate.kind {
        SourceKind::ClaudeCodeProject | SourceKind::ClaudeLocalAudit => parse_code(candidate),
        SourceKind::ClaudeDesktopMetadata => parse_desktop_metadata(candidate),
        _ => Err(ParseError::Unsupported(candidate.kind)),
    }
}

fn parse_code(candidate: &SourceCandidate) -> Result<ParsedSession, ParseError> {
    let mut external_id = if candidate.kind == SourceKind::ClaudeLocalAudit {
        candidate
            .path
            .parent()
            .and_then(|value| value.file_name())
            .and_then(|value| value.to_str())
            .map(ToOwned::to_owned)
    } else {
        None
    };
    let mut project_path = None;
    let mut created_at = None;
    let mut model = None;
    let mut updated_at = None;
    let mut turns = Vec::new();
    let mut seen_message_ids = std::collections::HashSet::new();

    visit_jsonl(&candidate.path, |value| {
        update_timestamp(&mut updated_at, value);
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if external_id.is_none() {
            external_id = string(value, "/sessionId").or_else(|| string(value, "/session_id"));
        }
        if project_path.is_none() {
            project_path = string(value, "/cwd");
        }
        if created_at.is_none() {
            created_at = string(value, "/timestamp");
        }

        match event_type {
            "user" | "assistant" => {
                let message = value.get("message").unwrap_or(&Value::Null);
                let message_id = string(value, "/uuid").or_else(|| string(message, "/id"));
                if message_id
                    .as_ref()
                    .is_some_and(|id| !seen_message_ids.insert(id.clone()))
                {
                    return;
                }
                let mut message_role = role(
                    message
                        .get("role")
                        .and_then(Value::as_str)
                        .or(Some(event_type)),
                );
                let content = message.get("content").unwrap_or(&Value::Null);
                let text = extract_visible_text(content);
                if message_role == Role::User
                    && (boolean(value, "/isMeta").unwrap_or(false) || is_injected_context(&text))
                {
                    message_role = Role::System;
                }
                let mut tool_calls = Vec::new();
                collect_tool_calls(content, &mut tool_calls);
                attach_tool_results(content, &mut turns);
                let usage = message
                    .get("usage")
                    .and_then(|usage| usage_from(usage, "claude:message/usage"));
                if model.is_none() {
                    model = string(message, "/model");
                }
                if !text.trim().is_empty() || !tool_calls.is_empty() {
                    turns.push(Turn {
                        external_id: message_id,
                        ordinal: turns.len() as i64,
                        prompt_ordinal: None,
                        role: message_role,
                        created_at: string(value, "/timestamp"),
                        text,
                        event_type: Some(if message_role == Role::System && event_type == "user" {
                            "context:injected".to_string()
                        } else {
                            event_type.to_string()
                        }),
                        model: string(message, "/model"),
                        parent_external_id: string(value, "/parentUuid"),
                        usage,
                        tool_calls,
                        metadata_json: None,
                    });
                }
            }
            "system" => {
                let text = value.get("content").map(extract_text).unwrap_or_default();
                if !text.trim().is_empty() {
                    turns.push(Turn {
                        external_id: string(value, "/uuid"),
                        ordinal: turns.len() as i64,
                        prompt_ordinal: None,
                        role: Role::System,
                        created_at: string(value, "/timestamp"),
                        text,
                        event_type: string(value, "/subtype")
                            .or_else(|| Some("system".to_string())),
                        model: None,
                        parent_external_id: string(value, "/parentUuid"),
                        usage: None,
                        tool_calls: Vec::new(),
                        metadata_json: None,
                    });
                }
            }
            "attachment" => {
                let attachment = value.get("attachment").unwrap_or(&Value::Null);
                let attachment_type =
                    string(attachment, "/type").unwrap_or_else(|| "attachment".to_string());
                let text = attachment_text(attachment);
                if !text.trim().is_empty() {
                    let file_path = string(attachment, "/filename")
                        .or_else(|| string(attachment, "/content/file/filePath"));
                    turns.push(Turn {
                        external_id: string(value, "/uuid"),
                        ordinal: turns.len() as i64,
                        prompt_ordinal: None,
                        role: Role::System,
                        created_at: string(value, "/timestamp"),
                        text: bounded_text(text, 512 * 1024),
                        event_type: Some(format!("context:{attachment_type}")),
                        model: None,
                        parent_external_id: string(value, "/parentUuid"),
                        usage: None,
                        tool_calls: Vec::new(),
                        metadata_json: file_path.map(|path| {
                            serde_json::json!({ "path": path, "attachment_type": attachment_type })
                                .to_string()
                        }),
                    });
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
                .map(ToOwned::to_owned)
        })
        .ok_or_else(|| ParseError::MissingSessionId(candidate.path.display().to_string()))?;

    Ok(ParsedSession {
        provider: Provider::Claude,
        source_kind: candidate.kind,
        external_id,
        title: first_user_title(&turns),
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

fn attachment_text(attachment: &Value) -> String {
    match string(attachment, "/type").as_deref() {
        Some("file") => {
            let path = string(attachment, "/filename")
                .or_else(|| string(attachment, "/content/file/filePath"));
            let content = string(attachment, "/content/file/content")
                .or_else(|| string(attachment, "/content/text"))
                .or_else(|| string(attachment, "/content"));
            [
                path.map(|value| format!("Dosya eki: @\"{value}\"")),
                content,
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n")
        }
        Some("mcp_instructions_delta") => attachment
            .get("addedBlocks")
            .map(extract_text)
            .unwrap_or_default(),
        Some("skill_listing") => string(attachment, "/content").unwrap_or_default(),
        _ => attachment
            .get("content")
            .map(extract_text)
            .unwrap_or_default(),
    }
}

fn parse_desktop_metadata(candidate: &SourceCandidate) -> Result<ParsedSession, ParseError> {
    let value = read_json(&candidate.path)?;
    let external_id = string(&value, "/cliSessionId")
        .or_else(|| string(&value, "/sessionId"))
        .ok_or_else(|| ParseError::MissingSessionId(candidate.path.display().to_string()))?;
    Ok(ParsedSession {
        provider: Provider::Claude,
        source_kind: candidate.kind,
        external_id,
        title: string(&value, "/title"),
        project_path: string(&value, "/cwd").or_else(|| string(&value, "/originCwd")),
        source_path: candidate.path.clone(),
        created_at: string(&value, "/createdAt"),
        updated_at: string(&value, "/lastActivityAt"),
        model: string(&value, "/model"),
        archived: boolean(&value, "/isArchived").unwrap_or(false),
        summary: None,
        turns: Vec::new(),
        metadata_json: Some(serde_json::json!({ "desktop_metadata": true }).to_string()),
    })
}

fn extract_visible_text(content: &Value) -> String {
    match content {
        Value::Array(items) => items
            .iter()
            .filter(|item| {
                !matches!(
                    item.get("type").and_then(Value::as_str),
                    Some("tool_use" | "tool_result" | "thinking")
                )
            })
            .map(extract_text)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => extract_text(content),
    }
}

fn collect_tool_calls(content: &Value, output: &mut Vec<ToolCall>) {
    let Value::Array(items) = content else {
        return;
    };
    for item in items {
        if item.get("type").and_then(Value::as_str) == Some("tool_use") {
            output.push(ToolCall {
                external_id: string(item, "/id"),
                name: string(item, "/name").unwrap_or_else(|| "tool".to_string()),
                arguments_json: item.get("input").map(Value::to_string),
                result_text: None,
                status: None,
                duration_ms: None,
            });
        }
    }
}

fn attach_tool_results(content: &Value, turns: &mut [Turn]) {
    let Value::Array(items) = content else {
        return;
    };
    for item in items {
        if item.get("type").and_then(Value::as_str) != Some("tool_result") {
            continue;
        }
        let call_id = string(item, "/tool_use_id");
        let result = bounded_text(
            item.get("content").map(extract_text).unwrap_or_default(),
            256 * 1024,
        );
        if let Some(tool) = turns
            .iter_mut()
            .rev()
            .flat_map(|turn| turn.tool_calls.iter_mut())
            .find(|tool| call_id.is_some() && tool.external_id == call_id)
        {
            tool.result_text = Some(result);
            tool.status = Some(
                if boolean(item, "/is_error").unwrap_or(false) {
                    "error"
                } else {
                    "completed"
                }
                .to_string(),
            );
        }
    }
}
