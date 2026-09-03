use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Codex,
    Claude,
    Grok,
    Antigravity,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Grok => "grok",
            Self::Antigravity => "antigravity",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    CodexRollout,
    ClaudeCodeProject,
    ClaudeDesktopMetadata,
    ClaudeLocalAudit,
    GrokCliHistory,
    AntigravityTranscript,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CodexRollout => "codex_rollout",
            Self::ClaudeCodeProject => "claude_code_project",
            Self::ClaudeDesktopMetadata => "claude_desktop_metadata",
            Self::ClaudeLocalAudit => "claude_local_audit",
            Self::GrokCliHistory => "grok_cli_history",
            Self::AntigravityTranscript => "antigravity_transcript",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceCandidate {
    pub provider: Provider,
    pub kind: SourceKind,
    pub path: PathBuf,
    pub archived: bool,
    pub modified_at_ms: Option<i64>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
    Reasoning,
    Unknown,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Tool => "tool",
            Self::Reasoning => "reasoning",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageConfidence {
    Observed,
    Reconstructed,
    Estimated,
}

impl UsageConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Reconstructed => "reconstructed",
            Self::Estimated => "estimated",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub cache_write_input_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub confidence: Option<UsageConfidence>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub external_id: Option<String>,
    pub name: String,
    pub arguments_json: Option<String>,
    pub result_text: Option<String>,
    pub status: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub external_id: Option<String>,
    pub ordinal: i64,
    /// One-based user interaction number. Tool and assistant records inherit the
    /// most recent prompt number so provider event streams do not inflate turns.
    pub prompt_ordinal: Option<i64>,
    pub role: Role,
    pub created_at: Option<String>,
    pub text: String,
    pub event_type: Option<String>,
    pub model: Option<String>,
    pub parent_external_id: Option<String>,
    pub usage: Option<TokenUsage>,
    pub tool_calls: Vec<ToolCall>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedSession {
    pub provider: Provider,
    pub source_kind: SourceKind,
    pub external_id: String,
    pub title: Option<String>,
    pub project_path: Option<String>,
    pub source_path: PathBuf,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub model: Option<String>,
    pub archived: bool,
    pub summary: Option<String>,
    pub turns: Vec<Turn>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDiscovery {
    pub provider: Provider,
    pub installed: bool,
    pub roots: Vec<PathBuf>,
    pub candidates: Vec<SourceCandidate>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveryReport {
    pub providers: Vec<ProviderDiscovery>,
}

impl DiscoveryReport {
    pub fn total_candidates(&self) -> usize {
        self.providers.iter().map(|p| p.candidates.len()).sum()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportReport {
    pub discovered: usize,
    pub imported: usize,
    pub unchanged: usize,
    pub failed: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportProgress {
    pub processed: usize,
    pub total: usize,
    pub provider: String,
    pub source_name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionListItem {
    pub id: String,
    pub provider: String,
    pub title: String,
    pub project_path: Option<String>,
    pub source_path: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub model: Option<String>,
    pub archived: bool,
    pub turn_count: i64,
    pub tool_call_count: i64,
    pub total_tokens: Option<i64>,
    pub source_turn_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReference {
    pub path: String,
    pub exists: bool,
    pub is_image: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub session: SessionListItem,
    pub summary: Option<String>,
    pub turns: Vec<Turn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnPage {
    pub turns: Vec<Turn>,
    pub offset: usize,
    pub total: usize,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub provider: String,
    pub sessions: i64,
    pub prompts: i64,
    pub assistant_turns: i64,
    pub tool_calls: i64,
    pub total_tokens: i64,
    pub active_days: i64,
    pub average_prompts_per_session: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyUsage {
    pub date: String,
    pub provider: String,
    pub sessions: i64,
    pub prompts: i64,
    pub assistant_turns: i64,
    pub tool_calls: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUsage {
    pub provider: String,
    pub name: String,
    pub calls: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub provider: String,
    pub model: String,
    pub sessions: i64,
    pub prompts: i64,
    pub assistant_turns: i64,
    pub tool_calls: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageAnalytics {
    pub providers: Vec<ProviderUsage>,
    pub days: Vec<DailyUsage>,
    pub top_tools: Vec<ToolUsage>,
    pub models: Vec<ModelUsage>,
    pub busiest_day: Option<DailyUsage>,
    pub total_sessions: i64,
    pub total_prompts: i64,
    pub total_assistant_turns: i64,
    pub total_tool_calls: i64,
    pub total_tokens: i64,
}
