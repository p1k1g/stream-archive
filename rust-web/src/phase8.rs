use crate::{
    authorize, internal_error,
    model::{HistoryResponse, LiveHistoryItem, VodHistoryItem},
    AppState, ApiResult,
};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct HistoryQuery {
    pub q: Option<String>,
    pub status: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StorageResponse {
    pub threshold_gb: f64,
    pub database_size_bytes: u64,
    pub volumes: Vec<StorageVolume>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StorageVolume {
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
    let mut history = load_history_compat(state.store.path(), 500).map_err(internal_error)?;
    let needle = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let status = query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_uppercase);
    let from = normalized_date(query.from.as_deref());
    let to = normalized_date(query.to.as_deref());
    let limit = query.limit.unwrap_or(100).clamp(1, 500);

    history.live.retain(|item| {
        text_matches_live(item, needle.as_deref())
            && status
                .as_deref()
                .is_none_or(|wanted| item.status.eq_ignore_ascii_case(wanted))
            && date_matches(&item.started_at, from.as_deref(), to.as_deref())
    });
    history.vod.retain(|item| {
        text_matches_vod(item, needle.as_deref())
            && status
                .as_deref()
                .is_none_or(|wanted| item.state.eq_ignore_ascii_case(wanted))
            && item
                .started_at
                .as_deref()
                .is_none_or(|started| date_matches(started, from.as_deref(), to.as_deref()))
    });
    history.live.truncate(limit);
    history.vod.truncate(limit);
    Ok(Json(history))
}

fn load_history_compat(path: &Path, limit: usize) -> rusqlite::Result<HistoryResponse> {
    let limit = limit.clamp(1, 500) as i64;
    let conn = Connection::open(path)?;

    let mut live_stmt = conn.prepare(
        "SELECT id,account,channel_name,bno,title,file_path,started_at,ended_at,duration_seconds,size_bytes,reason,status FROM live_recordings ORDER BY started_at DESC LIMIT ?1",
    )?;
    let live = live_stmt
        .query_map(params![limit], |row| {
            Ok(LiveHistoryItem {
                id: row.get(0)?,
                account: row.get(1)?,
                channel_name: row.get(2)?,
                bno: row.get(3)?,
                title: row.get(4)?,
                file_path: row.get(5)?,
                started_at: row.get(6)?,
                ended_at: row.get(7)?,
                duration_seconds: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                size_bytes: row.get::<_, Option<i64>>(9)?.unwrap_or(0).max(0) as u64,
                reason: row.get(10)?,
                status: row.get::<_, Option<String>>(11)?.unwrap_or_else(|| "UNKNOWN".to_string()),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut vod_stmt = conn.prepare(
        "SELECT id,COALESCE(kind,'JOB'),COALESCE(vod_url,''),COALESCE(title,''),COALESCE(streamer,''),COALESCE(part_count,0),COALESCE(state,'UNKNOWN'),output_file,COALESCE(message,''),started_at,finished_at FROM vod_jobs ORDER BY COALESCE(started_at,updated_at) DESC LIMIT ?1",
    )?;
    let vod = vod_stmt
        .query_map(params![limit], |row| {
            Ok(VodHistoryItem {
                id: row.get(0)?,
                kind: row.get(1)?,
                vod_url: row.get(2)?,
                title: row.get(3)?,
                streamer: row.get(4)?,
                part_count: row.get::<_, i64>(5)?.max(0) as usize,
                state: row.get(6)?,
                output_file: row.get(7)?,
                message: row.get(8)?,
                started_at: row.get(9)?,
                finished_at: row.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(HistoryResponse { live, vod })
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

    let live_default = safe
        .get("OUTPUT_DIR")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_live_output().to_string());
    let mut targets = vec![("LIVE 기본".to_string(), live_default)];
    for channel in channels {
        if !channel.outdir.trim().is_empty() {
            targets.push((format!("LIVE · {}", channel.name), channel.outdir));
        }
    }
    if let Some(parent) = state.store.path().parent() {
        targets.push(("SQLite 데이터".to_string(), parent.display().to_string()));
    }

    Ok(Json(StorageResponse {
        threshold_gb,
        database_size_bytes: database_size(state.store.path()),
        volumes: collapse_volumes(targets, threshold_gb),
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
        .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

fn text_matches_live(item: &LiveHistoryItem, needle: Option<&str>) -> bool {
    let Some(needle) = needle else { return true; };
    [
        Some(item.account.as_str()),
        Some(item.channel_name.as_str()),
        item.title.as_deref(),
        item.file_path.as_deref(),
        item.reason.as_deref(),
        Some(item.status.as_str()),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_ascii_lowercase().contains(needle))
}

fn text_matches_vod(item: &VodHistoryItem, needle: Option<&str>) -> bool {
    let Some(needle) = needle else { return true; };
    [
        Some(item.kind.as_str()),
        Some(item.vod_url.as_str()),
        Some(item.title.as_str()),
        Some(item.streamer.as_str()),
        item.output_file.as_deref(),
        Some(item.message.as_str()),
        Some(item.state.as_str()),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_ascii_lowercase().contains(needle))
}

fn normalized_date(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| value.len() >= 10)
        .map(|value| value[..10].to_string())
}

fn date_matches(timestamp: &str, from: Option<&str>, to: Option<&str>) -> bool {
    let date = timestamp.get(..10).unwrap_or(timestamp);
    from.is_none_or(|min| date >= min) && to.is_none_or(|max| date <= max)
}

fn collapse_volumes(targets: Vec<(String, String)>, threshold_gb: f64) -> Vec<StorageVolume> {
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

fn volume_key(path: &Path) -> String {
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

    #[test]
    fn history_date_filter_uses_calendar_date() {
        assert!(date_matches("2026-09-08T03:00:00Z", Some("2026-09-08"), Some("2026-09-08")));
        assert!(!date_matches("2026-09-07T23:59:59Z", Some("2026-09-08"), None));
    }
}