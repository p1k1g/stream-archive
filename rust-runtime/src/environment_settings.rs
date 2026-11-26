//! Editable environment settings over the existing SQLite keys.
//! This service has no presentation or native-dialog dependencies.
use crate::{
    primary_config::{VOD_TOOL_KEYS, validate_setting_updates, validate_vod_tool_updates},
    tool_discovery::executable_file,
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingKind {
    Text,
    Executable,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentSetting {
    pub key: String,
    pub value: String,
    pub description: String,
    pub kind: SettingKind,
}

const FIELDS: &[(&str, SettingKind, &str)] = &[
    (
        "STREAMLINK_PATH",
        SettingKind::Executable,
        "Streamlink executable; blank or AUTO uses discovery",
    ),
    (
        "STREAMLINK_FALLBACK",
        SettingKind::Executable,
        "Fallback Streamlink executable; blank or AUTO uses discovery",
    ),
    (
        "YT_DLP_PATH",
        SettingKind::Executable,
        "yt-dlp executable; blank uses runtime discovery",
    ),
    (
        "FFMPEG_PATH",
        SettingKind::Executable,
        "FFmpeg executable; blank uses runtime discovery",
    ),
    (
        "OUTPUT_DIR",
        SettingKind::Directory,
        "Default LIVE output folder; blank keeps the runtime default",
    ),
    (
        "CHECK_INTERVAL",
        SettingKind::Text,
        "LIVE polling interval in seconds (1–86400)",
    ),
    (
        "MIN_FREE_SPACE_GB",
        SettingKind::Text,
        "Minimum free disk space in GB (0–1000000)",
    ),
    (
        "STREAM_ARCHIVE_CLOSE_ACTION",
        SettingKind::Text,
        "Windows close action: EXIT (default), TRAY or ASK; Unix headless is unaffected",
    ),
    (
        "STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS",
        SettingKind::Text,
        "Windows VOD 완료·실패 알림: true / false; 변경사항 저장 후 적용",
    ),
    (
        "GUI_NOTIFY_RECORD_START",
        SettingKind::Text,
        "Windows LIVE 녹화 시작 알림: Y / N; 변경사항 저장 후 적용",
    ),
    (
        "GUI_NOTIFY_RECORD_FINISH",
        SettingKind::Text,
        "Windows LIVE 녹화 완료 알림: Y / N; 변경사항 저장 후 적용",
    ),
    (
        "GUI_NOTIFY_WARNING",
        SettingKind::Text,
        "Windows 디스크 부족·채널 오류 경고 알림: Y / N; 변경사항 저장 후 적용",
    ),
    (
        "GUI_AUTO_START_WATCHER",
        SettingKind::Text,
        "앱 시작 시 LIVE 감시 자동 시작: Y / N; 변경사항 저장 후 다음 실행부터 적용",
    ),
    (
        "GUI_START_MINIMIZED_TRAY",
        SettingKind::Text,
        "앱 시작 시 메인 창을 열지 않고 시스템 트레이로 최소화 시작: Y / N",
    ),
    (
        "QUALITY",
        SettingKind::Text,
        "LIVE quality, for example best",
    ),
    (
        "FILE_NAME_PATTERN",
        SettingKind::Text,
        "LIVE recording filename pattern: LEGACY, TIME_TITLE, BJ_TITLE or TITLE_NUMBER",
    ),
    (
        "RECORD_STALL_TIMEOUT",
        SettingKind::Text,
        "LIVE recording stall timeout in seconds (10–86400)",
    ),
    (
        "RECORD_RETRY_INTERVAL",
        SettingKind::Text,
        "LIVE recording retry interval in seconds (1–3600)",
    ),
    (
        "LOG_ENABLED",
        SettingKind::Text,
        "Persist daily runtime log files: Y / N; applies on next restart",
    ),
    (
        "LOG_RETENTION_DAYS",
        SettingKind::Text,
        "Runtime log file retention in days (0–36500, 0 keeps forever)",
    ),
];

pub fn snapshot(values: &BTreeMap<String, String>) -> Vec<EnvironmentSetting> {
    FIELDS
        .iter()
        .map(|(key, kind, description)| EnvironmentSetting {
            key: (*key).into(),
            value: values.get(*key).cloned().unwrap_or_default(),
            description: (*description).into(),
            kind: *kind,
        })
        .collect()
}

/// Validate only changed keys. Existing persisted paths are not rewritten or
/// rejected merely by opening/saving an unrelated native field.
/// Explicit new paths must exist; blank/AUTO retain existing discovery rules.
pub fn validate_updates(updates: &BTreeMap<String, String>) -> Result<()> {
    let mut general = BTreeMap::new();
    let mut vod = BTreeMap::new();
    for (key, value) in updates {
        let Some((_, kind, _)) = FIELDS.iter().find(|(name, _, _)| *name == key) else {
            bail!("environment setting is not editable: {key}");
        };
        if VOD_TOOL_KEYS.contains(&key.as_str()) {
            vod.insert(key.clone(), value.clone());
        } else {
            general.insert(key.clone(), value.clone());
        }
        if value.contains(['\0', '\r', '\n']) || value.len() > 2048 {
            bail!("{key} must be a single line of at most 2048 bytes");
        }
        let automatic = value.trim().is_empty()
            || (key.starts_with("STREAMLINK_") && value.eq_ignore_ascii_case("AUTO"));
        if !automatic && *kind != SettingKind::Text {
            let path = Path::new(value);
            if !path.is_absolute() {
                bail!("{key} must be an absolute path (or use the documented automatic value)");
            }
            match kind {
                SettingKind::Executable if !executable_file(path) => {
                    bail!("{key} is missing or is not an executable file: {value}");
                }
                SettingKind::Directory if !path.is_dir() => {
                    bail!("{key} must be an existing directory: {value}");
                }
                _ => {}
            }
            #[cfg(windows)]
            if *kind == SettingKind::Executable {
                let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                if !extension.eq_ignore_ascii_case("exe") && !extension.eq_ignore_ascii_case("com")
                {
                    bail!("{key} must select a Windows .exe or .com executable");
                }
            }
        }
    }
    validate_setting_updates(&general)?;
    validate_vod_tool_updates(&vod)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(key: &str, value: &str) -> BTreeMap<String, String> {
        BTreeMap::from([(key.into(), value.into())])
    }

    #[test]
    fn accepts_runtime_defaults_and_valid_numeric_settings() {
        for (key, value) in [
            ("STREAMLINK_PATH", "AUTO"),
            ("YT_DLP_PATH", ""),
            ("OUTPUT_DIR", ""),
            ("CHECK_INTERVAL", "30"),
            ("MIN_FREE_SPACE_GB", "1.5"),
        ] {
            validate_updates(&update(key, value)).unwrap();
        }
        assert!(validate_updates(&update("CHECK_INTERVAL", "0")).is_err());
        assert!(validate_updates(&update("MIN_FREE_SPACE_GB", "NaN")).is_err());
    }

    #[test]
    fn windows_close_action_accepts_only_explicit_exit_tray_or_ask() {
        validate_updates(&update("STREAM_ARCHIVE_CLOSE_ACTION", "EXIT")).unwrap();
        validate_updates(&update("STREAM_ARCHIVE_CLOSE_ACTION", "TRAY")).unwrap();
        validate_updates(&update("STREAM_ARCHIVE_CLOSE_ACTION", "ASK")).unwrap();
        for value in ["", "tray", "MINIMIZE", "TRAY\nEXIT"] {
            assert!(validate_updates(&update("STREAM_ARCHIVE_CLOSE_ACTION", value)).is_err());
        }
    }

    #[test]
    fn download_notifications_accepts_only_explicit_booleans() {
        for value in ["true", "false"] {
            validate_updates(&update("STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS", value)).unwrap();
        }
        for value in ["", "TRUE", "1", "false\ntrue"] {
            assert!(
                validate_updates(&update("STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS", value)).is_err()
            );
        }
    }

    #[test]
    fn live_and_warning_notifications_accept_only_y_or_n() {
        for key in [
            "GUI_NOTIFY_RECORD_START",
            "GUI_NOTIFY_RECORD_FINISH",
            "GUI_NOTIFY_WARNING",
            "GUI_AUTO_START_WATCHER",
            "GUI_START_MINIMIZED_TRAY",
            "LOG_ENABLED",
        ] {
            for value in ["Y", "N", "y", "n"] {
                validate_updates(&update(key, value)).unwrap();
            }
            for value in ["", "true", "YES", "1", "Y\nN"] {
                assert!(validate_updates(&update(key, value)).is_err());
            }
        }
    }

    #[test]
    fn advanced_numeric_settings_validate_bounds() {
        validate_updates(&update("RECORD_STALL_TIMEOUT", "90")).unwrap();
        validate_updates(&update("RECORD_STALL_TIMEOUT", "10")).unwrap();
        assert!(validate_updates(&update("RECORD_STALL_TIMEOUT", "9")).is_err());

        validate_updates(&update("RECORD_RETRY_INTERVAL", "5")).unwrap();
        assert!(validate_updates(&update("RECORD_RETRY_INTERVAL", "0")).is_err());

        validate_updates(&update("LOG_RETENTION_DAYS", "0")).unwrap();
        validate_updates(&update("LOG_RETENTION_DAYS", "30")).unwrap();
        assert!(validate_updates(&update("LOG_RETENTION_DAYS", "-1")).is_err());
    }

    #[test]
    fn file_name_pattern_accepts_only_known_presets() {
        for value in [
            "LEGACY",
            "TIME_TITLE",
            "BJ_TITLE",
            "TITLE_NUMBER",
            "bj_title",
        ] {
            validate_updates(&update("FILE_NAME_PATTERN", value)).unwrap();
        }
        for value in ["", "CUSTOM", "../bad", "LEGACY\nTIME_TITLE"] {
            assert!(validate_updates(&update("FILE_NAME_PATTERN", value)).is_err());
        }
    }

    #[test]
    fn rejects_invalid_missing_and_wrong_kind_paths() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-directory.txt");
        std::fs::write(&file, b"test").unwrap();
        for (key, path) in [
            ("OUTPUT_DIR", file),
            ("FFMPEG_PATH", dir.path().to_path_buf()),
            ("OUTPUT_DIR", dir.path().join("missing")),
            ("FFMPEG_PATH", dir.path().join("missing.exe")),
        ] {
            assert!(validate_updates(&update(key, &path.to_string_lossy())).is_err());
        }
        assert!(validate_updates(&update("OUTPUT_DIR", "relative")).is_err());
        assert!(validate_updates(&update("FFMPEG_PATH", "bad\0path")).is_err());
        assert!(validate_updates(&update("SOOP_PASSWORD", "secret")).is_err());
    }

    #[test]
    fn accepts_existing_directory_and_executable() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tool.exe");
        std::fs::write(&file, b"test").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        validate_updates(&update("FFMPEG_PATH", &file.to_string_lossy())).unwrap();
        validate_updates(&update("OUTPUT_DIR", &dir.path().to_string_lossy())).unwrap();
    }

    #[test]
    fn serialization_exposes_only_editable_nonsecret_fields() {
        let fields = snapshot(&update("SOOP_PASSWORD", "never-export"));
        let json = serde_json::to_string(&fields).unwrap();
        let decoded: Vec<EnvironmentSetting> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.len(), FIELDS.len());
        assert!(!json.contains("never-export"));
    }
}
