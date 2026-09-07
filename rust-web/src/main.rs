mod backend;
mod model;

use anyhow::{bail, Context, Result};
use axum::{
    extract::State,
    http::{header::{AUTHORIZATION, CONTENT_TYPE}, HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post, put},
    Json, Router,
};
use backend::{
    channels_path, ensure_runtime_files, read_channels, read_safe_settings, resolve_backend_dir,
    settings_path, update_settings, write_channels, LogBuffer, WatcherManager, HIDDEN_SETTING_KEYS,
};
use model::{Channel, LogsResponse, SettingsResponse, StatusResponse, WatcherStatus};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{net::TcpListener, signal, sync::Mutex};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

type ApiError = (StatusCode, String);
type ApiResult<T> = Result<T, ApiError>;

#[derive(Clone)]
struct AppState {
    backend_dir: PathBuf,
    bind: String,
    token: Arc<String>,
    watcher: Arc<WatcherManager>,
    logs: LogBuffer,
    file_write_lock: Arc<Mutex<()>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let backend_dir = resolve_backend_dir()?;
    ensure_runtime_files(&backend_dir)?;

    let bind = env::var("SOOP_WEB_BIND").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    let (token, token_source) = load_or_create_token(&backend_dir)?;

    let logs = LogBuffer::new();
    let watcher = Arc::new(WatcherManager::new(backend_dir.clone(), logs.clone()));
    let state = AppState {
        backend_dir: backend_dir.clone(),
        bind: bind.clone(),
        token: Arc::new(token.clone()),
        watcher: watcher.clone(),
        logs: logs.clone(),
        file_write_lock: Arc::new(Mutex::new(())),
    };

    logs.push(format!(
        "[SERVER] Phase 1 web bridge ready; backend={}",
        backend_dir.display()
    ))
    .await;

    if bind.starts_with("0.0.0.0:") || bind.starts_with("[::]:") {
        warn!(
            "SOOP web server is listening on all interfaces. Use HTTPS/reverse proxy for internet access."
        );
        logs.push(
            "[SERVER:WARN] listening on all interfaces; use HTTPS/reverse proxy for internet access",
        )
        .await;
    }

    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .route("/api/status", get(api_status))
        .route("/api/logs", get(api_logs))
        .route("/api/settings", get(api_settings).put(api_update_settings))
        .route("/api/channels", get(api_channels).put(api_update_channels))
        .route("/api/watcher/start", post(api_watcher_start))
        .route("/api/watcher/stop", post(api_watcher_stop))
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    let listener = TcpListener::bind(&bind)
        .await
        .with_context(|| format!("failed to bind {bind}"))?;

    println!();
    println!("SOOP Rust Web - Phase 1");
    println!("Backend : {}", backend_dir.display());
    println!("Listen  : http://{bind}");
    println!("Token   : {token}");
    println!("Source  : {}", token_source.display());
    println!();
    println!("The server is NOT registered as an OS service.");
    println!("Press Ctrl+C to stop the web server and the watcher.");
    println!();

    info!("listening on http://{bind}");

    if env_flag("SOOP_START_WATCHER") {
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
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = signal::ctrl_c().await;
            let _ = shutdown_watcher.stop().await;
        })
        .await
        .context("web server failed")?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("soop_web=info,tower_http=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .ok()
        .is_some_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "y" | "yes" | "true" | "on"))
}

fn load_or_create_token(backend_dir: &Path) -> Result<(String, PathBuf)> {
    if let Ok(token) = env::var("SOOP_WEB_TOKEN") {
        let token = token.trim().to_string();
        if token.len() < 24 {
            bail!("SOOP_WEB_TOKEN must contain at least 24 characters");
        }
        return Ok((token, PathBuf::from("SOOP_WEB_TOKEN environment variable")));
    }

    let runtime_dir = backend_dir.join(".rust-web");
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

    let token = format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    fs::write(&token_path, format!("{token}\n"))
        .with_context(|| format!("failed to write {}", token_path.display()))?;

    Ok((token, token_path))
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

async fn style_css() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../web/style.css"),
    )
}

fn authorize(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    let supplied = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    if supplied == Some(state.token.as_str()) {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "invalid management token".into()))
    }
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
        phase: "phase1-powershell-bridge",
    }))
}

async fn api_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<LogsResponse>> {
    authorize(&headers, &state)?;
    Ok(Json(LogsResponse {
        lines: state.logs.tail(250).await,
    }))
}

async fn api_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<SettingsResponse>> {
    authorize(&headers, &state)?;

    let path = settings_path(&state.backend_dir);
    let values = read_safe_settings(&path).map_err(internal_error)?;
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
    let _guard = state.file_write_lock.lock().await;

    let path = settings_path(&state.backend_dir);
    update_settings(&path, &updates).map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;

    state
        .logs
        .push(format!(
            "[SERVER] settings updated: {}",
            updates.keys().cloned().collect::<Vec<_>>().join(", ")
        ))
        .await;

    let values = read_safe_settings(&path).map_err(internal_error)?;
    Ok(Json(SettingsResponse {
        values,
        hidden_keys: HIDDEN_SETTING_KEYS.to_vec(),
    }))
}

async fn api_channels(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<Channel>>> {
    authorize(&headers, &state)?;
    let path = channels_path(&state.backend_dir);
    let channels = read_channels(&path).map_err(internal_error)?;
    Ok(Json(channels))
}

async fn api_update_channels(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(channels): Json<Vec<Channel>>,
) -> ApiResult<Json<Vec<Channel>>> {
    authorize(&headers, &state)?;
    let _guard = state.file_write_lock.lock().await;

    let path = channels_path(&state.backend_dir);
    write_channels(&path, &channels).map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;

    state
        .logs
        .push(format!(
            "[SERVER] channel list updated ({} channels)",
            channels.len()
        ))
        .await;

    let channels = read_channels(&path).map_err(internal_error)?;
    Ok(Json(channels))
}

async fn api_watcher_start(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<WatcherStatus>> {
    authorize(&headers, &state)?;
    let status = state
        .watcher
        .start()
        .await
        .map_err(|err| (StatusCode::CONFLICT, err.to_string()))?;
    Ok(Json(status))
}

async fn api_watcher_stop(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<WatcherStatus>> {
    authorize(&headers, &state)?;
    let status = state.watcher.stop().await.map_err(internal_error)?;
    Ok(Json(status))
}
