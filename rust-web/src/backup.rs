use crate::{ApiResult, AppState, authorize, backup_service::BackupInfo, internal_error};
use axum::{
    Json,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};

pub(crate) async fn api_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let snapshot = state.backups.snapshot().await.map_err(internal_error)?;
    Ok(Json(json!({
        "directory": snapshot.directory,
        "directory_editable": snapshot.directory_editable,
        "policy": snapshot.policy,
        "backups": snapshot.backups
    })))
}

pub(crate) async fn api_create(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<BackupInfo>> {
    authorize(&headers, &state)?;
    let item = state
        .backups
        .create_manual()
        .await
        .map_err(internal_error)?;
    state
        .logs
        .push(format!(
            "[BACKUP] manual backup created file={} size={} sha256={}",
            item.file_name, item.size_bytes, item.sha256
        ))
        .await;
    Ok(Json(item))
}

pub(crate) async fn api_restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(file_name): AxumPath<String>,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let _lifecycle_guard = state.lifecycle_lock.lock().await;
    let _config_guard = state.config_write_lock.lock().await;
    let watcher = state.watcher.status().await.map_err(internal_error)?;
    if watcher.running || watcher.recording_count > 0 {
        return Err((
            StatusCode::CONFLICT,
            "Watcher를 중지한 뒤 복원하세요.".into(),
        ));
    }
    if state.vod.status().await.running {
        return Err((
            StatusCode::CONFLICT,
            "VOD 작업을 중지한 뒤 복원하세요.".into(),
        ));
    }
    if state
        .vod_queue
        .has_pending_or_active()
        .await
        .map_err(internal_error)?
    {
        return Err((
            StatusCode::CONFLICT,
            "VOD 다운로드 큐를 비운 뒤 복원하세요.".into(),
        ));
    }

    let outcome = state
        .backups
        .restore(&file_name)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;

    state.auth.reinitialize().map_err(internal_error)?;
    let invalidated = state
        .auth
        .invalidate_all_sessions()
        .map_err(internal_error)?;

    state
        .logs
        .push(format!(
            "[BACKUP] database restored file={} safety={} sessions_invalidated={invalidated}",
            outcome.restored.file_name, outcome.safety_backup.file_name
        ))
        .await;

    Ok(Json(json!({
        "ok": true,
        "restored": outcome.restored,
        "safety_backup": outcome.safety_backup,
        "sessions_invalidated": invalidated,
        "reauthenticate": true
    })))
}
