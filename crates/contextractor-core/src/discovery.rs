use crate::model::{DiscoveryReport, Provider, ProviderDiscovery, SourceCandidate, SourceKind};
use directories::BaseDirs;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

#[derive(Debug, Clone, Default)]
pub struct DiscoveryOptions {
    pub home_dir: Option<PathBuf>,
    pub roaming_dir: Option<PathBuf>,
    pub include_desktop_metadata: bool,
}

pub fn discover(options: &DiscoveryOptions) -> DiscoveryReport {
    let home = options
        .home_dir
        .clone()
        .or_else(|| BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let roaming = options.roaming_dir.clone().or_else(default_roaming_dir);

    let mut providers = Vec::new();
    let codex_root = provider_root(&home, options.home_dir.is_some(), "CODEX_HOME", ".codex");
    let claude_root = provider_root(
        &home,
        options.home_dir.is_some(),
        "CLAUDE_CONFIG_DIR",
        ".claude",
    );
    let grok_root = provider_root(&home, options.home_dir.is_some(), "GROK_HOME", ".grok");
    let gemini_root = provider_root(&home, options.home_dir.is_some(), "GEMINI_HOME", ".gemini");

    providers.push(discover_codex(&codex_root));
    providers.push(discover_claude(
        &claude_root,
        roaming.as_deref(),
        options.include_desktop_metadata,
    ));
    providers.push(discover_grok(&grok_root));
    providers.push(discover_antigravity(&gemini_root));

    DiscoveryReport { providers }
}

fn provider_root(home: &Path, explicit_home: bool, variable: &str, suffix: &str) -> PathBuf {
    if !explicit_home {
        if let Some(path) = env::var_os(variable).filter(|value| !value.is_empty()) {
            return PathBuf::from(path);
        }
    }
    home.join(suffix)
}

fn default_roaming_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        BaseDirs::new().map(|dirs| dirs.home_dir().join("Library/Application Support"))
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| BaseDirs::new().map(|dirs| dirs.home_dir().join(".config")))
    }
}

fn discover_codex(home: &Path) -> ProviderDiscovery {
    let active = home.join("sessions");
    let archived = home.join("archived_sessions");
    let mut roots = Vec::new();
    let mut candidates = Vec::new();

    if active.is_dir() {
        roots.push(active.clone());
        collect_named_jsonl(
            &active,
            |name| name.starts_with("rollout-") && name.ends_with(".jsonl"),
            Provider::Codex,
            SourceKind::CodexRollout,
            false,
            &mut candidates,
        );
    }
    if archived.is_dir() {
        roots.push(archived.clone());
        collect_named_jsonl(
            &archived,
            |name| name.starts_with("rollout-") && name.ends_with(".jsonl"),
            Provider::Codex,
            SourceKind::CodexRollout,
            true,
            &mut candidates,
        );
    }

    ProviderDiscovery {
        provider: Provider::Codex,
        installed: home.exists(),
        roots,
        candidates,
        warnings: Vec::new(),
    }
}

fn discover_claude(
    home: &Path,
    roaming: Option<&Path>,
    include_desktop_metadata: bool,
) -> ProviderDiscovery {
    let projects = home.join("projects");
    let mut roots = Vec::new();
    let mut candidates = Vec::new();
    let mut warnings = Vec::new();

    if projects.is_dir() {
        roots.push(projects.clone());
        for entry in WalkDir::new(&projects)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let path = entry.path();
            let is_jsonl = path.extension().and_then(|value| value.to_str()) == Some("jsonl");
            let is_subagent = path
                .components()
                .any(|component| component.as_os_str() == "subagents");
            if is_jsonl && !is_subagent {
                if let Some(candidate) =
                    candidate(Provider::Claude, SourceKind::ClaudeCodeProject, path, false)
                {
                    candidates.push(candidate);
                }
            }
        }
    }

    if include_desktop_metadata {
        if let Some(roaming) = roaming {
            let desktop = roaming.join("Claude/claude-code-sessions");
            if desktop.is_dir() {
                roots.push(desktop.clone());
                for entry in WalkDir::new(&desktop)
                    .max_depth(5)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_type().is_file())
                {
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy();
                    if name.starts_with("local_") && name.ends_with(".json") {
                        if let Some(candidate) = candidate(
                            Provider::Claude,
                            SourceKind::ClaudeDesktopMetadata,
                            path,
                            false,
                        ) {
                            candidates.push(candidate);
                        }
                    }
                }
                warnings.push(
                    "Claude Desktop records are metadata bridges; cloud-only chats require an official export."
                        .to_string(),
                );
            }
            let local_agent_sessions = roaming.join("Claude/local-agent-mode-sessions");
            if local_agent_sessions.is_dir() {
                roots.push(local_agent_sessions.clone());
                for entry in WalkDir::new(&local_agent_sessions)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        entry.file_type().is_file() && entry.file_name() == "audit.jsonl"
                    })
                {
                    if let Some(candidate) = candidate(
                        Provider::Claude,
                        SourceKind::ClaudeLocalAudit,
                        entry.path(),
                        false,
                    ) {
                        candidates.push(candidate);
                    }
                }
            }
        }
    }

    deduplicate_candidates(&mut candidates);
    ProviderDiscovery {
        provider: Provider::Claude,
        installed: home.exists()
            || roaming
                .map(|path| path.join("Claude").exists())
                .unwrap_or(false),
        roots,
        candidates,
        warnings,
    }
}

fn discover_grok(home: &Path) -> ProviderDiscovery {
    let sessions = home.join("sessions");
    let mut candidates = Vec::new();
    if sessions.is_dir() {
        collect_named_jsonl(
            &sessions,
            |name| name == "chat_history.jsonl",
            Provider::Grok,
            SourceKind::GrokCliHistory,
            false,
            &mut candidates,
        );
    }
    ProviderDiscovery {
        provider: Provider::Grok,
        installed: home.exists(),
        roots: sessions.is_dir().then_some(sessions).into_iter().collect(),
        candidates,
        warnings: vec![
            "Consumer Grok chats are cloud-synced and are not inferred from browser credentials."
                .to_string(),
        ],
    }
}

fn discover_antigravity(home: &Path) -> ProviderDiscovery {
    let brains = [
        home.join("antigravity").join("brain"),
        home.join("antigravity-cli").join("brain"),
    ];
    let mut roots = Vec::new();
    let mut candidates = Vec::new();
    for brain in brains {
        if brain.is_dir() {
            roots.push(brain.clone());
            collect_named_jsonl(
                &brain,
                |name| name == "transcript_full.jsonl",
                Provider::Antigravity,
                SourceKind::AntigravityTranscript,
                false,
                &mut candidates,
            );
        }
    }
    deduplicate_candidates(&mut candidates);
    ProviderDiscovery {
        provider: Provider::Antigravity,
        installed: home.join("antigravity").exists() || home.join("antigravity-cli").exists(),
        roots,
        candidates,
        warnings: Vec::new(),
    }
}

fn collect_named_jsonl<F>(
    root: &Path,
    predicate: F,
    provider: Provider,
    kind: SourceKind,
    archived: bool,
    output: &mut Vec<SourceCandidate>,
) where
    F: Fn(&str) -> bool,
{
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let name = entry.file_name().to_string_lossy();
        if predicate(&name) {
            if let Some(item) = candidate(provider, kind, entry.path(), archived) {
                output.push(item);
            }
        }
    }
}

fn candidate(
    provider: Provider,
    kind: SourceKind,
    path: &Path,
    archived: bool,
) -> Option<SourceCandidate> {
    let metadata = fs::metadata(path).ok()?;
    let modified_at_ms = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_millis() as i64);
    Some(SourceCandidate {
        provider,
        kind,
        path: path.to_path_buf(),
        archived,
        modified_at_ms,
        size_bytes: metadata.len(),
    })
}

fn deduplicate_candidates(candidates: &mut Vec<SourceCandidate>) {
    let mut seen = BTreeSet::new();
    candidates.retain(|candidate| seen.insert(candidate.path.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discovers_supported_sources_without_following_unknown_files() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let codex = home.join(".codex/sessions/2026/09/02");
        let claude = home.join(".claude/projects/demo");
        let grok = home.join(".grok/sessions/demo/session");
        let agy = home.join(".gemini/antigravity/brain/demo/.system_generated/logs");
        let agy_cli = home.join(".gemini/antigravity-cli/brain/cli-demo/.system_generated/logs");
        fs::create_dir_all(&codex).unwrap();
        fs::create_dir_all(&claude).unwrap();
        fs::create_dir_all(&grok).unwrap();
        fs::create_dir_all(&agy).unwrap();
        fs::create_dir_all(&agy_cli).unwrap();
        fs::write(codex.join("rollout-test.jsonl"), "{}\n").unwrap();
        fs::write(claude.join("session.jsonl"), "{}\n").unwrap();
        fs::write(grok.join("chat_history.jsonl"), "{}\n").unwrap();
        fs::write(agy.join("transcript_full.jsonl"), "{}\n").unwrap();
        fs::write(agy_cli.join("transcript_full.jsonl"), "{}\n").unwrap();

        let report = discover(&DiscoveryOptions {
            home_dir: Some(home.to_path_buf()),
            roaming_dir: None,
            include_desktop_metadata: false,
        });
        assert_eq!(report.total_candidates(), 5);
        let antigravity = report
            .providers
            .iter()
            .find(|provider| provider.provider == Provider::Antigravity)
            .unwrap();
        assert_eq!(antigravity.roots.len(), 2);
        assert_eq!(antigravity.candidates.len(), 2);
    }
}
