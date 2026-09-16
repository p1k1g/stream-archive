use anyhow::{Context, Result, bail};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};
use stream_archive_server::tool_discovery::{
    ToolKind, ToolResolution, executable_file, find_command, resolve_tool,
};

const SETTINGS_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    source TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };

    match command {
        "help" | "--help" | "-h" => print_help(),
        "version" | "--version" | "-V" => {
            println!("stream-archive-cli {}", env!("CARGO_PKG_VERSION"));
        }
        "init" => command_init()?,
        "doctor" => command_doctor()?,
        "tools" => command_tools(&args[1..])?,
        "serve" => command_serve(&args[1..])?,
        other => bail!("unknown command `{other}`; run `stream-archive-cli help`"),
    }
    Ok(())
}

fn print_help() {
    println!(
        r#"Stream Archive CLI

Unix/headless-oriented runtime helper for the shared Rust core.

Usage:
  stream-archive-cli init
  stream-archive-cli doctor
  stream-archive-cli tools
  stream-archive-cli tools --json
  stream-archive-cli tools configure
  stream-archive-cli serve [--watch]
  stream-archive-cli version

Commands:
  init             Create the local backend/data layout and settings database.
  doctor           Show paths, native secret-store readiness and media-tool status.
  tools            Discover Streamlink, yt-dlp and FFmpeg without Windows-only names.
  tools configure  Persist discovered absolute tool paths into SQLite atomically.
  serve            Run the sibling stream-archive-server in the foreground.
  serve --watch    Run the server and auto-start the LIVE watcher.

Environment:
  STREAM_ARCHIVE_BACKEND_DIR  Explicit backend directory.
  STREAM_ARCHIVE_DATA_DIR     Explicit data directory containing stream-archive.db.

Phase 20 keeps Windows GUI work out of this CLI. Phase 21 will introduce the Slint UI.
"#
    );
}

fn command_init() -> Result<()> {
    let backend = backend_dir(false)?;
    let data = data_dir(&backend)?;
    fs::create_dir_all(backend.join("vod"))
        .with_context(|| format!("failed to create {}", backend.join("vod").display()))?;
    fs::create_dir_all(&data).with_context(|| format!("failed to create {}", data.display()))?;
    let db = database_path(&backend)?;
    let conn = open_settings_db(&db)?;
    conn.execute_batch(SETTINGS_SCHEMA)?;

    println!("initialized Stream Archive headless layout");
    println!("backend : {}", backend.display());
    println!("data    : {}", data.display());
    println!("db      : {}", db.display());
    println!("next    : stream-archive-cli tools configure");
    Ok(())
}

fn command_doctor() -> Result<()> {
    let backend = backend_dir(false)?;
    let db = database_path(&backend)?;
    let settings = load_tool_settings(&db)?;
    let tools = resolve_all(&backend, &settings);

    println!("Stream Archive doctor");
    println!("os       : {} / {}", env::consts::OS, env::consts::ARCH);
    println!(
        "backend  : {}{}",
        backend.display(),
        exists_marker(&backend)
    );
    println!("database : {}{}", db.display(), exists_marker(&db));

    #[cfg(target_os = "linux")]
    {
        let secret_tool = find_command("secret-tool");
        println!(
            "secrets  : Linux Secret Service {}",
            secret_tool
                .as_ref()
                .map(|path| format!("ready ({})", path.display()))
                .unwrap_or_else(|| "not ready (`secret-tool` missing)".into())
        );
    }
    #[cfg(target_os = "macos")]
    println!("secrets  : macOS Keychain Services (native)");
    #[cfg(windows)]
    println!("secrets  : Windows CurrentUser DPAPI (native)");
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    println!("secrets  : unsupported on this operating system");

    print_tools(&tools);

    let mut problems = Vec::new();
    if !backend.is_dir() {
        problems.push("backend directory is missing; run `stream-archive-cli init`");
    }
    if !db.is_file() {
        problems.push("database is missing; run `stream-archive-cli init`");
    }
    for tool in &tools {
        if !tool.found() {
            problems.push(match tool.kind {
                ToolKind::Streamlink => "Streamlink is missing",
                ToolKind::YtDlp => "yt-dlp is missing",
                ToolKind::Ffmpeg => "FFmpeg is missing",
            });
        }
    }
    #[cfg(target_os = "linux")]
    if find_command("secret-tool").is_none() {
        problems.push("secret-tool/libsecret-tools is missing for persisted Linux secrets");
    }

    if problems.is_empty() {
        println!("doctor   : OK");
        Ok(())
    } else {
        println!("doctor   : needs attention");
        for problem in &problems {
            println!("  - {problem}");
        }
        bail!("doctor found {} issue(s)", problems.len())
    }
}

fn command_tools(args: &[String]) -> Result<()> {
    let backend = backend_dir(false)?;
    let db = database_path(&backend)?;
    let settings = load_tool_settings(&db)?;
    let tools = resolve_all(&backend, &settings);

    match args {
        [] => print_tools(&tools),
        [flag] if flag == "--json" => print_tools_json(&tools)?,
        [action] if action == "configure" => configure_tools(&backend, &db, tools)?,
        _ => bail!("usage: stream-archive-cli tools [--json|configure]"),
    }
    Ok(())
}

fn command_serve(args: &[String]) -> Result<()> {
    let watch = match args {
        [] => false,
        [flag] if flag == "--watch" => true,
        _ => bail!("usage: stream-archive-cli serve [--watch]"),
    };
    let backend = backend_dir(false)?;
    if !backend.is_dir() {
        bail!(
            "backend directory does not exist: {}; run `stream-archive-cli init`",
            backend.display()
        );
    }
    let server = server_binary().context(
        "stream-archive-server was not found next to the CLI or in PATH; build/install both binaries",
    )?;
    let mut command = Command::new(&server);
    command.env("STREAM_ARCHIVE_BACKEND_DIR", &backend);
    if watch {
        command.env("STREAM_ARCHIVE_START_WATCHER", "1");
    }
    let status = command
        .status()
        .with_context(|| format!("failed to start {}", server.display()))?;
    if !status.success() {
        bail!("stream-archive-server exited with {status}");
    }
    Ok(())
}

fn configure_tools(backend: &Path, db: &Path, tools: Vec<ToolResolution>) -> Result<()> {
    let missing = tools
        .iter()
        .filter(|tool| !tool.found())
        .map(|tool| tool.kind.label())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        print_tools(&tools);
        bail!(
            "cannot configure tools until all required executables are discoverable: {}",
            missing.join(", ")
        );
    }

    fs::create_dir_all(backend.join("vod"))?;
    let mut conn = open_settings_db(db)?;
    conn.execute_batch(SETTINGS_SCHEMA)?;
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();
    for tool in &tools {
        let path = tool.path.as_ref().expect("checked above");
        tx.execute(
            r#"
            INSERT INTO settings(key,value,source,updated_at)
            VALUES(?1,?2,'cli-tool-discovery',?3)
            ON CONFLICT(key) DO UPDATE SET
                value=excluded.value,
                source=excluded.source,
                updated_at=excluded.updated_at
            "#,
            params![
                tool.kind.primary_setting_key(),
                path.to_string_lossy().as_ref(),
                now.as_str()
            ],
        )?;
    }
    tx.commit()?;

    println!("configured media tools in {}", db.display());
    print_tools(&tools);
    println!("source   : cli-tool-discovery");
    Ok(())
}

fn resolve_all(backend: &Path, settings: &BTreeMap<String, String>) -> Vec<ToolResolution> {
    ToolKind::ALL
        .into_iter()
        .map(|kind| {
            let configured = kind
                .setting_keys()
                .iter()
                .map(|key| (*key, settings.get(*key).map(String::as_str).unwrap_or("")))
                .collect::<Vec<_>>();
            resolve_tool(kind, backend, &configured)
        })
        .collect()
}

fn print_tools(tools: &[ToolResolution]) {
    println!("media tools");
    for tool in tools {
        let path = tool
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<missing>".into());
        println!(
            "  {:10} {:7} {:12} {}",
            tool.kind.label(),
            if tool.found() { "OK" } else { "MISSING" },
            tool.source,
            path
        );
        for warning in &tool.warnings {
            println!("    warning: {warning}");
        }
    }
}

fn print_tools_json(tools: &[ToolResolution]) -> Result<()> {
    let rows = tools
        .iter()
        .map(|tool| {
            json!({
                "tool": tool.kind.label(),
                "found": tool.found(),
                "source": tool.source.as_str(),
                "path": tool.path.as_ref().map(|path| path.display().to_string()),
                "warnings": &tool.warnings,
            })
        })
        .collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}

fn load_tool_settings(db: &Path) -> Result<BTreeMap<String, String>> {
    if !db.is_file() {
        return Ok(BTreeMap::new());
    }
    let conn = Connection::open(db)
        .with_context(|| format!("failed to open SQLite database {}", db.display()))?;
    let table_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='settings')",
        [],
        |row| row.get(0),
    )?;
    if !table_exists {
        return Ok(BTreeMap::new());
    }

    let mut result = BTreeMap::new();
    for kind in ToolKind::ALL {
        for key in kind.setting_keys() {
            if result.contains_key(*key) {
                continue;
            }
            let value = conn
                .query_row(
                    "SELECT value FROM settings WHERE key=?1",
                    params![key],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if let Some(value) = value {
                result.insert((*key).to_string(), value);
            }
        }
    }
    Ok(result)
}

fn open_settings_db(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let conn = Connection::open(path)
        .with_context(|| format!("failed to open SQLite database {}", path.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(conn)
}

fn backend_dir(require_existing: bool) -> Result<PathBuf> {
    if let Ok(value) = env::var("STREAM_ARCHIVE_BACKEND_DIR") {
        let path = absolute_path(PathBuf::from(value))?;
        if require_existing && !path.is_dir() {
            bail!(
                "STREAM_ARCHIVE_BACKEND_DIR is not a directory: {}",
                path.display()
            );
        }
        return Ok(path);
    }

    let cwd = env::current_dir().context("cannot resolve current directory")?;
    let cwd_backend = cwd.join("backend");
    if cwd_backend.is_dir() {
        return Ok(cwd_backend);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            let sibling = parent.join("backend");
            if sibling.is_dir() {
                return Ok(sibling);
            }
        }
    }
    if require_existing {
        bail!("backend directory not found; run `stream-archive-cli init`");
    }
    Ok(cwd_backend)
}

fn data_dir(backend: &Path) -> Result<PathBuf> {
    if let Ok(value) = env::var("STREAM_ARCHIVE_DATA_DIR") {
        if !value.trim().is_empty() {
            return absolute_path(PathBuf::from(value));
        }
    }
    Ok(backend.parent().unwrap_or(backend).join("data"))
}

fn database_path(backend: &Path) -> Result<PathBuf> {
    Ok(data_dir(backend)?.join("stream-archive.db"))
}

fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(env::current_dir()
            .context("cannot resolve current directory")?
            .join(path))
    }
}

fn exists_marker(path: &Path) -> &'static str {
    if path.exists() { " [OK]" } else { " [missing]" }
}

fn server_binary() -> Option<PathBuf> {
    #[cfg(windows)]
    const NAMES: &[&str] = &["stream-archive-server.exe", "stream-archive-server"];
    #[cfg(not(windows))]
    const NAMES: &[&str] = &["stream-archive-server", "stream-archive-server.exe"];

    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            for name in NAMES {
                let candidate = parent.join(name);
                if executable_file(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    for name in NAMES {
        if let Some(path) = find_command(name) {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_setting_mapping_stays_stable() {
        assert_eq!(
            ToolKind::Streamlink.primary_setting_key(),
            "STREAMLINK_PATH"
        );
        assert_eq!(ToolKind::YtDlp.primary_setting_key(), "YT_DLP_PATH");
        assert_eq!(ToolKind::Ffmpeg.primary_setting_key(), "FFMPEG_PATH");
    }

    #[test]
    fn missing_database_has_no_tool_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let settings = load_tool_settings(&dir.path().join("missing.db")).unwrap();
        assert!(settings.is_empty());
    }
}
