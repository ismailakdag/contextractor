use contextractor_core::{
    discover, estimate_usage_cost, export_session, service::import_all_with_progress, Archive,
    CostEstimate, DiscoveryOptions, DiscoveryReport, ExportFormat, ExportOptions, FileReference,
    ImportOptions, ImportReport, SessionListItem, StoredSession, ToolCall, TurnPage,
    UsageAnalytics,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
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
    let requested = PathBuf::from(&path);
    let missing = !requested.exists();
    let existing = if missing {
        requested
            .parent()
            .filter(|parent| parent.is_dir())
            .map(Path::to_path_buf)
            .ok_or_else(|| format!("Dosya ve önceki klasörü artık bu konumda değil: {path}"))?
    } else {
        requested.clone()
    };

    #[cfg(target_os = "windows")]
    {
        let mut command = std::process::Command::new("explorer.exe");
        if !missing && requested.is_file() {
            command.arg(format!("/select,{}", requested.display()));
        } else {
            command.arg(&existing);
        }
        command.spawn().map_err(error_string)?;
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
            usage_analytics,
            usage_costs,
            reveal_path,
            export_session_to_file,
            export_archive_to_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running Contextractor");
}
