use crate::db::Archive;
use crate::discovery::{discover, DiscoveryOptions};
use crate::model::{ImportProgress, ImportReport, SourceCandidate, SourceKind};
use crate::parsers;
use rusqlite::OpenFlags;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};

#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    pub discovery: DiscoveryOptions,
    pub force: bool,
}

pub fn import_all(
    archive: &mut Archive,
    options: &ImportOptions,
) -> Result<ImportReport, io::Error> {
    import_all_with_progress(archive, options, |_| {})
}

pub fn import_all_with_progress<F>(
    archive: &mut Archive,
    options: &ImportOptions,
    mut progress: F,
) -> Result<ImportReport, io::Error>
where
    F: FnMut(&ImportProgress),
{
    let discovery = discover(&options.discovery);
    let codex_root = discovery
        .providers
        .iter()
        .find(|provider| provider.provider.as_str() == "codex")
        .and_then(|provider| provider.roots.first())
        .and_then(|root| root.parent());
    let codex_state = codex_root.map(|root| root.join("state_5.sqlite"));
    let codex_index = codex_root.map(|root| root.join("session_index.jsonl"));
    let mut candidates: Vec<SourceCandidate> = discovery
        .providers
        .iter()
        .flat_map(|provider| provider.candidates.iter().cloned())
        .collect();
    candidates.sort_by_key(|candidate| match candidate.kind {
        SourceKind::ClaudeDesktopMetadata => 0,
        SourceKind::ClaudeLocalAudit => 1,
        _ => 2,
    });

    let mut report = ImportReport {
        discovered: candidates.len(),
        warnings: discovery
            .providers
            .into_iter()
            .flat_map(|provider| provider.warnings)
            .collect(),
        ..ImportReport::default()
    };

    let total = candidates.len();
    for (index, candidate) in candidates.into_iter().enumerate() {
        progress(&ImportProgress {
            processed: index,
            total,
            provider: candidate.provider.as_str().to_string(),
            source_name: candidate
                .path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("session")
                .to_string(),
            status: "reading".to_string(),
        });
        let fingerprint = match fingerprint(&candidate) {
            Ok(value) => value,
            Err(error) => {
                report.failed += 1;
                report.warnings.push(format!(
                    "Could not fingerprint {}: {error}",
                    candidate.path.display()
                ));
                progress(&ImportProgress {
                    processed: index + 1,
                    total,
                    provider: candidate.provider.as_str().to_string(),
                    source_name: candidate.path.display().to_string(),
                    status: "error".to_string(),
                });
                continue;
            }
        };
        if !options.force
            && archive
                .source_is_current(&candidate.path, &fingerprint)
                .unwrap_or(false)
        {
            report.unchanged += 1;
            progress(&ImportProgress {
                processed: index + 1,
                total,
                provider: candidate.provider.as_str().to_string(),
                source_name: candidate
                    .path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("session")
                    .to_string(),
                status: "unchanged".to_string(),
            });
            continue;
        }

        match parsers::parse(&candidate) {
            Ok(parsed) => match archive.import_session(
                &parsed,
                &fingerprint,
                candidate.size_bytes,
                candidate.modified_at_ms,
            ) {
                Ok(_) => report.imported += 1,
                Err(error) => {
                    report.failed += 1;
                    report.warnings.push(format!(
                        "Database import failed for {}: {error}",
                        candidate.path.display()
                    ));
                }
            },
            Err(error) => {
                report.failed += 1;
                let message = error.to_string();
                let _ = archive.record_import_failure(
                    &candidate.path,
                    candidate.provider.as_str(),
                    candidate.kind.as_str(),
                    &fingerprint,
                    candidate.size_bytes,
                    candidate.modified_at_ms,
                    &message,
                );
                report.warnings.push(message);
            }
        }
        progress(&ImportProgress {
            processed: index + 1,
            total,
            provider: candidate.provider.as_str().to_string(),
            source_name: candidate
                .path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("session")
                .to_string(),
            status: "complete".to_string(),
        });
    }
    if let Some(index_path) = codex_index.filter(|path| path.is_file()) {
        match read_codex_titles(&index_path, codex_state.as_deref()).and_then(|titles| {
            archive
                .update_provider_titles("codex", &titles)
                .map_err(io::Error::other)
        }) {
            Ok(_) => {}
            Err(error) => report.warnings.push(format!(
                "Codex task names could not be synchronized: {error}"
            )),
        }
    }
    Ok(report)
}

fn read_codex_titles(
    index_path: &std::path::Path,
    state_path: Option<&std::path::Path>,
) -> io::Result<Vec<(String, String)>> {
    let mut titles = HashMap::new();
    for line in BufReader::new(File::open(index_path)?)
        .lines()
        .map_while(Result::ok)
    {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(title) = value.get("thread_name").and_then(Value::as_str) else {
            continue;
        };
        if !title.trim().is_empty() {
            titles.insert(id.to_string(), title.trim().to_string());
        }
    }
    let Some(path) = state_path.filter(|path| path.is_file()) else {
        return Ok(titles.into_iter().collect());
    };
    let connection = rusqlite::Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(io::Error::other)?;
    let mut statement = connection
        .prepare("SELECT id, name FROM threads WHERE name IS NOT NULL AND name<>''")
        .map_err(io::Error::other)?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(io::Error::other)?;
    for row in rows {
        let (id, name) = row.map_err(io::Error::other)?;
        titles.insert(id, name);
    }
    Ok(titles.into_iter().collect())
}

fn fingerprint(candidate: &SourceCandidate) -> io::Result<String> {
    let mut file = File::open(&candidate.path)?;
    let mut hasher = Sha256::new();
    hasher.update(candidate.kind.as_str().as_bytes());
    hasher.update(candidate.size_bytes.to_le_bytes());
    if let Some(modified) = candidate.modified_at_ms {
        hasher.update(modified.to_le_bytes());
    }
    let mut buffer = [0_u8; 64 * 1024];
    let head = file.read(&mut buffer)?;
    hasher.update(&buffer[..head]);
    if candidate.size_bytes > buffer.len() as u64 {
        file.seek(SeekFrom::End(-(buffer.len() as i64)))?;
        let tail = file.read(&mut buffer)?;
        hasher.update(&buffer[..tail]);
    }
    if candidate.kind == SourceKind::GrokCliHistory {
        if let Some(directory) = candidate.path.parent() {
            hash_adjacent_file(&directory.join("summary.json"), &mut hasher)?;
            let recap_directory = directory.join("recap_requests");
            if let Ok(entries) = std::fs::read_dir(recap_directory) {
                let mut entries = entries
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        entry.path().extension().and_then(|value| value.to_str()) == Some("json")
                    })
                    .collect::<Vec<_>>();
                entries.sort_by_key(|entry| entry.file_name());
                for entry in entries {
                    let metadata = entry.metadata()?;
                    hasher.update(entry.file_name().to_string_lossy().as_bytes());
                    hasher.update(metadata.len().to_le_bytes());
                    if let Ok(modified) = metadata.modified().and_then(|value| {
                        value
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_err(io::Error::other)
                    }) {
                        hasher.update(modified.as_millis().to_le_bytes());
                    }
                }
            }
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_adjacent_file(path: &std::path::Path, hasher: &mut Sha256) -> io::Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let metadata = path.metadata()?;
    hasher.update(path.as_os_str().to_string_lossy().as_bytes());
    hasher.update(metadata.len().to_le_bytes());
    if let Ok(modified) = metadata.modified().and_then(|value| {
        value
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)
    }) {
        hasher.update(modified.as_millis().to_le_bytes());
    }
    let mut file = File::open(path)?;
    let mut buffer = [0_u8; 64 * 1024];
    let head = file.read(&mut buffer)?;
    hasher.update(&buffer[..head]);
    if metadata.len() > buffer.len() as u64 {
        file.seek(SeekFrom::End(-(buffer.len() as i64)))?;
        let tail = file.read(&mut buffer)?;
        hasher.update(&buffer[..tail]);
    }
    Ok(())
}
