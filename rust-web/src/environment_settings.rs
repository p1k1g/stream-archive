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
        "QUALITY",
        SettingKind::Text,
        "LIVE quality, for example best",
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

/// Validate only changed keys. Existing paths loaded from Web/CLI are not
/// rewritten or rejected merely by opening/saving an unrelated native field.
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
