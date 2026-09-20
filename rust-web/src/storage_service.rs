use crate::store::Store;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageSnapshot {
    pub threshold_gb: f64,
    pub database_size_bytes: u64,
    pub volumes: Vec<StorageVolume>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageVolume {
    pub roles: Vec<String>,
    pub paths: Vec<String>,
    pub probe_path: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_percent: f64,
    pub threshold_gb: f64,
    pub status: String,
    pub error: Option<String>,
}

pub fn snapshot(store: &Store) -> anyhow::Result<StorageSnapshot> {
    let safe = store.safe_settings()?;
    let channels = store.channels()?;
    let threshold_gb = safe
        .get("MIN_FREE_SPACE_GB")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(20.0)
        .max(0.0);

    let live_default = safe
        .get("OUTPUT_DIR")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_live_output().to_string());

    let mut targets = vec![("LIVE 기본".to_string(), live_default)];
    for channel in channels {
        if !channel.outdir.trim().is_empty() {
            targets.push((
                format!("LIVE · {} · {}", channel.platform, channel.name),
                channel.outdir,
            ));
        }
    }
    if let Some(parent) = store.path().parent() {
        targets.push(("SQLite 데이터".to_string(), parent.display().to_string()));
    }

    Ok(StorageSnapshot {
        threshold_gb,
        database_size_bytes: database_size(store.path()),
        volumes: collapse_volumes(targets, threshold_gb),
    })
}

pub fn check_path(store: &Store, path: &str, role: &str) -> anyhow::Result<StorageVolume> {
    let path = path.trim();
    anyhow::ensure!(!path.is_empty(), "path is empty");
    let threshold_gb = store
        .setting_value("MIN_FREE_SPACE_GB")?
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(20.0)
        .max(0.0);
    storage_volume(
        vec![role.to_string()],
        vec![path.to_string()],
        threshold_gb,
    )
    .map_err(anyhow::Error::msg)
}

pub fn collapse_volumes(
    targets: Vec<(String, String)>,
    threshold_gb: f64,
) -> Vec<StorageVolume> {
    let mut grouped: BTreeMap<String, StorageVolume> = BTreeMap::new();
    for (role, path) in targets {
        match storage_volume(vec![role.clone()], vec![path.clone()], threshold_gb) {
            Ok(volume) => {
                let key = volume_key(Path::new(&volume.probe_path));
                if let Some(existing) = grouped.get_mut(&key) {
                    if !existing.roles.contains(&role) {
                        existing.roles.push(role);
                    }
                    if !existing.paths.contains(&path) {
                        existing.paths.push(path);
                    }
                } else {
                    grouped.insert(key, volume);
                }
            }
            Err(error) => {
                grouped.insert(
                    format!("error:{}", path.to_ascii_lowercase()),
                    StorageVolume {
                        roles: vec![role],
                        paths: vec![path],
                        probe_path: String::new(),
                        total_bytes: 0,
                        free_bytes: 0,
                        used_percent: 0.0,
                        threshold_gb,
                        status: "ERROR".to_string(),
                        error: Some(error),
                    },
                );
            }
        }
    }
    grouped.into_values().collect()
}

pub fn classify_status(free_bytes: u64, threshold_gb: f64) -> &'static str {
    let threshold_bytes = (threshold_gb.max(0.0) * 1024.0 * 1024.0 * 1024.0) as u64;
    if free_bytes <= threshold_bytes {
        "CRITICAL"
    } else if free_bytes <= threshold_bytes.saturating_mul(2) {
        "WARN"
    } else {
        "OK"
    }
}

pub fn format_bytes_compact(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;
    let value = bytes as f64;
    if value >= TIB {
        format!("{:.2} TiB", value / TIB)
    } else if value >= GIB {
        format!("{:.1} GiB", value / GIB)
    } else if value >= MIB {
        format!("{:.1} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.1} KiB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn storage_volume(
    roles: Vec<String>,
    paths: Vec<String>,
    threshold_gb: f64,
) -> Result<StorageVolume, String> {
    let requested = paths
        .first()
        .ok_or_else(|| "storage path is missing".to_string())?;
    let probe = existing_probe_path(Path::new(requested))?;
    let total_bytes = fs2::total_space(&probe)
        .map_err(|err| format!("total space check failed for {}: {err}", probe.display()))?;
    let free_bytes = fs2::available_space(&probe)
        .map_err(|err| format!("free space check failed for {}: {err}", probe.display()))?;
    let used_percent = if total_bytes == 0 {
        0.0
    } else {
        ((total_bytes.saturating_sub(free_bytes)) as f64 / total_bytes as f64) * 100.0
    };
    Ok(StorageVolume {
        roles,
        paths,
        probe_path: probe.display().to_string(),
        total_bytes,
        free_bytes,
        used_percent,
        threshold_gb,
        status: classify_status(free_bytes, threshold_gb).to_string(),
        error: None,
    })
}

fn existing_probe_path(path: &Path) -> Result<PathBuf, String> {
    let mut current = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map_err(|err| format!("current directory unavailable: {err}"))?
            .join(path)
    };
    if current.is_file() {
        current.pop();
    }
    loop {
        if current.exists() {
            return Ok(current);
        }
        if !current.pop() {
            break;
        }
    }
    Err(format!("no existing parent found for {}", path.display()))
}

fn volume_key(path: &Path) -> String {
    // Windows native packaging is the primary UI target for this service.
    // A drive/root component is stable for the same Windows volume. Unix keeps
    // the previous root-component behavior for headless compatibility without
    // introducing an OS-specific dependency solely for presentation grouping.
    path.components()
        .next()
        .map(|component| component.as_os_str().to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| path.display().to_string().to_ascii_lowercase())
}

fn database_size(path: &Path) -> u64 {
    let mut total = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    for suffix in ["-wal", "-shm"] {
        let extra = PathBuf::from(format!("{}{}", path.display(), suffix));
        total = total.saturating_add(fs::metadata(extra).map(|meta| meta.len()).unwrap_or(0));
    }
    total
}

fn default_live_output() -> &'static str {
    if cfg!(windows) {
        r"C:\SOOP_LIVE"
    } else {
        "./SOOP_LIVE"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_thresholds_match_web_semantics() {
        let gib = 1024_u64 * 1024 * 1024;
        assert_eq!(classify_status(20 * gib, 20.0), "CRITICAL");
        assert_eq!(classify_status(30 * gib, 20.0), "WARN");
        assert_eq!(classify_status(40 * gib, 20.0), "WARN");
        assert_eq!(classify_status(41 * gib, 20.0), "OK");
    }

    #[test]
    fn compact_bytes_formats_gib_and_tib() {
        let gib = 1024_u64 * 1024 * 1024;
        assert_eq!(format_bytes_compact(640 * gib), "640.0 GiB");
        assert_eq!(format_bytes_compact(2 * 1024 * gib), "2.00 TiB");
    }

    #[test]
    fn same_volume_targets_are_collapsed() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("one");
        let second = temp.path().join("two");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let volumes = collapse_volumes(
            vec![
                ("LIVE 기본".into(), first.display().to_string()),
                ("LIVE · test".into(), second.display().to_string()),
            ],
            1.0,
        );
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].roles.len(), 2);
        assert_eq!(volumes[0].paths.len(), 2);
    }

    #[test]
    fn nonexistent_child_uses_nearest_existing_parent() {
        let temp = tempfile::tempdir().unwrap();
        let probe = existing_probe_path(&temp.path().join("missing").join("child")).unwrap();
        assert_eq!(probe, temp.path());
    }

    #[test]
    fn inaccessible_or_unrooted_path_reports_error() {
        #[cfg(windows)]
        let path = Path::new(r"Z:\stream-archive-definitely-missing\child");
        #[cfg(not(windows))]
        let path = Path::new("/stream-archive-definitely-missing/child");
        let result = existing_probe_path(path);
        if path.ancestors().any(|ancestor| ancestor.exists()) {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
    }
}
