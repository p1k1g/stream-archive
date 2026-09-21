use crate::{
    ApiResult, AppState, authorize, internal_error,
    storage_service::{self, StorageSnapshot, StorageVolume},
};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;

use crate::history_service::{self, HistoryFilter};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct HistoryQuery {
    pub q: Option<String>,
    pub status: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StorageCheckRequest {
    pub path: String,
}

pub(crate) async fn api_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> ApiResult<Json<crate::model::HistoryResponse>> {
    authorize(&headers, &state)?;
    let filter = HistoryFilter {
        q: query.q,
        status: query.status,
        from: query.from,
        to: query.to,
        limit: query.limit,
    };
    history_service::load_history(state.store.path(), &filter)
        .map(Json)
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))
}

pub(crate) async fn api_storage(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<StorageSnapshot>> {
    authorize(&headers, &state)?;
    storage_service::snapshot(&state.store)
        .map(Json)
        .map_err(internal_error)
}

pub(crate) async fn api_storage_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<StorageCheckRequest>,
) -> ApiResult<Json<StorageVolume>> {
    authorize(&headers, &state)?;
    storage_service::check_path(&state.store, &request.path, "VOD 출력")
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))
}
