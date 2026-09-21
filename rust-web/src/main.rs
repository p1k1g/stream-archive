mod auth;
mod backup;
mod history_storage;
mod local_picker;
mod realtime;
mod vod_queue;

use stream_archive_server::{
    backend, backup_service, history_service, model, native_watcher, platform_runtime,
    primary_config, queue_service, recorder, security, storage_service, store, support, vod,
};

use anyhow::{Context, Result, bail};
use auth::AuthManager;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path as AxumPath, State},
    http::{
        HeaderMap, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    response::{Html, IntoResponse},
    routing::{delete, get, post},
};
use backend::{HIDDEN_SETTING_KEYS, LogBuffer, resolve_backend_dir};
use backup_service::BackupManager;
use model::{
    Channel, ChannelLookupResponse, LogsResponse, NativeWatcherStatus as WatcherStatus,
    SettingsResponse, StatusResponse, VodAnalyzeRequest, VodDownloadRequest, VodJobStatus,
};
use native_watcher::NativeWatcherManager;
use primary_config::{
    apply_vod_tool_defaults, validate_channels, validate_secret_updates, validate_setting_updates,
    validate_vod_tool_updates,
};
use security::{protect_secret, unprotect_secret};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use store::Store;
use support::{
    platform::{PlatformId, live::LiveSession},
    resolve_channel_name,
};
use tokio::{net::TcpListener, signal, sync::Mutex};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;
use vod::VodManager;
use vod_queue::VodQueueManager;

type ApiError = (StatusCode, String);
type ApiResult<T> = Result<T, ApiError>;

#[derive(Clone)]
struct AppState {
    backend_dir: PathBuf,
    bind: String,
    token: Arc<String>,
    auth: Arc<AuthManager>,
    backups: BackupManager,
    watcher: Arc<NativeWatcherManager>,
    vod: Arc<VodManager>,
    vod_queue: Arc<VodQueueManager>,
    store: Store,
    logs: LogBuffer,
    config_write_lock: Arc<Mutex<()>>,
    lifecycle_lock: Arc<Mutex<()>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let backend_dir = resolve_backend_dir()?;
    let db_path = Store::default_path(&backend_dir);
    let migrated_legacy_db = Store::migrate_legacy_database(&db_path)?;
    let store = Store::open(db_path)?;
    store::init_global(store.clone())?;

    let bind = env::var("STREAM_ARCHIVE_BIND").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    // Establish exclusive server ownership before constructing the VOD manager.
    // CHZZK VOD startup scavenges stale private job directories, so a duplicate
    // process must fail the bind before it can touch an active server's job files.
    let listener = TcpListener::bind(&bind)
        .await
        .with_context(|| format!("failed to bind {bind}"))?;
    let (token, token_source) = load_or_create_token(&backend_dir)?;
    let auth = Arc::new(AuthManager::open(store.path().to_path_buf())?);
    let backups = BackupManager::open(store.clone(), &backend_dir)?;
    let logs = LogBuffer::new();
    let watcher = Arc::new(NativeWatcherManager::new(backend_dir.clone(), logs.clone()));
    let vod = Arc::new(VodManager::new(backend_dir.clone(), logs.clone()));
    let lifecycle_lock = Arc::new(Mutex::new(()));
    let vod_queue = Arc::new(VodQueueManager::new(
        store.clone(),
        vod.clone(),
        logs.clone(),
        lifecycle_lock.clone(),
    )?);
    let state = AppState {
        backend_dir: backend_dir.clone(),
        bind: bind.clone(),
        token: Arc::new(token.clone()),
        auth: auth.clone(),
        backups: backups.clone(),
        watcher: watcher.clone(),
        vod: vod.clone(),
        vod_queue: vod_queue.clone(),
        store: store.clone(),
        logs: logs.clone(),
        config_write_lock: Arc::new(Mutex::new(())),
        lifecycle_lock: lifecycle_lock.clone(),
    };

    logs.push(format!(
        "[SERVER] Stream Archive ready; backend={} db={}",
        backend_dir.display(),
        store.path().display()
    ))
    .await;
    if migrated_legacy_db {
        logs.push(format!(
            "[DB] migrated legacy database filename to {}",
            store.path().display()
        ))
        .await;
    }
    if bind.starts_with("0.0.0.0:") || bind.starts_with("[::]:") {
        warn!(
            "Stream Archive is listening on all interfaces. Prefer loopback plus HTTPS reverse proxy."
        );
        logs.push("[SERVER:WARN] public/LAN listener detected; loopback + Caddy is recommended")
            .await;
    }

    spawn_vod_history_sync(store.clone(), vod.clone(), logs.clone());
    backup_service::spawn_auto_backup(backups.clone(), logs.clone());
    vod_queue.clone().spawn();

    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/phase8.js", get(phase8_js))
        .route("/phase9_1.js", get(phase9_1_js))
        .route("/phase10.js", get(phase10_js))
        .route("/phase12.js", get(phase12_js))
        .route("/phase13.js", get(phase13_js))
        .route("/phase14.js", get(phase14_js))
        .route("/phase14.css", get(phase14_css))
        .route("/style.css", get(style_css))
        .route("/api/auth/status", get(auth::api_status))
        .route("/api/auth/setup", post(auth::api_setup))
        .route("/api/auth/login", post(auth::api_login))
        .route("/api/auth/logout", post(auth::api_logout))
        .route("/api/auth/logout-all", post(auth::api_logout_all))
        .route("/api/auth/change-password", post(auth::api_change_password))
        .route("/api/status", get(api_status))
        .route("/api/events", get(realtime::api_events))
        .route("/api/diagnostics", get(api_diagnostics))
        .route("/api/logs", get(api_logs))
        .route("/api/history", get(history_storage::api_history))
        .route("/api/storage", get(history_storage::api_storage))
        .route(
            "/api/storage/check",
            post(history_storage::api_storage_check),
        )
        .route(
            "/api/backups",
            get(backup::api_list).post(backup::api_create),
        )
        .route(
            "/api/backups/{file_name}/restore",
            post(backup::api_restore),
        )
        .route("/api/local-picker", post(local_picker::api_local_picker))
        .route("/api/settings", get(api_settings).put(api_update_settings))
        .route("/api/secrets", get(api_secrets).put(api_update_secrets))
        .route("/api/secrets/test/soop", post(api_test_soop_auth))
        .route("/api/channels", get(api_channels).put(api_update_channels))
        .route("/api/channels/resolve/{account}", get(api_channel_resolve))
        .route("/api/watcher/start", post(api_watcher_start))
        .route("/api/watcher/stop", post(api_watcher_stop))
        .route(
            "/api/watcher/channel/{account}/{action}",
            post(api_channel_action),
        )
        .route("/api/vod/status", get(api_vod_status))
        .route("/api/vod/analyze", post(api_vod_analyze))
        .route("/api/vod/download", post(api_vod_download))
        .route("/api/vod/cancel", post(api_vod_cancel))
        .route(
            "/api/vod/queue",
            get(vod_queue::api_list).post(vod_queue::api_enqueue),
        )
        .route("/api/vod/queue/{id}/cancel", post(vod_queue::api_cancel))
        .route("/api/vod/queue/{id}/retry", post(vod_queue::api_retry))
        .route("/api/vod/queue/{id}", delete(vod_queue::api_remove))
        .route(
            "/api/vod/tool-settings",
            get(api_vod_tool_settings).put(api_update_vod_tool_settings),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    println!();
    println!("Stream Archive");
    println!("Version : {}", env!("CARGO_PKG_VERSION"));
    println!("Backend : {}", backend_dir.display());
    println!("Data    : {}", store.path().display());
    println!("Listen  : http://{bind}");
    println!("Recovery: {token}");
    println!("Source  : {}", token_source.display());
    println!("Config  : SQLite");
    println!("Watcher : Rust native");
    println!("Recorder: Rust RecorderManager -> Streamlink");
    println!("VOD     : Rust VodManager -> yt-dlp/ffmpeg");
    println!("History : SQLite + filtered query UX");
    println!("Storage : LIVE/channel/VOD free-space diagnostics");
    println!("Picker  : Native Windows file/folder dialog (direct loopback only)");
    println!("Auth    : Browser ID/password session + Bearer recovery token");
    println!("Session : HttpOnly/SameSite cookie + CSRF; Secure cookie through HTTPS proxy");
    println!("Realtime: SSE snapshot stream + automatic REST polling fallback");
    println!("Backup  : SQLite online backup + retention + guarded restore");
    println!("VODQueue: SQLite persistent FIFO queue + retry/cancel controls");
    println!("BackupDir: {}", backups.backup_dir().display());
    println!("Remote  : Keep 127.0.0.1:8787 and expose Caddy on 80/443");
    println!();
    println!("The server is NOT registered as an OS service.");
    println!("Press Ctrl+C to stop the web server and owned LIVE/VOD processes.");
    println!();
    info!("listening on http://{bind}");

    if env_flag("STREAM_ARCHIVE_START_WATCHER") {
        match watcher.start().await {
            Ok(status) => info!("watcher auto-start result: running={}", status.running),
            Err(err) => {
                warn!("watcher auto-start failed: {err:#}");
                logs.push(format!("[SERVER:ERR] watcher auto-start failed: {err:#}"))
                    .await;
            }
        }
    }

    let shutdown_watcher = watcher.clone();
    let shutdown_vod = vod.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = signal::ctrl_c().await;
            let _ = shutdown_vod.cancel().await;
            let _ = shutdown_watcher.stop().await;
        })
        .await
        .context("web server failed")?;
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("stream_archive_server=info,tower_http=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn env_flag(name: &str) -> bool {
    env::var(name).ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "y" | "yes" | "true" | "on"
        )
    })
}

fn load_or_create_token(backend_dir: &Path) -> Result<(String, PathBuf)> {
    if let Ok(token) = env::var("STREAM_ARCHIVE_TOKEN") {
        let token = token.trim().to_string();
        if token.len() < 24 {
            bail!("STREAM_ARCHIVE_TOKEN must contain at least 24 characters");
        }
        return Ok((
            token,
            PathBuf::from("STREAM_ARCHIVE_TOKEN environment variable"),
        ));
    }
    let runtime_dir = backend_dir.join(".stream-archive");
    fs::create_dir_all(&runtime_dir)
        .with_context(|| format!("failed to create {}", runtime_dir.display()))?;
    let token_path = runtime_dir.join("web-token.txt");
    if token_path.is_file() {
        let token = fs::read_to_string(&token_path)
            .with_context(|| format!("failed to read {}", token_path.display()))?
            .trim()
            .to_string();
        if token.len() < 24 {
            bail!(
                "{} contains an invalid/short management token",
                token_path.display()
            );
        }
        return Ok((token, token_path));
    }
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    fs::write(&token_path, format!("{token}\n"))
        .with_context(|| format!("failed to write {}", token_path.display()))?;
    Ok((token, token_path))
}

fn spawn_vod_history_sync(store: Store, vod: Arc<VodManager>, logs: LogBuffer) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let status = vod.status().await;
            if let Err(err) = store.upsert_vod(&status) {
                logs.push(format!("[DB:WARN] VOD history sync failed: {err:#}"))
                    .await;
            }
        }
    });
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../web/index.html"))
}
async fn app_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../web/app.js"),
    )
}
async fn phase8_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../web/phase8.js"),
    )
}
async fn phase9_1_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../web/phase9_1.js"),
    )
}
async fn phase10_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../web/phase10.js"),
    )
}
async fn phase12_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../web/phase12.js"),
    )
}
async fn phase13_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../web/phase13.js"),
    )
}
async fn phase14_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../web/phase14.js"),
    )
}
async fn phase14_css() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../web/phase14.css"),
    )
}
async fn style_css() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../web/style.css"),
    )
}

fn authorize(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    if state
        .auth
        .local_bypass_allowed(headers, &state.bind)
        .map_err(internal_error)?
    {
        return Ok(());
    }
    let supplied = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if let Some(supplied) = supplied {
        return if supplied == state.token.as_str() {
            Ok(())
        } else {
            Err((
                StatusCode::UNAUTHORIZED,
                "invalid management recovery token".into(),
            ))
        };
    }
    state.auth.authorize_session(headers)
}

fn internal_error(err: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

async fn api_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<StatusResponse>> {
    authorize(&headers, &state)?;
    let watcher = state.watcher.status().await.map_err(internal_error)?;
    Ok(Json(StatusResponse {
        watcher,
        backend_dir: state.backend_dir.display().to_string(),
        bind: state.bind.clone(),
        phase: "stream-archive",
    }))
}

async fn api_diagnostics(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let safe = state.store.safe_settings().map_err(internal_error)?;
    let vod = state.store.vod_tool_settings().map_err(internal_error)?;
    let loopback_only = state.bind.starts_with("127.0.0.1:")
        || state.bind.starts_with("[::1]:")
        || state.bind.starts_with("localhost:");
    let streamlink = diagnose_tool(
        safe.get("STREAMLINK_PATH")
            .map(String::as_str)
            .unwrap_or("AUTO"),
        &[
            state.backend_dir.join("streamlink.exe"),
            PathBuf::from(r"C:\Program Files\Streamlink\bin\streamlink.exe"),
        ],
        "streamlink.exe",
    );
    let yt_dlp = diagnose_tool(
        vod.get("YT_DLP_PATH").map(String::as_str).unwrap_or(""),
        &[state.backend_dir.join("vod").join("yt-dlp.exe")],
        "yt-dlp.exe",
    );
    let ffmpeg = diagnose_tool(
        vod.get("FFMPEG_PATH").map(String::as_str).unwrap_or(""),
        &[
            state.backend_dir.join("vod").join("ffmpeg.exe"),
            PathBuf::from(r"C:\Program Files\Streamlink\ffmpeg\ffmpeg.exe"),
        ],
        "ffmpeg.exe",
    );
    Ok(Json(json!({
        "phase": "stream-archive",
        "bind": state.bind,
        "loopback_only": loopback_only,
        "database": state.store.path().display().to_string(),
        "sqlite_primary": true,
        "sqlite_only": true,
        "watcher_config_source": "sqlite-direct",
        "browser_session_auth": true,
        "bearer_recovery": true,
        "reverse_proxy": {
            "recommended": true,
            "upstream": "127.0.0.1:8787",
            "router_forward": "TCP 80/443 -> PC LAN IP; do not forward to 127.0.0.1",
            "direct_8787_exposure_recommended": false
        },
        "tools": {"streamlink": streamlink, "yt_dlp": yt_dlp, "ffmpeg": ffmpeg}
    })))
}

fn diagnose_tool(configured: &str, candidates: &[PathBuf], binary: &str) -> Value {
    let configured = configured.trim();
    if !configured.is_empty() && !configured.eq_ignore_ascii_case("AUTO") {
        let path = PathBuf::from(configured);
        return json!({"configured": configured, "found": path.is_file(), "resolved": if path.is_file() { Some(path.display().to_string()) } else { None }});
    }
    for path in candidates {
        if path.is_file() {
            return json!({"configured": configured, "found": true, "resolved": path.display().to_string()});
        }
    }
    if let Some(path) = find_on_path(binary) {
        return json!({"configured": configured, "found": true, "resolved": path.display().to_string()});
    }
    json!({"configured": configured, "found": false, "resolved": Value::Null})
}

fn find_on_path(binary: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|dir| dir.join(binary))
            .find(|path| path.is_file())
    })
}

async fn api_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<LogsResponse>> {
    authorize(&headers, &state)?;
    Ok(Json(LogsResponse {
        lines: state.logs.tail(400).await,
    }))
}

async fn api_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<SettingsResponse>> {
    authorize(&headers, &state)?;
    let values = state.store.safe_settings().map_err(internal_error)?;
    Ok(Json(SettingsResponse {
        values,
        hidden_keys: HIDDEN_SETTING_KEYS.to_vec(),
    }))
}

async fn api_update_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(updates): Json<BTreeMap<String, String>>,
) -> ApiResult<Json<SettingsResponse>> {
    authorize(&headers, &state)?;
    validate_setting_updates(&updates).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let _guard = state.config_write_lock.lock().await;
    state
        .store
        .sync_settings(&updates, "sqlite-live")
        .map_err(internal_error)?;
    let values = state.store.safe_settings().map_err(internal_error)?;
    state
        .logs
        .push(format!(
            "[SERVER] SQLite settings updated: {}",
            updates.keys().cloned().collect::<Vec<_>>().join(", ")
        ))
        .await;
    Ok(Json(SettingsResponse {
        values,
        hidden_keys: HIDDEN_SETTING_KEYS.to_vec(),
    }))
}

async fn api_secrets(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<BTreeMap<String, bool>>> {
    authorize(&headers, &state)?;
    Ok(Json(
        state.store.configured_secrets().map_err(internal_error)?,
    ))
}

async fn api_update_secrets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(updates): Json<BTreeMap<String, String>>,
) -> ApiResult<Json<BTreeMap<String, bool>>> {
    authorize(&headers, &state)?;
    validate_secret_updates(&updates).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let _guard = state.config_write_lock.lock().await;
    let mut encrypted = BTreeMap::new();
    for (key, value) in &updates {
        if !value.is_empty() {
            encrypted.insert(
                key.clone(),
                protect_secret(value).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?,
            );
        }
    }
    if !encrypted.is_empty() {
        state
            .store
            .sync_settings(&encrypted, "sqlite-secret")
            .map_err(internal_error)?;
        state
            .logs
            .push(format!(
                "[SERVER] SQLite protected secrets updated: {}",
                encrypted.keys().cloned().collect::<Vec<_>>().join(", ")
            ))
            .await;
    }
    Ok(Json(
        state.store.configured_secrets().map_err(internal_error)?,
    ))
}

async fn api_test_soop_auth(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    authorize(&headers, &state)?;
    let settings = state
        .store
        .live_settings_with_secrets()
        .map_err(internal_error)?;
    let username = settings.get("SOOP_USERNAME").cloned().unwrap_or_default();
    let password = unprotect_secret(
        settings
            .get("SOOP_PASSWORD")
            .map(String::as_str)
            .unwrap_or(""),
        "SOOP_PASSWORD",
    )
    .map_err(internal_error)?;
    let worker_url = settings
        .get("CLOUDFLARE_WORKER_URL")
        .cloned()
        .unwrap_or_default();
    let worker_key = unprotect_secret(
        settings
            .get("CLOUDFLARE_API_KEY")
            .map(String::as_str)
            .unwrap_or(""),
        "CLOUDFLARE_API_KEY",
    )
    .map_err(internal_error)?;

    for (label, value) in [
        ("SOOP username", username.as_str()),
        ("SOOP password", password.as_str()),
        ("Worker URL", worker_url.as_str()),
        ("Worker API key", worker_key.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{label} is not configured"),
            ));
        }
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .no_proxy()
        .http1_only()
        .build()
        .map_err(internal_error)?;
    let mut session = LiveSession::new(PlatformId::Soop, client.clone()).map_err(internal_error)?;
    let login_id = session.login(&username, &password).await.map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            format!("SOOP login test failed: {err:#}"),
        )
    })?;

    let response = client
        .post(&worker_url)
        .header("X-API-Key", &worker_key)
        .json(&json!({}))
        .send()
        .await
        .map_err(|err| {
            (
                StatusCode::BAD_REQUEST,
                format!("Worker test failed: {err:#}"),
            )
        })?;
    let worker_status = response.status();
    if worker_status == reqwest::StatusCode::UNAUTHORIZED
        || worker_status == reqwest::StatusCode::FORBIDDEN
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Worker API key authentication failed".into(),
        ));
    }
    if worker_status != reqwest::StatusCode::BAD_REQUEST && !worker_status.is_success() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Worker endpoint test failed: HTTP {worker_status}"),
        ));
    }

    state
        .logs
        .push(format!(
            "[AUTH] SOOP credential test passed login_id={login_id}"
        ))
        .await;
    Ok(Json(json!({"login_id": login_id, "worker_status": "OK"})))
}
async fn api_channels(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<Channel>>> {
    authorize(&headers, &state)?;
    Ok(Json(state.store.channels().map_err(internal_error)?))
}

async fn api_update_channels(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(channels): Json<Vec<Channel>>,
) -> ApiResult<Json<Vec<Channel>>> {
    authorize(&headers, &state)?;
    validate_channels(&channels).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let _guard = state.config_write_lock.lock().await;
    state
        .store
        .sync_channels(&channels)
        .map_err(internal_error)?;
    let saved = state.store.channels().map_err(internal_error)?;
    state
        .logs
        .push(format!(
            "[SERVER] SQLite channel list updated ({} channels)",
            saved.len()
        ))
        .await;
    Ok(Json(saved))
}

async fn api_channel_resolve(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(account): AxumPath<String>,
) -> ApiResult<Json<ChannelLookupResponse>> {
    authorize(&headers, &state)?;
    let name = resolve_channel_name(&account)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    state
        .logs
        .push(format!("[SERVER] channel resolved: {account} -> {name}"))
        .await;
    Ok(Json(ChannelLookupResponse { account, name }))
}

async fn api_watcher_start(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<WatcherStatus>> {
    authorize(&headers, &state)?;
    let _lifecycle_guard = state.lifecycle_lock.lock().await;
    Ok(Json(
        state
            .watcher
            .start()
            .await
            .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?,
    ))
}

async fn api_watcher_stop(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<WatcherStatus>> {
    authorize(&headers, &state)?;
    Ok(Json(state.watcher.stop().await.map_err(internal_error)?))
}

async fn api_channel_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((account, action)): AxumPath<(String, String)>,
    body: Bytes,
) -> ApiResult<StatusCode> {
    authorize(&headers, &state)?;
    if action == "password" {
        let password = std::str::from_utf8(&body).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                "stream password must be UTF-8".into(),
            )
        })?;
        if password.trim().is_empty() {
            return Err((StatusCode::BAD_REQUEST, "stream password is empty".into()));
        }
        if password.chars().count() > 128 {
            return Err((
                StatusCode::BAD_REQUEST,
                "stream password is too long".into(),
            ));
        }
        state
            .watcher
            .channel_password(account, password.to_string())
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    } else {
        state
            .watcher
            .channel_action(account, &action)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn api_vod_tool_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<BTreeMap<String, String>>> {
    authorize(&headers, &state)?;
    Ok(Json(
        state.store.vod_tool_settings().map_err(internal_error)?,
    ))
}

async fn api_update_vod_tool_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(updates): Json<BTreeMap<String, String>>,
) -> ApiResult<Json<BTreeMap<String, String>>> {
    authorize(&headers, &state)?;
    validate_vod_tool_updates(&updates).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let _guard = state.config_write_lock.lock().await;
    state
        .store
        .sync_settings(&updates, "sqlite-vod")
        .map_err(internal_error)?;
    let values = state.store.vod_tool_settings().map_err(internal_error)?;
    state
        .logs
        .push(format!(
            "[SERVER] SQLite VOD tool settings updated: {}",
            updates.keys().cloned().collect::<Vec<_>>().join(", ")
        ))
        .await;
    Ok(Json(values))
}

async fn api_vod_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<VodJobStatus>> {
    authorize(&headers, &state)?;
    let status = state.vod.status().await;
    state.store.upsert_vod(&status).map_err(internal_error)?;
    Ok(Json(status))
}

async fn api_vod_analyze(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut req): Json<VodAnalyzeRequest>,
) -> ApiResult<Json<VodJobStatus>> {
    authorize(&headers, &state)?;
    let _lifecycle_guard = state.lifecycle_lock.lock().await;
    let tools = state.store.vod_tool_settings().map_err(internal_error)?;
    apply_vod_tool_defaults(&tools, &mut req.yt_dlp_path, &mut req.ffmpeg_path);
    let status = state
        .vod
        .analyze(req)
        .await
        .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    state.store.upsert_vod(&status).map_err(internal_error)?;
    Ok(Json(status))
}

async fn api_vod_download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut req): Json<VodDownloadRequest>,
) -> ApiResult<Json<VodJobStatus>> {
    authorize(&headers, &state)?;
    let _lifecycle_guard = state.lifecycle_lock.lock().await;
    let tools = state.store.vod_tool_settings().map_err(internal_error)?;
    apply_vod_tool_defaults(&tools, &mut req.yt_dlp_path, &mut req.ffmpeg_path);
    let status = state
        .vod
        .download(req)
        .await
        .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    state.store.upsert_vod(&status).map_err(internal_error)?;
    Ok(Json(status))
}

async fn api_vod_cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<VodJobStatus>> {
    authorize(&headers, &state)?;
    let _lifecycle_guard = state.lifecycle_lock.lock().await;
    let status = state.vod.cancel().await.map_err(internal_error)?;
    state.store.upsert_vod(&status).map_err(internal_error)?;
    Ok(Json(status))
}
