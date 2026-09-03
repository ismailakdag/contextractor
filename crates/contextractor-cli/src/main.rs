use anyhow::{bail, Context, Result};
use contextractor_core::{
    discover, export_session, import_all, Archive, DiscoveryOptions, ExportFormat, ExportOptions,
    ImportOptions,
};
use std::path::PathBuf;

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let command = arguments.first().map(String::as_str).unwrap_or("help");
    let db_path = argument_value(&arguments, "--db")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("contextractor.sqlite"));
    let home = argument_value(&arguments, "--home").map(PathBuf::from);
    let discovery = DiscoveryOptions {
        home_dir: home,
        roaming_dir: argument_value(&arguments, "--roaming").map(PathBuf::from),
        include_desktop_metadata: true,
    };

    match command {
        "discover" => {
            let report = discover(&discovery);
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        "scan" => {
            let mut archive = Archive::open(&db_path)
                .with_context(|| format!("could not open {}", db_path.display()))?;
            let report = import_all(
                &mut archive,
                &ImportOptions {
                    discovery,
                    force: arguments.iter().any(|argument| argument == "--force"),
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        "list" => {
            let archive = Archive::open(&db_path)?;
            let sessions = archive.list_sessions(
                argument_value(&arguments, "--provider"),
                argument_value(&arguments, "--search"),
                200,
            )?;
            println!("{}", serde_json::to_string_pretty(&sessions)?);
        }
        "usage" => {
            let archive = Archive::open(&db_path)?;
            let usage = archive.usage_analytics(argument_value(&arguments, "--provider"))?;
            println!("{}", serde_json::to_string_pretty(&usage)?);
        }
        "export" => {
            let session_id = argument_value(&arguments, "--session")
                .context("export requires --session <id>")?;
            let format = match argument_value(&arguments, "--format").unwrap_or("markdown") {
                "markdown" | "md" => ExportFormat::Markdown,
                "json" => ExportFormat::Json,
                "jsonl" => ExportFormat::Jsonl,
                "prompts" => ExportFormat::Prompts,
                "system" => ExportFormat::SystemPrompts,
                "context" => ExportFormat::ContextPrompts,
                "responses" => ExportFormat::Responses,
                "tools" => ExportFormat::ToolCalls,
                "summary" => ExportFormat::Summary,
                other => bail!("unsupported export format: {other}"),
            };
            let archive = Archive::open(&db_path)?;
            let session = archive
                .get_session(session_id)?
                .context("session not found")?;
            println!(
                "{}",
                export_session(&session, format, &ExportOptions::default())?
            );
        }
        _ => print_help(),
    }
    Ok(())
}

fn argument_value<'a>(arguments: &'a [String], name: &str) -> Option<&'a str> {
    arguments
        .windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].as_str())
}

fn print_help() {
    println!(
        "Contextractor CLI\n\n\
         discover [--home PATH] [--roaming PATH]\n\
         scan --db PATH [--home PATH] [--force]\n\
         list --db PATH [--provider NAME] [--search QUERY]\n\
         usage --db PATH [--provider NAME]\n\
         export --db PATH --session ID [--format markdown|json|jsonl|prompts|system|context|responses|tools|summary]"
    );
}
