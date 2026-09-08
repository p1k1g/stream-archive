use crate::{
    authorize, internal_error,
    model::{HistoryQuery, HistoryResponse, StorageResponse, StorageVolume},
    AppState, ApiResult,
};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use std::{collections::BTreeMap, env, fs, path::{Path, PathBuf}};

#[derive(Debug, Deserialize)]
pub(crate) struct StorageCheckRequest {
    pub path: String,
}

pub(crate) async fn api_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> ApiResult<Json<HistoryResponse>> {
    authorize(&headers, &state)?;
    Ok(Json(
        state
            .store
            .history_filtered(&query)
            .map_err(internal_error)?,
    ))
}

pub(crate) async fn api_storage(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<StorageResponse>> {
    authorize(&headers, &state)?;
    let safe = state.store.safe_settings().map_err(internal_error)?;
    let channels = state.store.channels().map_err(internal_error)?;
    let threshold_gb = safe
        .get("MIN_FREE_SPACE_GB")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(20.0)
        .max(0.0);

    let mut targets = Vec::new();
    let live_default = safe
        .get("OUTPUT_DIR")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_live_output().to_string());
    targets.push(("LIVE 기본".to_string(), live_default));

    for channel in channels {
        if !channel.outdir.trim().is_empty() {
            targets.push((format!("LIVE · {}", channel.name), channel.outdir));
        }
    }
    if let Some(parent) = state.store.path().parent() {
        targets.push(("SQLite 데이터".to_string(), parent.display().to_string()));
    }

    let volumes = collapse_volumes(targets, threshold_gb);
    let database_size_bytes = database_size(state.store.path());
    Ok(Json(StorageResponse {
        threshold_gb,
        database_size_bytes,
        volumes,
    }))
}

pub(crate) async fn api_storage_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<StorageCheckRequest>,
) -> ApiResult<Json<StorageVolume>> {
    authorize(&headers, &state)?;
    let path = request.path.trim();
    if path.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "path is empty".to_string()));
    }
    let threshold_gb = state
        .store
        .setting_value("MIN_FREE_SPACE_GB")
        .map_err(internal_error)?
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(20.0)
        .max(0.0);
    storage_volume(vec!["VOD 출력".to_string()], vec![path.to_string()], threshold_gb)
        .map(Json)
        .map_err(|err| (StatusCode::BAD_REQUEST, err))
}

fn collapse_volumes(targets: Vec<(String, String)>, threshold_gb: f64) -> Vec<StorageVolume> {
    let mut grouped: BTreeMap<String, StorageVolume> = BTreeMap::new();
    for (role, path) in targets {
        match storage_volume(vec![role.clone()], vec![path.clone()], threshold_gb) {
            Ok(volume) => {
                let key = format!("{}:{}", volume.total_bytes, volume.free_bytes);
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
                    format!("error:{path}"),
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
    let threshold_bytes = (threshold_gb * 1024.0 * 1024.0 * 1024.0) as u64;
    let status = if free_bytes <= threshold_bytes {
        "CRITICAL"
    } else if free_bytes <= threshold_bytes.saturating_mul(2) {
        "WARN"
    } else {
        "OK"
    };
    Ok(StorageVolume {
        roles,
        paths,
        probe_path: probe.display().to_string(),
        total_bytes,
        free_bytes,
        used_percent,
        threshold_gb,
        status: status.to_string(),
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

fn database_size(path: &Path) -> u64 {
    let mut total = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    for suffix in ["-wal", "-shm"] {
        let extra = PathBuf::from(format!("{}{}", path.display(), suffix));
        total = total.saturating_add(fs::metadata(extra).map(|meta| meta.len()).unwrap_or(0));
    }
    total
}

fn default_live_output() -> &'static str {
    if cfg!(windows) { r"C:\SOOP_LIVE" } else { "./SOOP_LIVE" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_existing_parent_is_found() {
        let base = env::current_dir().unwrap();
        let probe = existing_probe_path(&base.join("phase8-does-not-exist").join("child")).unwrap();
        assert!(probe.exists());
    }
}
