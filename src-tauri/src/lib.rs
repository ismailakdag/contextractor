use contextractor_core::{
    discover, estimate_usage_cost, export_session, service::import_all_with_progress, Archive,
    CostEstimate, DiscoveryOptions, DiscoveryReport, ExportFormat, ExportOptions, FileReference,
    ImportOptions, ImportReport, SessionListItem, StoredSession, ToolCall, TurnPage,
    UsageAnalytics,
};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager, State};

struct AppState {
    database_path: PathBuf,
    portable: bool,
}

#[derive(Serialize)]
struct AppInfo {
    database_path: String,
    portable: bool,
    version: &'static str,
}

#[derive(Serialize)]
struct SessionDetail {
    data: StoredSession,
    cost: CostEstimate,
    total_turns: usize,
    has_more: bool,
}

#[derive(Serialize)]
struct UsageCostRow {
    session_id: String,
    provider: String,
    cost: CostEstimate,
}

#[derive(Serialize)]
struct FileCollectionReport {
    destination: String,
    report_path: String,
    selected_references: usize,
    copied_files: usize,
    copied_bytes: u64,
    missing: usize,
    skipped: usize,
    duplicates: usize,
}

#[derive(Serialize)]
struct FileCollectionEntry {
    source: String,
    destination: Option<String>,
    status: String,
    reason: Option<String>,
    origins: Vec<String>,
}

fn error_string(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        database_path: state.database_path.display().to_string(),
        portable: state.portable,
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[tauri::command]
fn discover_sources() -> DiscoveryReport {
    discover(&DiscoveryOptions {
        include_desktop_metadata: true,
        ..DiscoveryOptions::default()
    })
}

#[tauri::command]
async fn scan_sources(
    window: tauri::Window,
    state: State<'_, AppState>,
) -> Result<ImportReport, String> {
    let database_path = state.database_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut archive = Archive::open(&database_path).map_err(error_string)?;
        import_all_with_progress(
            &mut archive,
            &ImportOptions {
                discovery: DiscoveryOptions {
                    include_desktop_metadata: true,
                    ..DiscoveryOptions::default()
                },
                force: false,
            },
            |progress| {
                let _ = window.emit("scan-progress", progress);
            },
        )
        .map_err(error_string)
    })
    .await
    .map_err(error_string)?
}

#[tauri::command]
async fn list_sessions(
    provider: Option<String>,
    search: Option<String>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<SessionListItem>, String> {
    let database_path = state.database_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Archive::open_existing(&database_path)
            .map_err(error_string)?
            .list_sessions(provider.as_deref(), search.as_deref(), limit.unwrap_or(300))
            .map_err(error_string)
    })
    .await
    .map_err(error_string)?
}

#[tauri::command]
async fn get_session(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<SessionDetail>, String> {
    let database_path = state.database_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let archive = Archive::open_existing(&database_path).map_err(error_string)?;
        let header = archive.get_session_header(&id).map_err(error_string)?;
        header
            .map(|(session, summary)| {
                let usage = archive.session_usage_estimate(&id).map_err(error_string)?;
                let cost = estimate_usage_cost(&session.provider, session.model.clone(), usage);
                let page = archive
                    .load_turn_page(&id, "conversation", 0, 120, None)
                    .map_err(error_string)?;
                let data = StoredSession {
                    session,
                    summary,
                    turns: page.turns,
                };
                Ok(SessionDetail {
                    data,
                    cost,
                    total_turns: page.total,
                    has_more: page.has_more,
                })
            })
            .transpose()
    })
    .await
    .map_err(error_string)?
}

#[tauri::command]
async fn get_tool_call(
    id: String,
    turn_ordinal: i64,
    tool_ordinal: i64,
    state: State<'_, AppState>,
) -> Result<Option<ToolCall>, String> {
    let database_path = state.database_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Archive::open_existing(&database_path)
            .map_err(error_string)?
            .load_tool_call(&id, turn_ordinal, tool_ordinal)
            .map_err(error_string)
    })
    .await
    .map_err(error_string)?
}

#[tauri::command]
async fn get_session_files(
    id: String,
    state: State<'_, AppState>,
) -> Result<Vec<FileReference>, String> {
    let database_path = state.database_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Archive::open_existing(&database_path)
            .map_err(error_string)?
            .session_file_references(&id)
            .map_err(error_string)
    })
    .await
    .map_err(error_string)?
}

#[tauri::command]
async fn collect_session_files(
    id: String,
    destination: String,
    origin_filter: String,
    state: State<'_, AppState>,
) -> Result<FileCollectionReport, String> {
    let database_path = state.database_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let archive = Archive::open_existing(&database_path).map_err(error_string)?;
        let (session, _) = archive
            .get_session_header(&id)
            .map_err(error_string)?
            .ok_or_else(|| "Oturum bulunamadı".to_string())?;
        if !matches!(origin_filter.as_str(), "user" | "assistant" | "all") {
            return Err("Geçersiz dosya paketi kaynağı".into());
        }
        let references = archive
            .session_export_file_references(&id)
            .map_err(error_string)?;
        let references = references
            .into_iter()
            .filter(|reference| reference_matches_origin(&reference.origins, &origin_filter))
            .collect::<Vec<_>>();
        let selected_references = references.len();
        let base = PathBuf::from(destination);
        if !base.is_dir() {
            return Err("Seçilen hedef klasör artık mevcut değil".into());
        }
        let package = unique_collection_path(&base, &session.title);
        fs::create_dir_all(&package).map_err(error_string)?;

        let mut entries = Vec::new();
        let mut counters = CollectionCounters::default();
        for reference in references {
            let source = normalize_requested_path(&reference.path)
                .unwrap_or_else(|_| PathBuf::from(&reference.path));
            if !source.exists() {
                let missing_key = collection_path_key(&source);
                if !counters.seen_missing.insert(missing_key) {
                    counters.duplicates += 1;
                    continue;
                }
                counters.missing += 1;
                entries.push(FileCollectionEntry {
                    source: reference.path,
                    destination: None,
                    status: "missing".into(),
                    reason: Some("Dosya veya klasör bulunamadı".into()),
                    origins: reference.origins,
                });
                continue;
            }
            let target = package
                .join("references")
                .join(portable_source_path(&source));
            if source.is_dir() {
                copy_tree(
                    &source,
                    &target,
                    &package,
                    &mut counters,
                    &mut entries,
                    reference.origins,
                );
            } else {
                copy_one(
                    &source,
                    &target,
                    &mut counters,
                    &mut entries,
                    reference.origins,
                );
            }
        }

        let report_path = package.join("contextractor-file-report.json");
        let report = serde_json::json!({
            "session": { "id": session.id, "title": session.title, "provider": session.provider },
            "include_workspace": false,
            "origin_filter": origin_filter,
            "selected_references": selected_references,
            "copied_files": counters.files,
            "copied_bytes": counters.bytes,
            "missing": counters.missing,
            "skipped": counters.skipped,
            "duplicates": counters.duplicates,
            "entries": entries,
        });
        fs::write(
            &report_path,
            serde_json::to_vec_pretty(&report).map_err(error_string)?,
        )
        .map_err(error_string)?;
        Ok(FileCollectionReport {
            destination: package.display().to_string(),
            report_path: report_path.display().to_string(),
            selected_references,
            copied_files: counters.files,
            copied_bytes: counters.bytes,
            missing: counters.missing,
            skipped: counters.skipped,
            duplicates: counters.duplicates,
        })
    })
    .await
    .map_err(error_string)?
}

#[derive(Default)]
struct CollectionCounters {
    files: usize,
    bytes: u64,
    missing: usize,
    skipped: usize,
    duplicates: usize,
    seen_files: std::collections::HashMap<String, String>,
    seen_directories: std::collections::HashMap<String, String>,
    seen_missing: std::collections::HashSet<String>,
}

const COLLECTION_FILE_LIMIT: usize = 50_000;
const COLLECTION_BYTE_LIMIT: u64 = 4 * 1024 * 1024 * 1024;

fn copy_tree(
    source: &Path,
    target: &Path,
    package: &Path,
    counters: &mut CollectionCounters,
    entries: &mut Vec<FileCollectionEntry>,
    origins: Vec<String>,
) {
    let source_key = collection_path_key(source);
    if let Some(previous) = counters.seen_directories.get(&source_key).cloned() {
        counters.duplicates += 1;
        entries.push(collection_duplicate(source, &previous, origins));
        return;
    }
    counters
        .seen_directories
        .insert(source_key, target.display().to_string());
    if counters.files >= COLLECTION_FILE_LIMIT || counters.bytes >= COLLECTION_BYTE_LIMIT {
        counters.skipped += 1;
        entries.push(collection_skip(
            source,
            "Paket güvenlik sınırına ulaştı",
            origins,
        ));
        return;
    }
    let Ok(children) = fs::read_dir(source) else {
        counters.skipped += 1;
        entries.push(collection_skip(source, "Klasör okunamadı", origins));
        return;
    };
    for child in children.flatten() {
        let path = child.path();
        if path.starts_with(package) {
            continue;
        }
        let next_target = target.join(child.file_name());
        match child.file_type() {
            Ok(kind) if kind.is_symlink() => {
                counters.skipped += 1;
                entries.push(collection_skip(
                    &path,
                    "Sembolik bağlantı takip edilmedi",
                    origins.clone(),
                ));
            }
            Ok(kind) if kind.is_dir() => {
                copy_tree(
                    &path,
                    &next_target,
                    package,
                    counters,
                    entries,
                    origins.clone(),
                );
            }
            Ok(kind) if kind.is_file() => {
                copy_one(&path, &next_target, counters, entries, origins.clone());
            }
            _ => {
                counters.skipped += 1;
                entries.push(collection_skip(
                    &path,
                    "Desteklenmeyen dosya türü",
                    origins.clone(),
                ));
            }
        }
        if counters.files >= COLLECTION_FILE_LIMIT || counters.bytes >= COLLECTION_BYTE_LIMIT {
            break;
        }
    }
}

fn copy_one(
    source: &Path,
    target: &Path,
    counters: &mut CollectionCounters,
    entries: &mut Vec<FileCollectionEntry>,
    origins: Vec<String>,
) {
    let source_key = collection_path_key(source);
    if let Some(previous) = counters.seen_files.get(&source_key).cloned() {
        counters.duplicates += 1;
        entries.push(collection_duplicate(source, &previous, origins));
        return;
    }
    if counters.files >= COLLECTION_FILE_LIMIT || counters.bytes >= COLLECTION_BYTE_LIMIT {
        counters.skipped += 1;
        entries.push(collection_skip(
            source,
            "Paket güvenlik sınırına ulaştı",
            origins,
        ));
        return;
    }
    let size = source
        .metadata()
        .map(|value| value.len())
        .unwrap_or_default();
    if counters.bytes.saturating_add(size) > COLLECTION_BYTE_LIMIT {
        counters.skipped += 1;
        entries.push(collection_skip(
            source,
            "4 GB paket sınırını aşardı",
            origins,
        ));
        return;
    }
    let result = target
        .parent()
        .ok_or_else(|| std::io::Error::other("Hedef klasör yok"))
        .and_then(fs::create_dir_all)
        .and_then(|_| fs::copy(source, target));
    match result {
        Ok(bytes) => {
            counters
                .seen_files
                .insert(source_key, target.display().to_string());
            counters.files += 1;
            counters.bytes += bytes;
            entries.push(FileCollectionEntry {
                source: source.display().to_string(),
                destination: Some(target.display().to_string()),
                status: "copied".into(),
                reason: None,
                origins,
            });
        }
        Err(error) => {
            counters.skipped += 1;
            entries.push(collection_skip(source, &error.to_string(), origins));
        }
    }
}

fn collection_duplicate(
    source: &Path,
    previous_destination: &str,
    origins: Vec<String>,
) -> FileCollectionEntry {
    FileCollectionEntry {
        source: source.display().to_string(),
        destination: Some(previous_destination.to_string()),
        status: "deduplicated".into(),
        reason: Some("Aynı fiziksel kaynak pakette zaten bulunuyor".into()),
        origins,
    }
}

fn reference_matches_origin(origins: &[String], filter: &str) -> bool {
    match filter {
        "user" => origins.iter().any(|origin| origin == "user"),
        "assistant" => origins.iter().any(|origin| origin == "assistant"),
        "all" => origins
            .iter()
            .any(|origin| origin == "user" || origin == "assistant"),
        _ => false,
    }
}

fn collection_path_key(source: &Path) -> String {
    let value = fs::canonicalize(source)
        .unwrap_or_else(|_| source.to_path_buf())
        .to_string_lossy()
        .to_string();
    #[cfg(target_os = "windows")]
    return value.replace('/', "\\").to_ascii_lowercase();
    #[cfg(not(target_os = "windows"))]
    value
}

fn collection_skip(source: &Path, reason: &str, origins: Vec<String>) -> FileCollectionEntry {
    FileCollectionEntry {
        source: source.display().to_string(),
        destination: None,
        status: "skipped".into(),
        reason: Some(reason.into()),
        origins,
    }
}

fn unique_collection_path(base: &Path, title: &str) -> PathBuf {
    let safe = title
        .chars()
        .map(|value| {
            if value.is_alphanumeric() || matches!(value, '-' | '_') {
                value
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(56)
        .collect::<String>();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let stem = format!(
        "{}-{stamp}",
        if safe.is_empty() { "session" } else { &safe }
    );
    let mut candidate = base.join(&stem);
    let mut suffix = 2;
    while candidate.exists() {
        candidate = base.join(format!("{stem}-{suffix}"));
        suffix += 1;
    }
    candidate
}

fn portable_source_path(source: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in source.components() {
        match component {
            std::path::Component::Prefix(prefix) => {
                output.push(prefix.as_os_str().to_string_lossy().replace(':', ""));
            }
            std::path::Component::Normal(value) => output.push(value),
            std::path::Component::ParentDir => output.push("_parent"),
            _ => {}
        }
    }
    output
}

#[tauri::command]
async fn usage_costs(
    provider: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<UsageCostRow>, String> {
    let database_path = state.database_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let archive = Archive::open_existing(&database_path).map_err(error_string)?;
        let sessions = archive
            .list_sessions(provider.as_deref(), None, 500)
            .map_err(error_string)?;
        sessions
            .into_iter()
            .map(|session| {
                let usage = archive
                    .session_usage_estimate(&session.id)
                    .map_err(error_string)?;
                Ok(UsageCostRow {
                    session_id: session.id,
                    provider: session.provider.clone(),
                    cost: estimate_usage_cost(&session.provider, session.model, usage),
                })
            })
            .collect()
    })
    .await
    .map_err(error_string)?
}

#[tauri::command]
fn reveal_path(path: String) -> Result<String, String> {
    let requested = normalize_requested_path(&path)?;
    let missing = !requested.exists();
    let existing = if missing {
        meaningful_existing_ancestor(&requested).ok_or_else(|| {
            format!(
                "Dosya bulunamadı ve güvenle açılabilecek yakın bir klasörü yok; Belgeler'e yönlendirilmedi: {path}"
            )
        })?
    } else {
        requested.clone()
    };

    #[cfg(target_os = "windows")]
    {
        if !missing && requested.is_file() {
            use std::os::windows::process::CommandExt;
            std::process::Command::new("explorer.exe")
                .raw_arg(windows_select_argument(&requested))
                .spawn()
                .map_err(error_string)?;
        } else {
            shell_explore_directory(&existing)?;
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("open");
        if !missing && requested.is_file() {
            command.arg("-R").arg(&requested);
        } else {
            command.arg(&existing);
        }
        command.spawn().map_err(error_string)?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let target = if requested.is_dir() {
            requested
        } else {
            existing.parent().unwrap_or(&existing).to_path_buf()
        };
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map_err(error_string)?;
    }
    Ok(existing.display().to_string())
}

fn meaningful_existing_ancestor(requested: &Path) -> Option<PathBuf> {
    requested
        .ancestors()
        .skip(1)
        .enumerate()
        .find_map(|(index, parent)| {
            if !parent.is_dir() {
                return None;
            }
            let distance = index + 1;
            let generic = parent
                .file_name()
                .and_then(|value| value.to_str())
                .is_none_or(|leaf| {
                    matches!(
                        leaf.to_ascii_lowercase().as_str(),
                        "users"
                            | "documents"
                            | "downloads"
                            | "desktop"
                            | "appdata"
                            | "local"
                            | "roaming"
                    )
                });
            (distance <= 3 && (distance == 1 || !generic)).then(|| parent.to_path_buf())
        })
}

#[cfg(target_os = "windows")]
fn windows_select_argument(path: &Path) -> String {
    format!("/select,\"{}\"", path.display())
}

#[cfg(target_os = "windows")]
fn shell_explore_directory(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let operation = "explore\0".encode_utf16().collect::<Vec<_>>();
    let target = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result <= 32 {
        return Err(format!(
            "Klasör Windows Explorer ile açılamadı (hata {result}): {}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: *mut std::ffi::c_void,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show_command: i32,
    ) -> isize;
}

fn normalize_requested_path(value: &str) -> Result<PathBuf, String> {
    let cleaned = value
        .trim()
        .trim_matches(['"', '\'', '<', '>'])
        .trim_start_matches("file:///")
        .trim_start_matches("file://")
        .to_string();
    #[cfg(target_os = "windows")]
    let cleaned = {
        let mut cleaned = cleaned.replace('/', "\\");
        if cleaned.len() >= 4
            && cleaned.starts_with('\\')
            && cleaned.as_bytes().get(2) == Some(&b':')
            && cleaned.as_bytes().get(3) == Some(&b'\\')
        {
            cleaned.remove(0);
        }
        if let Some(index) = cleaned
            .char_indices()
            .skip(3)
            .find(|(index, value)| {
                *value == '\\'
                    && cleaned.as_bytes().get(index + 2) == Some(&b':')
                    && cleaned.as_bytes().get(index + 3) == Some(&b'\\')
            })
            .map(|(index, _)| index + 1)
        {
            cleaned = cleaned[index..].to_string();
        }
        cleaned
    };
    let path = PathBuf::from(&cleaned);
    if !path.is_absolute() {
        return Err(format!("Mutlak bir dosya yolu bulunamadı: {value}"));
    }
    Ok(path)
}

#[tauri::command]
async fn get_session_turns(
    id: String,
    mode: String,
    offset: usize,
    limit: Option<usize>,
    search: Option<String>,
    state: State<'_, AppState>,
) -> Result<TurnPage, String> {
    let database_path = state.database_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Archive::open_existing(&database_path)
            .map_err(error_string)?
            .load_turn_page(&id, &mode, offset, limit.unwrap_or(80), search.as_deref())
            .map_err(error_string)
    })
    .await
    .map_err(error_string)?
}

#[tauri::command]
async fn usage_analytics(
    provider: Option<String>,
    state: State<'_, AppState>,
) -> Result<UsageAnalytics, String> {
    let database_path = state.database_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Archive::open_existing(&database_path)
            .map_err(error_string)?
            .usage_analytics(provider.as_deref())
            .map_err(error_string)
    })
    .await
    .map_err(error_string)?
}

#[tauri::command]
async fn export_session_to_file(
    session_id: String,
    format: String,
    path: String,
    search: Option<String>,
    options: ExportOptions,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let database_path = state.database_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let format = parse_export_format(&format)?;
        let archive = Archive::open_existing(&database_path).map_err(error_string)?;
        let mut session = archive
            .get_session(&session_id)
            .map_err(error_string)?
            .ok_or_else(|| "Session was not found".to_string())?;
        filter_session(&mut session, search.as_deref());
        let output = export_session(&session, format, &options).map_err(error_string)?;
        write_export(&path, output)
    })
    .await
    .map_err(error_string)?
}

#[tauri::command]
async fn export_archive_to_file(
    provider: Option<String>,
    search: Option<String>,
    format: String,
    path: String,
    options: ExportOptions,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let database_path = state.database_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let format = parse_export_format(&format)?;
        let archive = Archive::open_existing(&database_path).map_err(error_string)?;
        let rows = archive
            .list_sessions(provider.as_deref(), search.as_deref(), 10_000)
            .map_err(error_string)?;
        let mut sessions = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(mut session) = archive.get_session(&row.id).map_err(error_string)? {
                filter_session(&mut session, search.as_deref());
                sessions.push(session);
            }
        }
        let output = match format {
            ExportFormat::Json => serde_json::to_string_pretty(&sessions).map_err(error_string)?,
            ExportFormat::Jsonl => sessions
                .iter()
                .map(serde_json::to_string)
                .collect::<Result<Vec<_>, _>>()
                .map_err(error_string)?
                .join("\n"),
            _ => sessions
                .iter()
                .map(|session| export_session(session, format, &options).map_err(error_string))
                .collect::<Result<Vec<_>, _>>()?
                .join("\n\n---\n\n"),
        };
        let count = sessions.len();
        write_export(&path, output)?;
        Ok(count)
    })
    .await
    .map_err(error_string)?
}

fn parse_export_format(format: &str) -> Result<ExportFormat, String> {
    match format {
        "markdown" => Ok(ExportFormat::Markdown),
        "prompts" => Ok(ExportFormat::Prompts),
        "system" => Ok(ExportFormat::SystemPrompts),
        "context" => Ok(ExportFormat::ContextPrompts),
        "responses" => Ok(ExportFormat::Responses),
        "tools" => Ok(ExportFormat::ToolCalls),
        "summary" => Ok(ExportFormat::Summary),
        "json" => Ok(ExportFormat::Json),
        "jsonl" => Ok(ExportFormat::Jsonl),
        _ => Err(format!("Unsupported export format: {format}")),
    }
}

fn filter_session(session: &mut StoredSession, search: Option<&str>) {
    let Some(query) = search.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let query = query.to_lowercase();
    session.turns.retain(|turn| {
        turn.text.to_lowercase().contains(&query)
            || turn.tool_calls.iter().any(|tool| {
                tool.name.to_lowercase().contains(&query)
                    || tool
                        .arguments_json
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains(&query)
                    || tool
                        .result_text
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains(&query)
            })
    });
}

fn write_export(path: &str, output: String) -> Result<(), String> {
    let destination = PathBuf::from(path);
    if let Some(parent) = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(error_string)?;
    }
    std::fs::write(destination, output).map_err(error_string)
}

fn data_location(app: &tauri::App) -> Result<(PathBuf, bool), String> {
    let executable = std::env::current_exe().map_err(error_string)?;
    let executable_dir = executable.parent().unwrap_or_else(|| Path::new("."));
    if executable_dir.join("portable.flag").is_file() {
        return Ok((
            executable_dir.join("data").join("contextractor.sqlite"),
            true,
        ));
    }
    let app_data = app.path().app_data_dir().map_err(error_string)?;
    Ok((app_data.join("contextractor.sqlite"), false))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let (database_path, portable) = data_location(app)?;
            Archive::open(&database_path).map_err(error_string)?;
            app.manage(AppState {
                database_path,
                portable,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            discover_sources,
            scan_sources,
            list_sessions,
            get_session,
            get_session_turns,
            get_tool_call,
            get_session_files,
            collect_session_files,
            usage_analytics,
            usage_costs,
            reveal_path,
            export_session_to_file,
            export_archive_to_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running Contextractor");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_collection_copies_tree_with_structure_and_origins() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("package").join("workspace");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("nested").join("note.md"), "hello").unwrap();
        let mut counters = CollectionCounters::default();
        let mut entries = Vec::new();
        copy_tree(
            &source,
            &target,
            &root.path().join("package"),
            &mut counters,
            &mut entries,
            vec!["workspace".into()],
        );
        assert_eq!(counters.files, 1);
        assert_eq!(counters.bytes, 5);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            fs::read_to_string(target.join("nested").join("note.md")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn file_collection_filters_conversation_origins() {
        let user_and_tool = vec!["user".into(), "tool".into()];
        let assistant = vec!["assistant".into()];
        let system = vec!["system".into()];
        assert!(reference_matches_origin(&user_and_tool, "user"));
        assert!(reference_matches_origin(&user_and_tool, "all"));
        assert!(reference_matches_origin(&assistant, "assistant"));
        assert!(reference_matches_origin(&assistant, "all"));
        assert!(!reference_matches_origin(&system, "all"));
    }

    #[test]
    fn file_collection_deduplicates_the_same_physical_file() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.md");
        fs::write(&source, "same").unwrap();
        let mut counters = CollectionCounters::default();
        let mut entries = Vec::new();
        copy_one(
            &source,
            &root.path().join("first.md"),
            &mut counters,
            &mut entries,
            vec!["user".into()],
        );
        copy_one(
            &source,
            &root.path().join("second.md"),
            &mut counters,
            &mut entries,
            vec!["assistant".into()],
        );
        assert_eq!(counters.files, 1);
        assert_eq!(counters.duplicates, 1);
        assert!(!root.path().join("second.md").exists());
        assert_eq!(entries[1].status, "deduplicated");
    }

    #[test]
    fn missing_path_does_not_fall_back_to_a_generic_documents_folder() {
        let root = tempfile::tempdir().unwrap();
        let documents = root.path().join("Documents");
        fs::create_dir(&documents).unwrap();
        assert!(meaningful_existing_ancestor(&documents.join("missing.md")).is_some());
        assert!(meaningful_existing_ancestor(
            &documents
                .join("missing-project")
                .join("nested")
                .join("file.md")
        )
        .is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn reveal_path_repairs_markdown_and_duplicated_windows_roots() {
        assert_eq!(
            normalize_requested_path("/E:/Obsidian Vaults/Trace Analysis/system/index.md").unwrap(),
            PathBuf::from(r"E:\Obsidian Vaults\Trace Analysis\system\index.md")
        );
        assert_eq!(
            normalize_requested_path(
                r"E:\trace analysis\E:\Obsidian Vaults\Trace Analysis\system\index.md"
            )
            .unwrap(),
            PathBuf::from(r"E:\Obsidian Vaults\Trace Analysis\system\index.md")
        );
        assert_eq!(
            windows_select_argument(Path::new(
                r"E:\Obsidian Vaults\Trace Analysis\system\index.md"
            )),
            r#"/select,"E:\Obsidian Vaults\Trace Analysis\system\index.md""#
        );
    }
}
