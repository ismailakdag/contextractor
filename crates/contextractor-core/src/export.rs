use crate::model::{Role, StoredSession};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Markdown,
    Json,
    Jsonl,
    Prompts,
    SystemPrompts,
    ContextPrompts,
    Responses,
    ToolCalls,
    Summary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportOptions {
    pub include_tool_calls: bool,
    pub include_tool_results: bool,
    pub include_reasoning: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            include_tool_calls: true,
            include_tool_results: true,
            include_reasoning: false,
        }
    }
}

pub fn export_session(
    session: &StoredSession,
    format: ExportFormat,
    options: &ExportOptions,
) -> Result<String, serde_json::Error> {
    match format {
        ExportFormat::Markdown => Ok(markdown(session, options)),
        ExportFormat::Prompts => Ok(role_export(session, &[Role::User], "Prompt")),
        ExportFormat::SystemPrompts => Ok(role_export(session, &[Role::System], "System prompt")),
        ExportFormat::ContextPrompts => {
            Ok(role_export(session, &[Role::System, Role::User], "Context"))
        }
        ExportFormat::Responses => Ok(role_export(session, &[Role::Assistant], "Response")),
        ExportFormat::ToolCalls => Ok(tool_export(session, options)),
        ExportFormat::Summary => Ok(summary(session)),
        ExportFormat::Json => serde_json::to_string_pretty(session),
        ExportFormat::Jsonl => jsonl(session, options),
    }
}

fn markdown(session: &StoredSession, options: &ExportOptions) -> String {
    let mut output = String::new();
    output.push_str("# ");
    output.push_str(&session.session.title);
    output.push_str("\n\n");
    output.push_str(&format!("- Provider: {}\n", session.session.provider));
    if let Some(model) = &session.session.model {
        output.push_str(&format!("- Model: {model}\n"));
    }
    if let Some(project) = &session.session.project_path {
        output.push_str(&format!("- Project: `{project}`\n"));
    }
    output.push('\n');

    for turn in &session.turns {
        if turn.role == Role::Reasoning && !options.include_reasoning {
            continue;
        }
        output.push_str("## ");
        output.push_str(match turn.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::System => "System",
            Role::Tool => "Tool",
            Role::Reasoning => "Reasoning",
            Role::Unknown => "Event",
        });
        output.push_str("\n\n");
        if !turn.text.trim().is_empty() {
            output.push_str(turn.text.trim());
            output.push_str("\n\n");
        }
        if options.include_tool_calls {
            for tool in &turn.tool_calls {
                output.push_str(&format!("### Tool · {}\n\n", tool.name));
                if let Some(arguments) = &tool.arguments_json {
                    output.push_str("```json\n");
                    output.push_str(arguments);
                    output.push_str("\n```\n\n");
                }
                if options.include_tool_results {
                    if let Some(result) = &tool.result_text {
                        output.push_str("<details><summary>Result</summary>\n\n```text\n");
                        output.push_str(result);
                        output.push_str("\n```\n\n</details>\n\n");
                    }
                }
            }
        }
    }
    output
}

fn role_export(session: &StoredSession, roles: &[Role], label: &str) -> String {
    session
        .turns
        .iter()
        .filter(|turn| roles.contains(&turn.role) && !turn.text.trim().is_empty())
        .enumerate()
        .map(|(index, turn)| {
            let role = if roles.len() > 1 {
                format!(" · {}", turn.role.as_str())
            } else {
                String::new()
            };
            format!("## {label} {}{role}\n\n{}", index + 1, turn.text.trim())
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn tool_export(session: &StoredSession, options: &ExportOptions) -> String {
    let mut output = Vec::new();
    for turn in &session.turns {
        for tool in &turn.tool_calls {
            let mut section = format!("## Tool · {}\n\n", tool.name);
            if let Some(arguments) = &tool.arguments_json {
                section.push_str("```json\n");
                section.push_str(arguments);
                section.push_str("\n```\n\n");
            }
            if options.include_tool_results {
                if let Some(result) = &tool.result_text {
                    section.push_str("### Result\n\n```text\n");
                    section.push_str(result);
                    section.push_str("\n```\n");
                }
            }
            output.push(section);
        }
    }
    output.join("\n")
}

fn summary(session: &StoredSession) -> String {
    if let Some(summary) = &session.summary {
        return summary.clone();
    }
    let prompts: Vec<_> = session
        .turns
        .iter()
        .filter(|turn| turn.role == Role::User && !turn.text.trim().is_empty())
        .collect();
    let tools: usize = session.turns.iter().map(|turn| turn.tool_calls.len()).sum();
    format!(
        "{}\n\nProvider: {} · {} prompts · {} tool calls · {} turns",
        session.session.title,
        session.session.provider,
        prompts.len(),
        tools,
        session.turns.len()
    )
}

fn jsonl(session: &StoredSession, options: &ExportOptions) -> Result<String, serde_json::Error> {
    let mut lines = Vec::new();
    for turn in &session.turns {
        if turn.role == Role::Reasoning && !options.include_reasoning {
            continue;
        }
        lines.push(serde_json::to_string(turn)?);
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SessionListItem, Turn};

    #[test]
    fn prompt_export_excludes_assistant_turns() {
        let session = StoredSession {
            session: SessionListItem {
                id: "1".into(),
                provider: "codex".into(),
                title: "Test".into(),
                project_path: None,
                source_path: None,
                created_at: None,
                updated_at: None,
                model: None,
                archived: false,
                turn_count: 2,
                tool_call_count: 0,
                total_tokens: None,
                source_turn_count: None,
            },
            summary: None,
            turns: vec![
                Turn {
                    external_id: None,
                    ordinal: 0,
                    prompt_ordinal: None,
                    role: Role::User,
                    created_at: None,
                    text: "Keep me".into(),
                    event_type: None,
                    model: None,
                    parent_external_id: None,
                    usage: None,
                    tool_calls: vec![],
                    metadata_json: None,
                },
                Turn {
                    external_id: None,
                    ordinal: 1,
                    prompt_ordinal: None,
                    role: Role::Assistant,
                    created_at: None,
                    text: "Drop me".into(),
                    event_type: None,
                    model: None,
                    parent_external_id: None,
                    usage: None,
                    tool_calls: vec![],
                    metadata_json: None,
                },
            ],
        };
        let exported =
            export_session(&session, ExportFormat::Prompts, &ExportOptions::default()).unwrap();
        assert!(exported.contains("Keep me"));
        assert!(!exported.contains("Drop me"));
    }

    #[test]
    fn context_export_keeps_system_and_user_only() {
        let mut session = StoredSession {
            session: SessionListItem {
                id: "1".into(),
                provider: "codex".into(),
                title: "Test".into(),
                project_path: None,
                source_path: None,
                created_at: None,
                updated_at: None,
                model: None,
                archived: false,
                turn_count: 3,
                tool_call_count: 0,
                total_tokens: None,
                source_turn_count: None,
            },
            summary: None,
            turns: Vec::new(),
        };
        for (ordinal, role, text) in [
            (0, Role::System, "System rules"),
            (1, Role::User, "User request"),
            (2, Role::Assistant, "Assistant answer"),
        ] {
            session.turns.push(Turn {
                external_id: None,
                ordinal,
                prompt_ordinal: None,
                role,
                created_at: None,
                text: text.into(),
                event_type: None,
                model: None,
                parent_external_id: None,
                usage: None,
                tool_calls: Vec::new(),
                metadata_json: None,
            });
        }
        let output = export_session(
            &session,
            ExportFormat::ContextPrompts,
            &ExportOptions::default(),
        )
        .unwrap();
        assert!(output.contains("System rules"));
        assert!(output.contains("User request"));
        assert!(!output.contains("Assistant answer"));
    }
}
