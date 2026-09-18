use stream_archive_server::{
    backup_service::{BackupInfo, BackupSnapshot},
    diagnostics::{DiagnosticItem, DiagnosticStatus, DiagnosticsSnapshot},
    history_service::format_history_timestamp_local,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupRowView {
    pub file_name: String,
    pub kind: String,
    pub created_at: String,
    pub size: String,
    pub sha256: String,
    pub integrity: String,
    pub integrity_tone: String,
    pub can_restore: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRowView {
    pub name: String,
    pub status: String,
    pub detail: String,
    pub status_tone: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRowView {
    pub text: String,
}

pub fn backup_rows(snapshot: &BackupSnapshot) -> Vec<BackupRowView> {
    snapshot.backups.iter().map(backup_row).collect()
}

fn backup_row(item: &BackupInfo) -> BackupRowView {
    let integrity = item.integrity.to_ascii_uppercase();
    BackupRowView {
        file_name: item.file_name.clone(),
        kind: item.kind.clone(),
        created_at: format_history_timestamp_local(&item.created_at),
        size: format_bytes(item.size_bytes),
        sha256: item.sha256.clone(),
        integrity_tone: integrity_tone(&integrity).into(),
        can_restore: integrity == "OK",
        integrity,
    }
}

pub fn diagnostic_rows(snapshot: &DiagnosticsSnapshot) -> Vec<DiagnosticRowView> {
    snapshot.items.iter().map(diagnostic_row).collect()
}

fn diagnostic_row(item: &DiagnosticItem) -> DiagnosticRowView {
    DiagnosticRowView {
        name: item.name.clone(),
        status: item.status.label().into(),
        detail: item.detail.clone(),
        status_tone: diagnostic_tone(item.status).into(),
    }
}

pub fn log_rows(lines: Vec<String>, max_lines: usize) -> Vec<LogRowView> {
    let max_lines = max_lines.max(1);
    let start = lines.len().saturating_sub(max_lines);
    lines
        .into_iter()
        .skip(start)
        .map(|line| LogRowView {
            text: line.replace('\r', " ").replace('\n', " "),
        })
        .collect()
}

fn integrity_tone(value: &str) -> &'static str {
    match value {
        "OK" => "ok",
        "NO_METADATA" => "warn",
        _ => "error",
    }
}

fn diagnostic_tone(status: DiagnosticStatus) -> &'static str {
    match status {
        DiagnosticStatus::Ok => "ok",
        DiagnosticStatus::Warning => "warn",
        DiagnosticStatus::Error => "error",
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let value = bytes as f64;
    if value >= GIB {
        format!("{:.2} GiB", value / GIB)
    } else if value >= MIB {
        format!("{:.1} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.1} KiB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_archive_server::{
        backup_service::BackupPolicy,
        diagnostics::DiagnosticStatus,
    };

    #[test]
    fn backup_rows_format_local_time_size_and_integrity() {
        let snapshot = BackupSnapshot {
            directory: "D:/backups".into(),
            directory_editable: true,
            policy: BackupPolicy {
                enabled: true,
                interval_hours: 24,
                keep_count: 10,
                retention_days: 3,
            },
            backups: vec![BackupInfo {
                file_name: "stream_archive_manual_test.db".into(),
                created_at: "2026-09-18T11:00:00Z".into(),
                kind: "manual".into(),
                size_bytes: 1024 * 1024,
                sha256: "abcdef".into(),
                integrity: "OK".into(),
            }],
        };
        let rows = backup_rows(&snapshot);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].size, "1.0 MiB");
        assert_eq!(rows[0].integrity_tone, "ok");
        assert!(rows[0].can_restore);
        assert!(!rows[0].created_at.contains('T'));
    }

    #[test]
    fn invalid_integrity_disables_restore() {
        let item = BackupInfo {
            file_name: "stream_archive_manual_bad.db".into(),
            created_at: "2026-09-18T11:00:00Z".into(),
            kind: "manual".into(),
            size_bytes: 1,
            sha256: "x".into(),
            integrity: "HASH_MISMATCH".into(),
        };
        let row = backup_row(&item);
        assert_eq!(row.integrity_tone, "error");
        assert!(!row.can_restore);
    }

    #[test]
    fn logs_are_bounded_and_flatten_multiline_rows() {
        let rows = log_rows(
            vec![
                "old".into(),
                "first".into(),
                "second\ncontinued".into(),
                "third".into(),
            ],
            3,
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].text, "first");
        assert_eq!(rows[1].text, "second continued");
    }

    #[test]
    fn diagnostic_tones_preserve_severity() {
        assert_eq!(diagnostic_tone(DiagnosticStatus::Ok), "ok");
        assert_eq!(diagnostic_tone(DiagnosticStatus::Warning), "warn");
        assert_eq!(diagnostic_tone(DiagnosticStatus::Error), "error");
    }
}
