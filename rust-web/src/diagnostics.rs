//! Read-only environment diagnostics. No network requests, subprocess probes,
//! directory creation or settings writes are performed by this service.
use crate::{
    backup_service::BackupPolicy,
    tool_discovery::{ToolKind, ToolResolution, resolve_tool},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticItem {
    pub name: String,
    pub status: DiagnosticStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsSnapshot {
    pub status: DiagnosticStatus,
    pub runtime_ready: bool,
    pub items: Vec<DiagnosticItem>,
}

impl DiagnosticsSnapshot {
    pub fn startup_failure(backend: Option<&Path>, error: &str) -> Self {
        let database = backend.map(crate::store::Store::default_path);
        Self::unavailable(backend, database.as_deref(), error)
    }

    pub fn unavailable(backend: Option<&Path>, database: Option<&Path>, error: &str) -> Self {
        let mut items = vec![item("Settings load", DiagnosticStatus::Error, error)];
        if let Some(path) = backend {
            items.push(directory("Canonical backend", path, true));
        }
        if let Some(path) = database {
            items.push(database_item(path));
        }
        Self::from_items(items)
    }

    fn from_items(mut items: Vec<DiagnosticItem>) -> Self {
        let status = items
            .iter()
            .map(|row| row.status)
            .max()
            .unwrap_or(DiagnosticStatus::Error);
        // Readiness describes this environment snapshot, not provider auth or
        // the ability to download every URL. Warnings require attention too.
        let runtime_ready = status == DiagnosticStatus::Ok;
        items.insert(0, item("Runtime readiness", status, "Environment checks only; authentication, network and media execution are not probed"));
        Self {
            status,
            runtime_ready,
            items,
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
    let mut snapshot = collect(backend, database, values);
    let backup_item = directory("Backup directory", backup_directory, false);
    let backup_status = backup_item.status;
    snapshot.items.push(backup_item);
    snapshot.items.push(item(
        "Backup policy",
        DiagnosticStatus::Ok,
        &format!(
            "enabled={} interval={}h keep={} retention={}d",
            backup_policy.enabled,
            backup_policy.interval_hours,
            backup_policy.keep_count,
            backup_policy.retention_days
        ),
    ));
    snapshot.status = snapshot.status.max(backup_status);
    snapshot.runtime_ready = snapshot.status == DiagnosticStatus::Ok;
    if let Some(first) = snapshot.items.first_mut() {
        *first = item(
            "Runtime readiness",
            snapshot.status,
            "Environment checks only; authentication, network and media execution are not probed",
        );
    }
    snapshot
}

pub fn collect(
    backend: &Path,
    database: &Path,
    values: &BTreeMap<String, String>,
) -> DiagnosticsSnapshot {
    let mut items = vec![
        directory("Canonical backend", backend, true),
        database_item(database),
        item(
            "Settings load",
            DiagnosticStatus::Ok,
            "Loaded from canonical SQLite settings/cache; no separate INI/TXT configuration file",
        ),
    ];
    if let Some(parent) = database.parent() {
        items.push(directory("Data directory", parent, true));
    }
    // The bundled tools folder is only one discovery candidate. Its absence
    // must not lower runtime readiness when configured paths/PATH can supply tools.
    items.push(optional_directory(
        "Bundled VOD tools directory (optional)",
        &backend.join("vod"),
        "Not present; configured paths, PATH, and common install locations are still searched",
    ));
    if let Some(value) = values.get("OUTPUT_DIR").filter(|s| !s.trim().is_empty()) {
        items.push(directory(
            "Configured LIVE output directory",
            Path::new(value),
            false,
        ));
    } else {
        items.push(item(
            "LIVE output directory",
            DiagnosticStatus::Ok,
            "Runtime default; created by the existing recorder when needed",
        ));
    }
    for kind in ToolKind::ALL {
        let configured: Vec<_> = kind
            .setting_keys()
            .iter()
            .map(|key| (*key, values.get(*key).map(String::as_str).unwrap_or("")))
            .collect();
        for (key, value) in &configured {
            items.push(item(
                key,
                DiagnosticStatus::Ok,
                if value.is_empty() {
                    "(automatic / unset)"
                } else {
                    value
                },
            ));
        }
        items.push(tool_item(&resolve_tool(kind, backend, &configured)));
    }
    #[cfg(target_os = "linux")]
    items.push(match crate::tool_discovery::find_command("secret-tool") {
        Some(path) => item(
            "Secret-store resource",
            DiagnosticStatus::Ok,
            &format!(
                "Secret Service client: {} (session not probed)",
                path.display()
            ),
        ),
        None => item(
            "Secret-store resource",
            DiagnosticStatus::Warning,
            "secret-tool is missing; persisted secrets require Secret Service",
        ),
    });
    #[cfg(not(target_os = "linux"))]
    {
        items.push(item(
            "Secret-store resource",
            DiagnosticStatus::Ok,
            "Native OS secret-store integration; credentials are not probed",
        ));
    }
    items.push(item(
        "Runtime resources",
        DiagnosticStatus::Ok,
        "Native Rust providers are compiled in; no legacy recorder scripts are required",
    ));
    DiagnosticsSnapshot::from_items(items)
}

fn item(name: &str, status: DiagnosticStatus, detail: &str) -> DiagnosticItem {
    DiagnosticItem {
        name: name.into(),
        status,
        detail: detail.into(),
    }
}

fn directory(name: &str, path: &Path, required: bool) -> DiagnosticItem {
    let status = if path.is_dir() {
        DiagnosticStatus::Ok
    } else if required || path.exists() {
        DiagnosticStatus::Error
    } else {
        DiagnosticStatus::Warning
    };
    item(
        name,
        status,
        &format!(
            "{} — {}",
            path.display(),
            if path.is_dir() {
                "exists"
            } else {
                "missing or not a directory"
            }
        ),
    )
}

fn optional_directory(name: &str, path: &Path, missing_detail: &str) -> DiagnosticItem {
    let detail = if path.is_dir() {
        format!("{} — exists", path.display())
    } else {
        format!("{} — {missing_detail}", path.display())
    };
    item(name, DiagnosticStatus::Ok, &detail)
}

fn database_item(path: &Path) -> DiagnosticItem {
    item(
        "Database path / existence",
        if path.is_file() {
            DiagnosticStatus::Ok
        } else {
            DiagnosticStatus::Error
        },
        &format!(
            "{} — {}",
            path.display(),
            if path.is_file() {
                "exists"
            } else {
                "missing or not a file"
            }
        ),
    )
}

fn tool_item(resolved: &ToolResolution) -> DiagnosticItem {
    let status = if resolved.found() && resolved.warnings.is_empty() {
        DiagnosticStatus::Ok
    } else {
        DiagnosticStatus::Warning
    };
    let path = resolved
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "not found".into());
    item(
        &format!("{} executable availability", resolved.kind.label()),
        status,
        &format!(
            "{path} [{}]; filesystem discovery only, not an execution test. {}",
            resolved.source,
            resolved.warnings.join("; ")
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_paths_are_reported_without_creating_them() {
        let dir = tempfile::tempdir().unwrap();
        let backend = dir.path().join("missing");
        let db = dir.path().join("missing.db");
        let report = collect(&backend, &db, &BTreeMap::new());
        assert_eq!(report.status, DiagnosticStatus::Error);
        assert!(!report.runtime_ready);
        assert!(!backend.exists());
        assert!(!db.exists());
    }

    #[test]
    fn missing_optional_bundled_vod_directory_is_informational() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backend").join("vod");
        let row = optional_directory(
            "Bundled VOD tools directory (optional)",
            &path,
            "Not present; configured paths, PATH, and common install locations are still searched",
        );
        assert_eq!(row.status, DiagnosticStatus::Ok);
        assert!(row.detail.contains("Not present"));
        assert!(!path.exists());
    }

    #[test]
    fn discovery_warnings_and_missing_tools_are_not_ok() {
        let mut resolution = ToolResolution {
            kind: ToolKind::Ffmpeg,
            path: None,
            source: "missing".into(),
            warnings: vec![],
        };
        assert_eq!(tool_item(&resolution).status, DiagnosticStatus::Warning);
        resolution.path = Some("ffmpeg.exe".into());
        assert_eq!(tool_item(&resolution).status, DiagnosticStatus::Ok);
        resolution
            .warnings
            .push("configured path missing; using fallback".into());
        assert_eq!(tool_item(&resolution).status, DiagnosticStatus::Warning);
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
        assert!(
            report
                .items
                .iter()
                .any(|item| item.name == "Backup directory")
        );
    }

    #[test]
    fn aggregate_and_serialization_preserve_state() {
        let report =
            DiagnosticsSnapshot::from_items(vec![item("test", DiagnosticStatus::Ok, "ready")]);
        assert!(report.runtime_ready);
        let json = serde_json::to_string(&report).unwrap();
        let decoded: DiagnosticsSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.status, DiagnosticStatus::Ok);
        let warning = DiagnosticsSnapshot::from_items(vec![item(
            "test",
            DiagnosticStatus::Warning,
            "attention",
        )]);
        assert!(!warning.runtime_ready);
        let failed = DiagnosticsSnapshot::unavailable(None, None, "SQLite load failed");
        assert_eq!(failed.status, DiagnosticStatus::Error);
    }
}
