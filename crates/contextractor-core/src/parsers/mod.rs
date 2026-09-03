mod antigravity;
mod claude;
mod codex;
mod grok;
mod helpers;

use crate::model::{ParsedSession, SourceCandidate, SourceKind};
use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("could not read {path}: {source}")]
    Io { path: String, source: io::Error },
    #[error("invalid JSON in {path}: {message}")]
    Json { path: String, message: String },
    #[error("unsupported source format: {0:?}")]
    Unsupported(SourceKind),
    #[error("source does not contain a session identifier: {0}")]
    MissingSessionId(String),
}

pub fn parse(candidate: &SourceCandidate) -> Result<ParsedSession, ParseError> {
    match candidate.kind {
        SourceKind::CodexRollout => codex::parse(candidate),
        SourceKind::ClaudeCodeProject
        | SourceKind::ClaudeDesktopMetadata
        | SourceKind::ClaudeLocalAudit => claude::parse(candidate),
        SourceKind::GrokCliHistory => grok::parse(candidate),
        SourceKind::AntigravityTranscript => antigravity::parse(candidate),
    }
}
