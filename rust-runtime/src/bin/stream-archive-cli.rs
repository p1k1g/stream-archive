use anyhow::{Context, Result, bail};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};
use stream_archive_server::{
    diagnostics::{
        DiagnosticsSnapshot, collect_active_local_preflight, collect_read_only_preflight,
    },
    tool_discovery::{ToolKind, ToolResolution, resolve_tool},
    unix_cli::{run_management, run_serve},
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
        "status" | "settings" | "providers" | "channels" | "watcher" | "vod" | "queue"
        | "history" | "backup" | "storage" | "logs" => {
            let runtime =
                tokio::runtime::Runtime::new().context("failed to create Unix CLI runtime")?;
            runtime.block_on(run_management(command, &args[1..]))?;
        }
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
  stream-archive-cli status [--json]
  stream-archive-cli settings show [--json]
  stream-archive-cli settings set <KEY> <VALUE>
  stream-archive-cli providers status [--json]
  stream-archive-cli providers set <KEY> <VALUE>
  stream-archive-cli providers secret <KEY> --stdin
  stream-archive-cli providers test-soop
  stream-archive-cli tools [--json|configure]
  stream-archive-cli doctor [--json] [--active-tools]
  stream-archive-cli channels list [--json]
  stream-archive-cli channels add <platform> <account> <name> <output-dir> [--disabled]
  stream-archive-cli channels remove|enable|disable <platform> <account>
  stream-archive-cli channels action <platform> <account> <stop|resume|recheck>
  stream-archive-cli channels password <platform> <account> --stdin
  stream-archive-cli watcher status [--json]
  stream-archive-cli watcher start
  stream-archive-cli watcher stop
  stream-archive-cli vod analyze <URL> [--json] [options]
  stream-archive-cli vod download <URL> --output <DIR> [--json] [options]
  stream-archive-cli vod status [--json]
  stream-archive-cli vod cancel [--json]
  stream-archive-cli queue list [--json]
  stream-archive-cli queue add <URL> --output <DIR> [options]
  stream-archive-cli queue cancel|retry|remove <ID> [--json]
  stream-archive-cli history list [--json] [filters]
  stream-archive-cli backup status [--json]
  stream-archive-cli backup create [--json]
  stream-archive-cli backup restore <FILE-NAME> --yes [--json]
  stream-archive-cli storage [--json]
  stream-archive-cli logs [--tail N] [--json]
  stream-archive-cli serve [--watch]
  stream-archive-cli version

Commands:
  init       Create the local backend/data layout and settings database.
  status     Show shared runtime readiness and persistent/local operation state.
  settings   Read or update validated non-secret runtime settings.
  providers  Read provider readiness or update provider settings/secrets.
  tools      Discover or persist Streamlink, yt-dlp and FFmpeg paths.
  doctor     Run shared runtime preflight; --active-tools executes local version probes.
  channels   Manage the canonical LIVE channel list through StreamArchiveCore.
  watcher    Inspect or run the shared LIVE watcher.
  vod        Analyze, download, inspect or cancel foreground VOD work.
  queue      Manage the persistent VOD queue.
  history    Read LIVE/VOD history through the shared service.
  backup     Inspect, create or explicitly restore managed backups.
  storage    Show shared storage/free-space state.
  logs       Show the current process runtime log buffer.
  serve      Run the shared headless runtime in the foreground.

Secret input:
  Provider secrets and protected-stream passwords are accepted from stdin only.
  Secret values are not accepted as ordinary command-line arguments.

Environment:
  STREAM_ARCHIVE_BACKEND_DIR  Explicit backend directory.
  STREAM_ARCHIVE_DATA_DIR     Explicit data directory containing stream-archive.db.
  STREAM_ARCHIVE_BACKUP_DIR   Explicit managed backup directory.

Long-running commands remain attached to the CLI process. Ctrl+C and Unix SIGTERM
use the shared shutdown path; no HTTP/Web control plane is started.
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DoctorOptions {
    json: bool,
    active_tools: bool,
}

fn parse_doctor_options(args: &[String]) -> Result<DoctorOptions> {
    let mut options = DoctorOptions::default();
    for arg in args {
        match arg.as_str() {
            "--json" if !options.json => options.json = true,
            "--active-tools" if !options.active_tools => options.active_tools = true,
            "--json" | "--active-tools" => bail!("duplicate doctor option: {arg}"),
            _ => bail!("usage: stream-archive-cli doctor [--json] [--active-tools]"),
        }
    }
    Ok(options)
}

fn command_doctor(args: &[String]) -> Result<()> {
    let options = parse_doctor_options(args)?;
    let backend = backend_dir(false)?;
    let db = database_path(&backend)?;
    let snapshot = if options.active_tools {
        let runtime = tokio::runtime::Runtime::new()
            .context("failed to create local media-tool probe runtime")?;
        runtime.block_on(collect_active_local_preflight(&backend, &db))
    } else {
        doctor_snapshot(&backend, &db)
    };

    if options.json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        print_preflight(&snapshot);
    }

    doctor_result(&snapshot)
}

fn doctor_snapshot(backend: &Path, db: &Path) -> DiagnosticsSnapshot {
    collect_read_only_preflight(backend, db)
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
    let backend = backend_dir(true)?;
    if !backend.is_dir() {
        bail!(
            "backend directory does not exist: {}; run `stream-archive-cli init`",
            backend.display()
        );
    }
    let runtime = tokio::runtime::Runtime::new().context("failed to create headless runtime")?;
    runtime.block_on(run_serve(watch))
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
    fn doctor_options_support_passive_and_active_json_modes() {
        assert_eq!(
            parse_doctor_options(&[]).unwrap(),
            DoctorOptions {
                json: false,
                active_tools: false,
            }
        );
        assert_eq!(
            parse_doctor_options(&["--active-tools".into(), "--json".into()]).unwrap(),
            DoctorOptions {
                json: true,
                active_tools: true,
            }
        );
        assert!(parse_doctor_options(&["--active-tools".into(), "--active-tools".into()]).is_err());
        assert!(parse_doctor_options(&["--network".into()]).is_err());
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
    fn doctor_preserves_settings_read_failure_as_blocking_error() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let db = dir.path().join("broken-settings.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY);")
            .unwrap();
        drop(conn);

        let snapshot = doctor_snapshot(&backend, &db);
        assert!(!snapshot.runtime_ready);
        assert!(snapshot.summary.blocking_errors >= 1);
        let settings = snapshot
            .items
            .iter()
            .find(|item| item.id == "runtime.settings")
            .expect("settings failure check");
        assert_eq!(
            settings.status,
            stream_archive_server::diagnostics::DiagnosticStatus::Error
        );
        assert!(settings.detail.contains("settings load failed"));
        assert!(doctor_result(&snapshot).is_err());
    }

    #[test]
    fn doctor_snapshot_never_serializes_secret_values() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let db = dir.path().join("settings.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(SETTINGS_SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO settings(key,value,source,updated_at) VALUES('CHZZK_NID_AUT','top-secret','test','now')",
            [],
        )
        .unwrap();
        drop(conn);

        let snapshot = doctor_snapshot(&backend, &db);
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(!json.contains("top-secret"));
        assert!(json.contains("configured"));
    }
}
