use crate::formatting::format_bytes;
use stream_archive_server::{
    backup_service::{BackupInfo, BackupSnapshot},
    diagnostics::{
        DiagnosticCategory, DiagnosticItem, DiagnosticRequirement, DiagnosticStatus,
        DiagnosticsSnapshot,
    },
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
    pub category: String,
    pub requirement: String,
    pub name: String,
    pub status: String,
    pub summary: String,
    pub detail: String,
    pub remediation: String,
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
        kind: backup_kind_label(&item.kind).into(),
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
        category: diagnostic_category_label(item.category).into(),
        requirement: diagnostic_requirement_label(item.requirement).into(),
        name: localized_name(item),
        status: diagnostic_status_label(item.status).into(),
        summary: localized_summary(item),
        detail: localized_detail(item),
        remediation: crate::diagnostic_text::text(&item.remediation),
        status_tone: diagnostic_tone(item.status).into(),
    }
}

fn localized_name(item: &DiagnosticItem) -> String {
    if item.id.starts_with("tool.")
        && let Some(name) = item.name.strip_suffix(" executable")
    {
        return format!("{name} 실행 파일");
    }
    crate::diagnostic_text::text(&item.name)
}

fn localized_summary(item: &DiagnosticItem) -> String {
    if item.id.starts_with("tool.")
        && let Some(version) = item.summary.strip_prefix("Executable probe passed (")
    {
        return format!("실행 파일 확인 성공 ({version}");
    }
    crate::diagnostic_text::text(&item.summary)
}

fn localized_detail(item: &DiagnosticItem) -> String {
    // Translate only known wrappers, never arbitrary probe output or filesystem errors.
    let detail = crate::diagnostic_text::text(&item.detail);
    if item.id.starts_with("provider.") {
        return detail
            .replace("not configured", "미설정")
            .replace("configured", "설정됨");
    }
    for (suffix, translated) in [
        (" — exists", " — 존재함"),
        (" — missing or not a directory", " — 없거나 디렉터리가 아님"),
        (
            " — missing or not a regular file",
            " — 없거나 일반 파일이 아님",
        ),
        (
            "; Secret Service session is not probed",
            "; Secret Service 세션은 검사하지 않음",
        ),
    ] {
        if let Some(path) = detail.strip_suffix(suffix) {
            return format!("{path}{translated}");
        }
    }
    if item.id.starts_with("tool.") {
        for (wrapper, translated) in [
            ("; active local version probe: ", "; 로컬 버전 실행 확인: "),
            ("; active probe error: ", "; 실행 확인 오류: "),
            (
                "; passive filesystem discovery only; active local version probing is opt-in. ",
                "; 파일 탐색만 수행함. 로컬 버전 실행 확인은 별도 요청 시 수행함. ",
            ),
        ] {
            if let Some((path, raw)) = detail.split_once(wrapper) {
                return format!("{path}{translated}{raw}");
            }
        }
    }
    detail
}

pub fn log_rows(lines: Vec<String>, max_lines: usize) -> Vec<LogRowView> {
    let max_lines = max_lines.max(1);
    let start = lines.len().saturating_sub(max_lines);
    lines
        .into_iter()
        .skip(start)
        .map(|line| LogRowView {
            text: line.replace(['\r', '\n'], " "),
        })
        .collect()
}

fn backup_kind_label(kind: &str) -> &str {
    match kind {
        "manual" => "수동",
        "automatic" | "auto" => "자동",
        "pre_restore" => "복원 전 안전 백업",
        other => other,
    }
}

fn diagnostic_status_label(status: DiagnosticStatus) -> &'static str {
    match status {
        DiagnosticStatus::Ok => "정상",
        DiagnosticStatus::Warning => "주의",
        DiagnosticStatus::Error => "오류",
    }
}

fn diagnostic_category_label(category: DiagnosticCategory) -> &'static str {
    match category {
        DiagnosticCategory::Runtime => "런타임",
        DiagnosticCategory::Database => "데이터베이스",
        DiagnosticCategory::Storage => "저장소",
        DiagnosticCategory::Tools => "도구",
        DiagnosticCategory::Secrets => "보안 저장소",
        DiagnosticCategory::Providers => "공급자",
        DiagnosticCategory::Backup => "백업",
    }
}

fn diagnostic_requirement_label(requirement: DiagnosticRequirement) -> &'static str {
    match requirement {
        DiagnosticRequirement::Required => "필수",
        DiagnosticRequirement::Optional => "선택",
        DiagnosticRequirement::Informational => "정보",
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use stream_archive_server::{backup_service::BackupPolicy, diagnostics::DiagnosticStatus};

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
    #[test]
    fn diagnostic_translation_preserves_probe_output_errors_and_classification() {
        let mut item = DiagnosticItem {
            id: "tool.streamlink".into(), category: DiagnosticCategory::Tools,
            requirement: DiagnosticRequirement::Required,
            name: "streamlink executable".into(), status: DiagnosticStatus::Error,
            summary: "Executable probe could not complete".into(),
            detail: "D:/Tools/streamlink.exe [PATH]; active probe error: Directory is unavailable".into(),
            remediation: "Verify the executable path and local process permissions, then retry the active probe.".into(),
        };
        let row = diagnostic_row(&item);
        assert_eq!(row.name, "streamlink 실행 파일");
        assert_eq!(row.summary, "실행 파일 확인을 완료하지 못했습니다");
        assert_eq!(
            row.detail,
            "D:/Tools/streamlink.exe [PATH]; 실행 확인 오류: Directory is unavailable"
        );
        assert_eq!(row.status_tone, "error");
        assert_eq!(row.requirement, "필수");
        item.id = "unknown".into();
        item.name = "Future diagnostic".into();
        item.detail = "Unrecognized error: configured".into();
        let row = diagnostic_row(&item);
        assert_eq!(row.name, item.name);
        assert_eq!(row.detail, item.detail);
    }
}
