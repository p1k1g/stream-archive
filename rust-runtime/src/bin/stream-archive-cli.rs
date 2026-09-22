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
use stream_archive_server::{
    backend::HIDDEN_SETTING_KEYS,
    backup_service::{BackupPolicy, resolve_backup_dir},
    diagnostics::{DiagnosticsSnapshot, collect_with_backup_and_secrets},
    tool_discovery::{ToolKind, ToolResolution, executable_file, find_command, resolve_tool},
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
        "doctor" => command_doctor(&args[1..])?,
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
  stream-archive-cli doctor [--json]
  stream-archive-cli tools
  stream-archive-cli tools --json
  stream-archive-cli tools configure
  stream-archive-cli serve [--watch]
  stream-archive-cli version

Commands:
  init             Create the local backend/data layout and settings database.
  doctor           Run the shared local runtime preflight (use --json for automation).
  tools            Discover Streamlink, yt-dlp and FFmpeg without Windows-only names.
  tools configure  Persist discovered absolute tool paths into SQLite atomically.
  serve            Run the sibling headless runtime in the foreground.
  serve --watch    Run the headless runtime and auto-start the LIVE watcher.

Environment:
  STREAM_ARCHIVE_BACKEND_DIR  Explicit backend directory.
  STREAM_ARCHIVE_DATA_DIR     Explicit data directory containing stream-archive.db.

The CLI remains the Linux/macOS headless interface; Windows uses the Slint Native UI by default.
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

fn command_doctor(args: &[String]) -> Result<()> {
    let json_output = match args {
        [] => false,
        [flag] if flag == "--json" => true,
        _ => bail!("usage: stream-archive-cli doctor [--json]"),
    };

    let backend = backend_dir(false)?;
    let db = database_path(&backend)?;
    let (settings, secrets) = load_preflight_settings(&db).unwrap_or_default();
    let backup_policy = backup_policy_from_settings(&settings);
    let backup_dir = backup_directory_from_settings(&backend, &settings)?;
    let snapshot = collect_with_backup_and_secrets(
        &backend,
        &db,
        &settings,
        &secrets,
        &backup_dir,
        &backup_policy,
    );

    if json_output {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        print_preflight(&snapshot);
    }

    doctor_result(&snapshot)
}

fn print_preflight(snapshot: &DiagnosticsSnapshot) {
    println!("Stream Archive runtime preflight");
    println!("os: {} / {}", env::consts::OS, env::consts::ARCH);
    println!();

    for item in &snapshot.items {
        println!(
            "[{:<7}] {:<10} {} / {}",
            item.status.label().to_ascii_uppercase(),
            item.requirement.label(),
            item.category.label(),
            item.name
        );
        println!("  {}", item.summary);
        if !item.detail.is_empty() {
            println!("  detail: {}", item.detail);
        }
        if !item.remediation.is_empty() {
            println!("  action: {}", item.remediation);
        }
    }

    println!();
    println!(
        "Summary: {} OK, {} Warning, {} Error ({} blocking)",
        snapshot.summary.ok,
        snapshot.summary.warning,
        snapshot.summary.error,
        snapshot.summary.blocking_errors
    );
    println!(
        "Runtime usable: {}",
        if snapshot.runtime_ready { "yes" } else { "no" }
    );
}

fn doctor_result(snapshot: &DiagnosticsSnapshot) -> Result<()> {
    if snapshot.runtime_ready {
        Ok(())
    } else {
        bail!(
            "doctor found {} blocking preflight error(s)",
            snapshot.summary.blocking_errors
        )
    }
}

fn backup_policy_from_settings(settings: &BTreeMap<String, String>) -> BackupPolicy {
    let enabled = settings
        .get("BACKUP_ENABLED")
        .map(String::as_str)
        .unwrap_or("Y")
        .eq_ignore_ascii_case("Y");
    let interval_hours = settings
        .get("BACKUP_INTERVAL_HOURS")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(24)
        .max(1);
    let keep_count = settings
        .get("BACKUP_KEEP_COUNT")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10);
    let retention_days = settings
        .get("BACKUP_RETENTION_DAYS")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(3)
        .max(0);
    BackupPolicy {
        enabled,
        interval_hours,
        keep_count,
        retention_days,
    }
}

fn backup_directory_from_settings(
    backend: &Path,
    settings: &BTreeMap<String, String>,
) -> Result<PathBuf> {
    if env::var("STREAM_ARCHIVE_BACKUP_DIR")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return resolve_backup_dir(backend);
    }
    if let Some(value) = settings
        .get("BACKUP_DIR")
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(PathBuf::from(value));
    }
    resolve_backup_dir(backend)
}

fn load_preflight_settings(
    db: &Path,
) -> Result<(BTreeMap<String, String>, BTreeMap<String, bool>)> {
    if !db.is_file() {
        return Ok((BTreeMap::new(), BTreeMap::new()));
    }
    let conn = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("failed to open SQLite database {}", db.display()))?;
    let table_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='settings')",
        [],
        |row| row.get(0),
    )?;
    if !table_exists {
        return Ok((BTreeMap::new(), BTreeMap::new()));
    }

    let mut values = BTreeMap::new();
    let mut secrets = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (key, value) = row?;
        if HIDDEN_SETTING_KEYS.contains(&key.as_str()) {
            secrets.insert(key, !value.trim().is_empty());
        } else {
            values.insert(key, value);
        }
    }
    Ok((values, secrets))
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
        "stream-archive-server headless runtime was not found next to the CLI or in PATH; build/install both binaries",
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
        bail!("stream-archive-server headless runtime exited with {status}");
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
    if let Ok(exe) = env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let sibling = parent.join("backend");
        if sibling.is_dir() {
            return Ok(sibling);
        }
    }
    if require_existing {
        bail!("backend directory not found; run `stream-archive-cli init`");
    }
    Ok(cwd_backend)
}

fn data_dir(backend: &Path) -> Result<PathBuf> {
    if let Ok(value) = env::var("STREAM_ARCHIVE_DATA_DIR")
        && !value.trim().is_empty()
    {
        return absolute_path(PathBuf::from(value));
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

fn server_binary() -> Option<PathBuf> {
    #[cfg(windows)]
    const NAMES: &[&str] = &["stream-archive-server.exe", "stream-archive-server"];
    #[cfg(not(windows))]
    const NAMES: &[&str] = &["stream-archive-server", "stream-archive-server.exe"];

    if let Ok(exe) = env::current_exe()
        && let Some(parent) = exe.parent()
    {
        for name in NAMES {
            let candidate = parent.join(name);
            if executable_file(&candidate) {
                return Some(candidate);
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

    #[test]
    fn doctor_warning_only_is_successful() {
        use stream_archive_server::diagnostics::{
            DiagnosticCategory, DiagnosticItem, DiagnosticRequirement, DiagnosticStatus,
            DiagnosticsSnapshot,
        };
        let snapshot = DiagnosticsSnapshot::from_items(vec![DiagnosticItem {
            id: "provider.test".into(),
            category: DiagnosticCategory::Providers,
            requirement: DiagnosticRequirement::Optional,
            name: "optional".into(),
            status: DiagnosticStatus::Warning,
            summary: "attention".into(),
            detail: String::new(),
            remediation: String::new(),
        }]);
        assert!(doctor_result(&snapshot).is_ok());
    }

    #[test]
    fn doctor_blocking_error_is_failure() {
        use stream_archive_server::diagnostics::{
            DiagnosticCategory, DiagnosticItem, DiagnosticRequirement, DiagnosticStatus,
            DiagnosticsSnapshot,
        };
        let snapshot = DiagnosticsSnapshot::from_items(vec![DiagnosticItem {
            id: "database.test".into(),
            category: DiagnosticCategory::Database,
            requirement: DiagnosticRequirement::Required,
            name: "required".into(),
            status: DiagnosticStatus::Error,
            summary: "failed".into(),
            detail: String::new(),
            remediation: String::new(),
        }]);
        assert!(doctor_result(&snapshot).is_err());
    }

    #[test]
    fn preflight_loader_never_returns_secret_values_as_settings() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("settings.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(SETTINGS_SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO settings(key,value,source,updated_at) VALUES('CHZZK_NID_AUT','top-secret','test','now')",
            [],
        )
        .unwrap();
        drop(conn);

        let (values, secrets) = load_preflight_settings(&db).unwrap();
        assert!(!values.contains_key("CHZZK_NID_AUT"));
        assert_eq!(secrets.get("CHZZK_NID_AUT"), Some(&true));
        let json = serde_json::to_string(&(values, secrets)).unwrap();
        assert!(!json.contains("top-secret"));
    }
}
