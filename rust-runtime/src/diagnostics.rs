//! Shared read-only runtime preflight.
//!
//! This service performs local filesystem/settings/SQLite inspection only.
//! It does not contact providers, download media, create directories, mutate
//! settings, or expose secret values. External executable execution remains a
//! Phase 23.3 integration concern; tool checks here are deterministic discovery.
use crate::{
    backup_service::{BackupManager, BackupPolicy},
    store::Store,
    tool_discovery::{ToolKind, ToolResolution, resolve_tool},
};
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DiagnosticStatus {
    Ok,
    Warning,
    Error,
}

impl DiagnosticStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Warning => "Warning",
            Self::Error => "Error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticCategory {
    Runtime,
    Database,
    Storage,
    Tools,
    Secrets,
    Providers,
    Backup,
}

impl DiagnosticCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Runtime => "Runtime",
            Self::Database => "Database",
            Self::Storage => "Storage",
            Self::Tools => "Tools",
            Self::Secrets => "Secrets",
            Self::Providers => "Providers",
            Self::Backup => "Backup",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticRequirement {
    Required,
    Optional,
    Informational,
}

impl DiagnosticRequirement {
    pub fn label(self) -> &'static str {
        match self {
            Self::Required => "Required",
            Self::Optional => "Optional",
            Self::Informational => "Informational",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticItem {
    pub id: String,
    pub category: DiagnosticCategory,
    pub requirement: DiagnosticRequirement,
    pub name: String,
    pub status: DiagnosticStatus,
    pub summary: String,
    pub detail: String,
    pub remediation: String,
}

macro_rules! check {
    ($id:expr, $category:expr, $requirement:expr, $name:expr, $status:expr, $summary:expr, $detail:expr, $remediation:expr $(,)?) => {
        DiagnosticItem {
            id: ($id).into(),
            category: $category,
            requirement: $requirement,
            name: ($name).into(),
            status: $status,
            summary: ($summary).into(),
            detail: ($detail).into(),
            remediation: ($remediation).into(),
        }
    };
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticsSummary {
    pub ok: usize,
    pub warning: usize,
    pub error: usize,
    pub blocking_errors: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsSnapshot {
    pub status: DiagnosticStatus,
    pub runtime_ready: bool,
    pub attention_required: bool,
    pub summary: DiagnosticsSummary,
    pub items: Vec<DiagnosticItem>,
}

#[derive(Debug, Clone)]
pub struct PreflightInput {
    pub values: BTreeMap<String, String>,
    pub configured_secrets: BTreeMap<String, bool>,
    pub backup_directory: PathBuf,
    pub backup_policy: BackupPolicy,
}

impl DiagnosticsSnapshot {
    pub fn startup_failure(backend: Option<&Path>, error: &str) -> Self {
        let database = backend.map(crate::store::Store::default_path);
        Self::unavailable(backend, database.as_deref(), error)
    }

    pub fn unavailable(backend: Option<&Path>, database: Option<&Path>, error: &str) -> Self {
        let mut items = vec![check!(
            "runtime.settings",
            DiagnosticCategory::Runtime,
            DiagnosticRequirement::Required,
            "Settings load",
            DiagnosticStatus::Error,
            "Canonical settings are unavailable",
            error,
            "Resolve the SQLite/settings error and refresh diagnostics.",
        )];
        if let Some(path) = backend {
            items.push(directory_check(
                "runtime.backend",
                DiagnosticCategory::Runtime,
                DiagnosticRequirement::Required,
                "Canonical backend",
                path,
            ));
        }
        if let Some(path) = database {
            items.push(database_path_check(path));
            items.push(database_integrity_check(path));
        }
        Self::from_items(items)
    }

    pub fn from_items(items: Vec<DiagnosticItem>) -> Self {
        let mut summary = DiagnosticsSummary::default();
        let mut status = DiagnosticStatus::Ok;
        for row in &items {
            status = status.max(row.status);
            match row.status {
                DiagnosticStatus::Ok => summary.ok += 1,
                DiagnosticStatus::Warning => summary.warning += 1,
                DiagnosticStatus::Error => {
                    summary.error += 1;
                    if row.requirement == DiagnosticRequirement::Required {
                        summary.blocking_errors += 1;
                    }
                }
            }
        }
        let runtime_ready = summary.blocking_errors == 0;
        let attention_required = summary.warning > 0 || summary.error > 0;
        Self {
            status,
            runtime_ready,
            attention_required,
            summary,
            items,
        }
    }
}

pub fn load_read_only_preflight_input(backend: &Path, database: &Path) -> Result<PreflightInput> {
    let store_input = Store::read_preflight_settings(database).context("settings load failed")?;
    let backup = BackupManager::preflight_state(backend, &store_input.values)
        .context("backup preflight load failed")?;
    Ok(PreflightInput {
        values: store_input.values,
        configured_secrets: store_input.configured_secrets,
        backup_directory: backup.directory,
        backup_policy: backup.policy,
    })
}

pub fn collect_preflight_input(
    backend: &Path,
    database: &Path,
    input: &PreflightInput,
) -> DiagnosticsSnapshot {
    collect_with_backup_and_secrets(
        backend,
        database,
        &input.values,
        &input.configured_secrets,
        &input.backup_directory,
        &input.backup_policy,
    )
}

pub fn collect_read_only_preflight(backend: &Path, database: &Path) -> DiagnosticsSnapshot {
    match load_read_only_preflight_input(backend, database) {
        Ok(input) => collect_preflight_input(backend, database, &input),
        Err(error) => {
            DiagnosticsSnapshot::unavailable(Some(backend), Some(database), &format!("{error:#}"))
        }
    }
}

pub fn collect_with_backup(
    backend: &Path,
    database: &Path,
    values: &BTreeMap<String, String>,
    backup_directory: &Path,
    backup_policy: &BackupPolicy,
) -> DiagnosticsSnapshot {
    collect_with_backup_and_secrets(
        backend,
        database,
        values,
        &BTreeMap::new(),
        backup_directory,
        backup_policy,
    )
}

pub fn collect_with_backup_and_secrets(
    backend: &Path,
    database: &Path,
    values: &BTreeMap<String, String>,
    configured_secrets: &BTreeMap<String, bool>,
    backup_directory: &Path,
    backup_policy: &BackupPolicy,
) -> DiagnosticsSnapshot {
    let mut items = collect_items(backend, database, values, configured_secrets);
    items.push(directory_check(
        "storage.backup",
        DiagnosticCategory::Backup,
        DiagnosticRequirement::Optional,
        "Backup directory",
        backup_directory,
    ));
    let valid_policy = backup_policy.interval_hours >= 1 && backup_policy.retention_days >= 0;
    items.push(check!(
        "backup.policy",
        DiagnosticCategory::Backup,
        DiagnosticRequirement::Optional,
        "Backup policy",
        if valid_policy {
            DiagnosticStatus::Ok
        } else {
            DiagnosticStatus::Error
        },
        if backup_policy.enabled {
            "Automatic backup policy is enabled"
        } else {
            "Automatic backup policy is disabled"
        },
        &format!(
            "enabled={} interval={}h keep={} retention={}d",
            backup_policy.enabled,
            backup_policy.interval_hours,
            backup_policy.keep_count,
            backup_policy.retention_days
        ),
        if valid_policy {
            ""
        } else {
            "Use Settings > Manage to save a valid backup interval/retention policy."
        },
    ));
    DiagnosticsSnapshot::from_items(items)
}

pub fn collect(
    backend: &Path,
    database: &Path,
    values: &BTreeMap<String, String>,
) -> DiagnosticsSnapshot {
    collect_with_secrets(backend, database, values, &BTreeMap::new())
}

pub fn collect_with_secrets(
    backend: &Path,
    database: &Path,
    values: &BTreeMap<String, String>,
    configured_secrets: &BTreeMap<String, bool>,
) -> DiagnosticsSnapshot {
    DiagnosticsSnapshot::from_items(collect_items(backend, database, values, configured_secrets))
}

fn collect_items(
    backend: &Path,
    database: &Path,
    values: &BTreeMap<String, String>,
    configured_secrets: &BTreeMap<String, bool>,
) -> Vec<DiagnosticItem> {
    let mut items = vec![
        directory_check(
            "runtime.backend",
            DiagnosticCategory::Runtime,
            DiagnosticRequirement::Required,
            "Canonical backend",
            backend,
        ),
        database_path_check(database),
        database_integrity_check(database),
        check!(
            "runtime.settings",
            DiagnosticCategory::Runtime,
            DiagnosticRequirement::Required,
            "Settings load",
            DiagnosticStatus::Ok,
            "Canonical SQLite settings are available",
            "Loaded from canonical SQLite settings/cache; no separate INI/TXT configuration file",
            "",
        ),
    ];

    if let Some(parent) = database.parent() {
        items.push(directory_check(
            "storage.data",
            DiagnosticCategory::Storage,
            DiagnosticRequirement::Required,
            "Data directory",
            parent,
        ));
    }

    items.push(optional_directory_check(
        "runtime.bundled_vod_tools",
        DiagnosticCategory::Runtime,
        "Bundled VOD tools directory",
        &backend.join("vod"),
        "Not present; configured paths, PATH, and common install locations are still searched",
    ));

    if let Some(value) = values.get("OUTPUT_DIR").filter(|s| !s.trim().is_empty()) {
        items.push(directory_check(
            "storage.live_output",
            DiagnosticCategory::Storage,
            DiagnosticRequirement::Optional,
            "Configured LIVE output directory",
            Path::new(value),
        ));
    } else {
        items.push(check!(
            "storage.live_output",
            DiagnosticCategory::Storage,
            DiagnosticRequirement::Informational,
            "LIVE output directory",
            DiagnosticStatus::Ok,
            "Runtime default is in use",
            "No explicit OUTPUT_DIR is configured; the existing recorder resolves/creates its normal destination when recording starts.",
            "",
        ));
    }

    for kind in ToolKind::ALL {
        let configured: Vec<_> = kind
            .setting_keys()
            .iter()
            .map(|key| (*key, values.get(*key).map(String::as_str).unwrap_or("")))
            .collect();
        items.push(tool_check(&resolve_tool(kind, backend, &configured)));
    }

    items.push(secret_store_check());
    items.extend(provider_checks(values, configured_secrets));
    items.push(check!(
        "runtime.resources",
        DiagnosticCategory::Runtime,
        DiagnosticRequirement::Required,
        "Runtime resources",
        DiagnosticStatus::Ok,
        "Native provider/runtime resources are compiled in",
        "No legacy recorder scripts, browser launcher, or localhost application API is required.",
        "",
    ));
    items
}

fn directory_check(
    id: &str,
    category: DiagnosticCategory,
    requirement: DiagnosticRequirement,
    name: &str,
    path: &Path,
) -> DiagnosticItem {
    let status = if path.is_dir() {
        DiagnosticStatus::Ok
    } else if requirement == DiagnosticRequirement::Required || path.exists() {
        DiagnosticStatus::Error
    } else {
        DiagnosticStatus::Warning
    };
    check!(
        id,
        category,
        requirement,
        name,
        status,
        if path.is_dir() {
            "Directory is available"
        } else {
            "Directory is unavailable"
        },
        &format!(
            "{} — {}",
            path.display(),
            if path.is_dir() {
                "exists"
            } else {
                "missing or not a directory"
            }
        ),
        if path.is_dir() {
            ""
        } else if requirement == DiagnosticRequirement::Required {
            "Restore the expected directory before starting runtime work."
        } else {
            "Configure or create the directory before using the related feature."
        },
    )
}

fn optional_directory_check(
    id: &str,
    category: DiagnosticCategory,
    name: &str,
    path: &Path,
    missing_detail: &str,
) -> DiagnosticItem {
    let detail = if path.is_dir() {
        format!("{} — exists", path.display())
    } else {
        format!("{} — {missing_detail}", path.display())
    };
    check!(
        id,
        category,
        DiagnosticRequirement::Informational,
        name,
        DiagnosticStatus::Ok,
        if path.is_dir() {
            "Optional directory is present"
        } else {
            "Optional directory is not required"
        },
        &detail,
        "",
    )
}

fn database_path_check(path: &Path) -> DiagnosticItem {
    check!(
        "database.primary",
        DiagnosticCategory::Database,
        DiagnosticRequirement::Required,
        "Canonical SQLite database",
        if path.is_file() {
            DiagnosticStatus::Ok
        } else {
            DiagnosticStatus::Error
        },
        if path.is_file() {
            "Database file is present"
        } else {
            "Database file is unavailable"
        },
        &format!(
            "{} — {}",
            path.display(),
            if path.is_file() {
                "exists"
            } else {
                "missing or not a regular file"
            }
        ),
        if path.is_file() {
            ""
        } else {
            "Restore or initialize the canonical data/stream-archive.db before runtime use."
        },
    )
}

fn database_integrity_check(path: &Path) -> DiagnosticItem {
    if !path.is_file() {
        return check!(
            "database.integrity",
            DiagnosticCategory::Database,
            DiagnosticRequirement::Required,
            "SQLite integrity",
            DiagnosticStatus::Error,
            "Integrity cannot be checked",
            "Canonical database file is missing.",
            "Restore or initialize the canonical database first.",
        );
    }
    let result = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|conn| conn.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0)));
    match result {
        Ok(value) if value.eq_ignore_ascii_case("ok") => check!(
            "database.integrity",
            DiagnosticCategory::Database,
            DiagnosticRequirement::Required,
            "SQLite integrity",
            DiagnosticStatus::Ok,
            "SQLite quick_check passed",
            "Read-only PRAGMA quick_check returned ok.",
            "",
        ),
        Ok(value) => check!(
            "database.integrity",
            DiagnosticCategory::Database,
            DiagnosticRequirement::Required,
            "SQLite integrity",
            DiagnosticStatus::Error,
            "SQLite quick_check reported a problem",
            &value,
            "Stop runtime work and restore from a known-good backup before further writes.",
        ),
        Err(error) => check!(
            "database.integrity",
            DiagnosticCategory::Database,
            DiagnosticRequirement::Required,
            "SQLite integrity",
            DiagnosticStatus::Error,
            "SQLite database could not be checked",
            &format!("{error:#}"),
            "Verify file permissions/path and restore from backup if the database is invalid.",
        ),
    }
}

fn tool_check(resolved: &ToolResolution) -> DiagnosticItem {
    let status = if !resolved.found() {
        DiagnosticStatus::Error
    } else if resolved.warnings.is_empty() {
        DiagnosticStatus::Ok
    } else {
        DiagnosticStatus::Warning
    };
    let path = resolved
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "not found".into());
    let id = match resolved.kind {
        ToolKind::Streamlink => "tool.streamlink",
        ToolKind::YtDlp => "tool.ytdlp",
        ToolKind::Ffmpeg => "tool.ffmpeg",
    };
    check!(
        id,
        DiagnosticCategory::Tools,
        DiagnosticRequirement::Required,
        &format!("{} executable", resolved.kind.label()),
        status,
        if resolved.found() {
            "Executable was discovered"
        } else {
            "Required media executable was not found"
        },
        &format!(
            "{path} [{}]; filesystem discovery only; executable/version probing is deferred to Phase 23.3. {}",
            resolved.source,
            resolved.warnings.join("; ")
        ),
        if resolved.found() {
            if resolved.warnings.is_empty() {
                ""
            } else {
                "Review the configured path because discovery used a fallback candidate."
            }
        } else {
            "Configure the executable path or add the tool to PATH before media operations."
        },
    )
}

fn secret_store_check() -> DiagnosticItem {
    #[cfg(target_os = "linux")]
    {
        match crate::tool_discovery::find_command("secret-tool") {
            Some(path) => check!(
                "secret.native_store",
                DiagnosticCategory::Secrets,
                DiagnosticRequirement::Optional,
                "Native secret store",
                DiagnosticStatus::Ok,
                "Secret Service client is available",
                &format!("{}; Secret Service session is not probed", path.display()),
                "",
            ),
            None => check!(
                "secret.native_store",
                DiagnosticCategory::Secrets,
                DiagnosticRequirement::Optional,
                "Native secret store",
                DiagnosticStatus::Warning,
                "Secret Service client is unavailable",
                "secret-tool is missing; persisted Linux secrets require Secret Service.",
                "Install libsecret tools and provide a usable Secret Service session before saving provider secrets.",
            ),
        }
    }
    #[cfg(target_os = "macos")]
    {
        check!(
            "secret.native_store",
            DiagnosticCategory::Secrets,
            DiagnosticRequirement::Optional,
            "Native secret store",
            DiagnosticStatus::Ok,
            "macOS Keychain integration is available",
            "Native Keychain capability only; credentials are not read back by diagnostics.",
            "",
        )
    }
    #[cfg(windows)]
    {
        check!(
            "secret.native_store",
            DiagnosticCategory::Secrets,
            DiagnosticRequirement::Optional,
            "Native secret store",
            DiagnosticStatus::Ok,
            "Windows CurrentUser DPAPI integration is available",
            "Native DPAPI capability only; credentials are not read back by diagnostics.",
            "",
        )
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        check!(
            "secret.native_store",
            DiagnosticCategory::Secrets,
            DiagnosticRequirement::Optional,
            "Native secret store",
            DiagnosticStatus::Warning,
            "Native secret storage is unsupported on this operating system",
            "No plaintext fallback is enabled.",
            "Use a supported operating system for persisted provider secrets.",
        )
    }
}

fn provider_checks(
    values: &BTreeMap<String, String>,
    configured_secrets: &BTreeMap<String, bool>,
) -> Vec<DiagnosticItem> {
    let soop_username = values
        .get("SOOP_USERNAME")
        .is_some_and(|value| !value.trim().is_empty());
    let worker_url = values
        .get("CLOUDFLARE_WORKER_URL")
        .is_some_and(|value| !value.trim().is_empty());
    let soop_password = configured_secrets
        .get("SOOP_PASSWORD")
        .copied()
        .unwrap_or(false);
    let worker_key = configured_secrets
        .get("CLOUDFLARE_API_KEY")
        .copied()
        .unwrap_or(false);
    let soop_ready = soop_username && worker_url && soop_password && worker_key;

    let chzzk_aut = configured_secrets
        .get("CHZZK_NID_AUT")
        .copied()
        .unwrap_or(false);
    let chzzk_ses = configured_secrets
        .get("CHZZK_NID_SES")
        .copied()
        .unwrap_or(false);
    let chzzk_ready = chzzk_aut && chzzk_ses;

    vec![
        check!(
            "provider.soop",
            DiagnosticCategory::Providers,
            DiagnosticRequirement::Optional,
            "SOOP configuration",
            if soop_ready {
                DiagnosticStatus::Ok
            } else {
                DiagnosticStatus::Warning
            },
            if soop_ready {
                "SOOP local configuration is complete"
            } else {
                "SOOP local configuration is incomplete"
            },
            &format!(
                "username={} password={} worker_url={} worker_key={}",
                configured_label(soop_username),
                configured_label(soop_password),
                configured_label(worker_url),
                configured_label(worker_key)
            ),
            if soop_ready {
                ""
            } else {
                "Complete the SOOP/Worker fields in Settings before authenticated SOOP use."
            },
        ),
        check!(
            "provider.chzzk",
            DiagnosticCategory::Providers,
            DiagnosticRequirement::Optional,
            "CHZZK configuration",
            if chzzk_ready {
                DiagnosticStatus::Ok
            } else {
                DiagnosticStatus::Warning
            },
            if chzzk_ready {
                "CHZZK local authentication configuration is complete"
            } else {
                "CHZZK authentication is not fully configured"
            },
            &format!(
                "NID_AUT={} NID_SES={}",
                configured_label(chzzk_aut),
                configured_label(chzzk_ses)
            ),
            if chzzk_ready {
                ""
            } else {
                "Configure both CHZZK NID_AUT and NID_SES for authenticated CHZZK content."
            },
        ),
    ]
}

fn configured_label(value: bool) -> &'static str {
    if value {
        "configured"
    } else {
        "not configured"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn ok_item() -> DiagnosticItem {
        check!(
            "test.ok",
            DiagnosticCategory::Runtime,
            DiagnosticRequirement::Required,
            "test",
            DiagnosticStatus::Ok,
            "ready",
            "ready",
            "",
        )
    }

    #[test]
    fn read_only_preflight_input_preserves_store_load_failures() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("backend");
        std::fs::create_dir_all(&backend).unwrap();
        let db = dir.path().join("broken-settings.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY);")
            .unwrap();
        drop(conn);

        let snapshot = collect_read_only_preflight(&backend, &db);
        assert!(!snapshot.runtime_ready);
        assert!(snapshot.summary.blocking_errors >= 1);
        let settings = snapshot
            .items
            .iter()
            .find(|item| item.id == "runtime.settings")
            .expect("settings failure check");
        assert_eq!(settings.status, DiagnosticStatus::Error);
        assert!(settings.detail.contains("settings load failed"));
    }

    #[test]
    fn missing_paths_are_reported_without_creating_them() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("missing");
        let db = dir.path().join("missing.db");
        let report = collect(&backend, &db, &BTreeMap::new());
        assert_eq!(report.status, DiagnosticStatus::Error);
        assert!(!report.runtime_ready);
        assert!(report.summary.blocking_errors >= 1);
        assert!(!backend.exists());
        assert!(!db.exists());
    }

    #[test]
    fn optional_warning_does_not_block_runtime() {
        let report = DiagnosticsSnapshot::from_items(vec![
            ok_item(),
            check!(
                "test.optional",
                DiagnosticCategory::Providers,
                DiagnosticRequirement::Optional,
                "optional",
                DiagnosticStatus::Warning,
                "attention",
                "attention",
                "",
            ),
        ]);
        assert!(report.runtime_ready);
        assert!(report.attention_required);
        assert_eq!(report.summary.warning, 1);
        assert_eq!(report.summary.blocking_errors, 0);
    }

    #[test]
    fn required_error_blocks_runtime() {
        let report = DiagnosticsSnapshot::from_items(vec![check!(
            "test.required",
            DiagnosticCategory::Database,
            DiagnosticRequirement::Required,
            "required",
            DiagnosticStatus::Error,
            "failed",
            "failed",
            "",
        )]);
        assert!(!report.runtime_ready);
        assert_eq!(report.summary.blocking_errors, 1);
    }

    #[test]
    fn missing_optional_bundled_vod_directory_is_informational() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backend").join("vod");
        let row = optional_directory_check(
            "runtime.bundled_vod_tools",
            DiagnosticCategory::Runtime,
            "Bundled VOD tools directory",
            &path,
            "Not present",
        );
        assert_eq!(row.status, DiagnosticStatus::Ok);
        assert_eq!(row.requirement, DiagnosticRequirement::Informational);
        assert!(!path.exists());
    }

    #[test]
    fn discovery_warnings_and_missing_tools_preserve_requirement() {
        let mut resolution = ToolResolution {
            kind: ToolKind::Ffmpeg,
            path: None,
            source: "missing".into(),
            warnings: vec![],
        };
        let missing = tool_check(&resolution);
        assert_eq!(missing.status, DiagnosticStatus::Error);
        assert_eq!(missing.requirement, DiagnosticRequirement::Required);
        assert_eq!(missing.id, "tool.ffmpeg");

        resolution.path = Some("ffmpeg.exe".into());
        assert_eq!(tool_check(&resolution).status, DiagnosticStatus::Ok);
        resolution
            .warnings
            .push("configured path missing; using fallback".into());
        assert_eq!(tool_check(&resolution).status, DiagnosticStatus::Warning);
    }

    #[test]
    fn valid_sqlite_database_passes_read_only_quick_check() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("valid.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute("CREATE TABLE sample(id INTEGER PRIMARY KEY)", [])
            .unwrap();
        drop(conn);
        let row = database_integrity_check(&db);
        assert_eq!(row.status, DiagnosticStatus::Ok);
    }

    #[test]
    fn invalid_sqlite_file_fails_integrity_check() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("invalid.db");
        std::fs::write(&db, b"not sqlite").unwrap();
        let row = database_integrity_check(&db);
        assert_eq!(row.status, DiagnosticStatus::Error);
        assert_eq!(row.id, "database.integrity");
    }

    #[test]
    fn provider_checks_never_include_secret_values() {
        let values = BTreeMap::from([
            ("SOOP_USERNAME".into(), "tester".into()),
            (
                "CLOUDFLARE_WORKER_URL".into(),
                "https://example.invalid".into(),
            ),
        ]);
        let secrets = BTreeMap::from([
            ("SOOP_PASSWORD".into(), true),
            ("CLOUDFLARE_API_KEY".into(), true),
            ("CHZZK_NID_AUT".into(), true),
            ("CHZZK_NID_SES".into(), false),
        ]);
        let rows = provider_checks(&values, &secrets);
        let json = serde_json::to_string(&rows).unwrap();
        assert!(!json.contains("tester"));
        assert!(!json.contains("example.invalid"));
        assert!(json.contains("configured"));
        assert!(json.contains("not configured"));
    }

    #[test]
    fn backup_diagnostics_are_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("backend");
        let database = dir.path().join("stream-archive.db");
        let backup = dir.path().join("missing-backups");
        let report = collect_with_backup(
            &backend,
            &database,
            &BTreeMap::new(),
            &backup,
            &BackupPolicy {
                enabled: true,
                interval_hours: 24,
                keep_count: 10,
                retention_days: 3,
            },
        );
        assert!(!backup.exists());
        assert!(report.items.iter().any(|item| item.id == "storage.backup"));
    }

    #[test]
    fn aggregate_and_serialization_preserve_state() {
        let report = DiagnosticsSnapshot::from_items(vec![ok_item()]);
        assert!(report.runtime_ready);
        let json = serde_json::to_string(&report).unwrap();
        let decoded: DiagnosticsSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.status, DiagnosticStatus::Ok);
        assert_eq!(decoded.summary.ok, 1);
        let failed = DiagnosticsSnapshot::unavailable(None, None, "SQLite load failed");
        assert_eq!(failed.status, DiagnosticStatus::Error);
    }
}
