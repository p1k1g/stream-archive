use stream_archive_server::storage_service::{StorageSnapshot, StorageVolume, format_bytes_compact};

#[derive(Debug, Clone, PartialEq)]
pub struct StorageRowView {
    pub volume: String,
    pub roles: String,
    pub paths: String,
    pub capacity: String,
    pub used: String,
    pub status: String,
    pub status_tone: String,
    pub detail: String,
}

pub fn rows(snapshot: &StorageSnapshot) -> Vec<StorageRowView> {
    snapshot.volumes.iter().map(row).collect()
}

fn row(volume: &StorageVolume) -> StorageRowView {
    let error = volume.error.clone().unwrap_or_default();
    let volume_label = if volume.probe_path.is_empty() {
        "확인 불가".to_string()
    } else {
        root_label(&volume.probe_path)
    };
    let (status, status_tone) = match volume.status.as_str() {
        "OK" => ("정상", "ok"),
        "WARN" => ("주의", "warn"),
        "CRITICAL" => ("공간 부족", "error"),
        _ => ("확인 불가", "error"),
    };
    let capacity = if volume.total_bytes == 0 {
        "-".to_string()
    } else {
        let used = volume.total_bytes.saturating_sub(volume.free_bytes);
        format!(
            "사용 {} · 남음 {} · 전체 {}",
            format_bytes_compact(used),
            format_bytes_compact(volume.free_bytes),
            format_bytes_compact(volume.total_bytes)
        )
    };
    StorageRowView {
        volume: volume_label,
        roles: volume.roles.join(" · "),
        paths: volume.paths.join(" · "),
        capacity,
        used: if volume.total_bytes == 0 {
            "-".into()
        } else {
            format!("{:.0}%", volume.used_percent)
        },
        status: status.into(),
        status_tone: status_tone.into(),
        detail: if error.is_empty() {
            volume.paths.join(" · ")
        } else {
            error
        },
    }
}

fn root_label(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        if bytes.len() >= 3 && matches!(bytes[2], b'\\' | b'/') {
            return path[..3].to_string();
        }
        return path[..2].to_string();
    }
    if path.starts_with('/') {
        return "/".into();
    }
    path.to_string()
}

pub fn threshold_label(threshold_gb: f64) -> String {
    if threshold_gb.fract() == 0.0 {
        format!("{threshold_gb:.0} GiB")
    } else {
        format!("{threshold_gb:.1} GiB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_storage_row_status_and_capacity() {
        let gib = 1024_u64 * 1024 * 1024;
        let snapshot = StorageSnapshot {
            threshold_gb: 20.0,
            database_size_bytes: 2 * 1024 * 1024,
            volumes: vec![StorageVolume {
                roles: vec!["LIVE 기본".into()],
                paths: vec![r"D:\LIVE".into()],
                probe_path: r"D:\LIVE".into(),
                total_bytes: 2 * 1024 * gib,
                free_bytes: 640 * gib,
                used_percent: 68.75,
                threshold_gb: 20.0,
                status: "OK".into(),
                error: None,
            }],
        };
        let rows = rows(&snapshot);
        assert_eq!(rows[0].volume, r"D:\");
        assert_eq!(rows[0].status, "정상");
        assert_eq!(rows[0].used, "69%");
        assert!(rows[0].capacity.contains("640.0 GiB"));
    }

    #[test]
    fn error_volume_stays_visible() {
        let row = row(&StorageVolume {
            roles: vec!["LIVE 기본".into()],
            paths: vec!["missing".into()],
            probe_path: String::new(),
            total_bytes: 0,
            free_bytes: 0,
            used_percent: 0.0,
            threshold_gb: 20.0,
            status: "ERROR".into(),
            error: Some("no existing parent".into()),
        });
        assert_eq!(row.status, "확인 불가");
        assert_eq!(row.status_tone, "error");
        assert!(row.detail.contains("no existing parent"));
    }
}
