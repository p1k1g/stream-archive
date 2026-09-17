use crate::{
    ApiResult, AppState, authorize, internal_error,
    model::{VodDownloadRequest, VodQueueItem, VodQueueSnapshot},
    primary_config::apply_vod_tool_defaults,
};
use axum::{
    Json,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};

#[path = "queue_service.rs"]
mod shared_queue;
pub use shared_queue::VodQueueManager;

pub(crate) async fn api_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<VodQueueSnapshot>> {
    authorize(&headers, &state)?;
    Ok(Json(
        state.vod_queue.snapshot().await.map_err(internal_error)?,
    ))
}

pub(crate) async fn api_enqueue(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut req): Json<VodDownloadRequest>,
) -> ApiResult<Json<VodQueueItem>> {
    authorize(&headers, &state)?;
    let _config_guard = state.config_write_lock.lock().await;
    let tools = state.store.vod_tool_settings().map_err(internal_error)?;
    apply_vod_tool_defaults(&tools, &mut req.yt_dlp_path, &mut req.ffmpeg_path);
    let item = state
        .vod_queue
        .enqueue(req)
        .await
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(item))
}

pub(crate) async fn api_cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let _lifecycle_guard = state.lifecycle_lock.lock().await;
    let _config_guard = state.config_write_lock.lock().await;
    state
        .vod_queue
        .cancel(&id)
        .await
        .map_err(|err| (StatusCode::CONFLICT, err.to_string()))?;
    Ok(Json(json!({"ok":true})))
}

pub(crate) async fn api_retry(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let _config_guard = state.config_write_lock.lock().await;
    state
        .vod_queue
        .retry(&id)
        .await
        .map_err(|err| (StatusCode::CONFLICT, err.to_string()))?;
    Ok(Json(json!({"ok":true})))
}

pub(crate) async fn api_remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let _config_guard = state.config_write_lock.lock().await;
    state
        .vod_queue
        .remove(&id)
        .await
        .map_err(|err| (StatusCode::CONFLICT, err.to_string()))?;
    Ok(Json(json!({"ok":true})))
}
