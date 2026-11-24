use crate::backend::LogBuffer;
use anyhow::{Context, Result, anyhow, bail};
use reqwest::{
    Client, Method, Response, StatusCode,
    header::{CONTENT_TYPE, COOKIE, RANGE, SET_COOKIE},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fmt,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{Mutex, RwLock, Semaphore, watch},
    task::JoinHandle,
};
use url::Url;
use uuid::Uuid;

pub const SOOP_PRIVATE_AUTH_URL: &str = "https://live.sooplive.com/api/private_auth.php";
const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(45);
const MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(20);
const MAX_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const ON_DEMAND_REFRESH_COOLDOWN: Duration = Duration::from_secs(3);
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_HEADER_LINES: usize = 64;
const MAX_PLAYLIST_BYTES: usize = 4 * 1024 * 1024;
const MAX_CONCURRENT_CONNECTIONS: usize = 32;
const IDLE_READ_TIMEOUT: Duration = Duration::from_secs(30);
const PROXY_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36";

#[derive(Clone)]
pub struct SoopSignedAuth {
    pub account: String,
    pub bno: String,
    pub playlist_url: String,
    pub login_cookie_header: String,
    pub initial_signed_cookies: String,
    pub refresh_interval: Duration,
}

impl fmt::Debug for SoopSignedAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let host = Url::parse(&self.playlist_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_else(|| "<invalid>".to_string());
        f.debug_struct("SoopSignedAuth")
            .field("account", &self.account)
            .field("bno", &self.bno)
            .field("host", &host)
            .field("playlist_url", &"<redacted>")
            .field("login_cookie_header", &"<redacted>")
            .field("initial_signed_cookies", &"<redacted>")
            .field("refresh_interval", &self.refresh_interval)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct SignedCookieResult {
    pub cookie_header: String,
    pub refresh_interval: Duration,
}

#[derive(Debug)]
pub enum PrivateAuthError {
    Denied {
        result: i64,
        code: Option<i64>,
        message: String,
    },
    Other(anyhow::Error),
}

impl fmt::Display for PrivateAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied {
                result,
                code: Some(-12),
                ..
            } => write!(
                f,
                "SOOP 로그인이 필요합니다 (private_auth result={result}, code=-12)."
            ),
            Self::Denied {
                result,
                code,
                message,
            } => {
                if message.is_empty() {
                    write!(
                        f,
                        "SOOP 구독(또는 구독플러스) 권한 인증에 실패했습니다 (private_auth result={result}, code={code:?})."
                    )
                } else {
                    write!(
                        f,
                        "SOOP 구독(또는 구독플러스) 권한 인증에 실패했습니다 ({message}, result={result}, code={code:?})."
                    )
                }
            }
            Self::Other(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for PrivateAuthError {}

pub struct SoopSignedHlsProxyGuard {
    shutdown_tx: watch::Sender<bool>,
    accept_task: JoinHandle<()>,
    refresh_task: JoinHandle<()>,
}

impl Drop for SoopSignedHlsProxyGuard {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        self.accept_task.abort();
        self.refresh_task.abort();
    }
}

struct ProxyState {
    client: Client,
    account: String,
    bno: String,
    playlist_url: String,
    login_cookie_header: String,
    private_auth_url: String,
    referer: String,
    origin: String,
    upstream_origin: String,
    upstream_host_port: String,
    upstream_scheme_origin: url::Origin,
    token: String,
    entry_local_path: String,
    local_prefix: String,
    signed_cookie_header: RwLock<String>,
    refresh_lock: Mutex<Option<Instant>>,
    logs: LogBuffer,
}

pub fn is_allowed_soop_cdn_host(host: &str) -> bool {
    let normalized = host.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || normalized.parse::<IpAddr>().is_ok()
        || normalized.starts_with('.')
        || normalized.ends_with('.')
        || normalized.contains("..")
    {
        return false;
    }
    let Some(prefix) = normalized.strip_suffix(".sooplive.com") else {
        return false;
    };
    !prefix.is_empty()
        && prefix.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

pub fn validate_soop_cdn_url(raw: &str) -> Result<Url> {
    let parsed = Url::parse(raw).context("invalid SOOP CDN URL")?;
    if parsed.scheme() != "https" {
        bail!("SOOP CDN URL must use HTTPS");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("SOOP CDN URL must not contain userinfo");
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("SOOP CDN URL is missing host"))?;
    if !is_allowed_soop_cdn_host(host) {
        bail!("SOOP CDN host is not allowed: {host}");
    }
    Ok(parsed)
}

pub async fn request_subscription_signed_cookies(
    client: &Client,
    private_auth_url: &str,
    login_cookie_header: &str,
    account: &str,
    bno: &str,
    playlist_url: &str,
) -> std::result::Result<SignedCookieResult, PrivateAuthError> {
    let referer = format!("https://play.sooplive.com/{account}");
    let mut request = client
        .post(private_auth_url)
        .header("Referer", referer)
        .header("Origin", "https://play.sooplive.com")
        .form(&[
            ("type", "subs_live"),
            ("strm_id", account),
            ("broad_no", bno),
            ("url", playlist_url),
        ]);
    if !login_cookie_header.trim().is_empty() {
        request = request.header(COOKIE, login_cookie_header);
    }

    let response = request
        .send()
        .await
        .map_err(|err| PrivateAuthError::Other(anyhow!("private_auth request failed: {err}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(PrivateAuthError::Other(anyhow!(
            "private_auth HTTP {status}"
        )));
    }

    let (mut cookies, min_max_age) = extract_signed_cookies_from_headers(&response);
    let body = response
        .text()
        .await
        .map_err(|err| PrivateAuthError::Other(anyhow!("private_auth body read failed: {err}")))?;
    let value: Value = serde_json::from_str(&body).map_err(|err| {
        PrivateAuthError::Other(anyhow!("private_auth returned invalid JSON: {err}"))
    })?;

    let result = value.get("result").and_then(Value::as_i64).unwrap_or(-1);
    if result != 1 {
        let code = value.pointer("/data/code").and_then(Value::as_i64);
        let message = value
            .pointer("/data/message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        return Err(PrivateAuthError::Denied {
            result,
            code,
            message,
        });
    }

    if let Some(raw_cookie) = value.pointer("/data/signed_cookie").and_then(Value::as_str) {
        merge_cookie_pairs_from_str(&mut cookies, raw_cookie);
    }

    if cookies.is_empty() {
        return Err(PrivateAuthError::Other(anyhow!(
            "SOOP 구독 방송 인증 응답에 서명 쿠키가 없습니다."
        )));
    }

    let cookie_header = cookies
        .iter()
        .map(|(name, val)| format!("{name}={val}"))
        .collect::<Vec<_>>()
        .join("; ");
    let refresh_interval = min_max_age
        .map(|secs| Duration::from_secs(secs / 2).clamp(MIN_REFRESH_INTERVAL, MAX_REFRESH_INTERVAL))
        .unwrap_or(DEFAULT_REFRESH_INTERVAL);

    Ok(SignedCookieResult {
        cookie_header,
        refresh_interval,
    })
}

fn is_ignored_cookie_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("AbroadChk") || name.eq_ignore_ascii_case("AbroadVod")
}

fn extract_signed_cookies_from_headers(
    response: &Response,
) -> (BTreeMap<String, String>, Option<u64>) {
    let mut cookies = BTreeMap::new();
    let mut min_max_age: Option<u64> = None;

    for raw in response.headers().get_all(SET_COOKIE).iter() {
        let Ok(text) = raw.to_str() else {
            continue;
        };
        let mut segments = text.split(';');
        let Some(pair) = segments.next() else {
            continue;
        };
        let Some((name, val)) = pair.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let val = val.trim();
        if name.is_empty()
            || val.is_empty()
            || val.eq_ignore_ascii_case("deleted")
            || is_ignored_cookie_name(name)
        {
            continue;
        }
        cookies.insert(name.to_string(), val.to_string());

        for attr in segments {
            if let Some((attr_name, attr_val)) = attr.split_once('=')
                && attr_name.trim().eq_ignore_ascii_case("max-age")
                && let Ok(secs) = attr_val.trim().parse::<u64>()
                && secs > 0
            {
                min_max_age = Some(min_max_age.map_or(secs, |cur| cur.min(secs)));
            }
        }
    }

    (cookies, min_max_age)
}

fn merge_cookie_pairs_from_str(target: &mut BTreeMap<String, String>, raw: &str) {
    for part in raw.split(';') {
        let Some((name, val)) = part.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let val = val.trim();
        if name.is_empty() || val.is_empty() || is_ignored_cookie_name(name) {
            continue;
        }
        target
            .entry(name.to_string())
            .or_insert_with(|| val.to_string());
    }
}

fn proxy_http_client() -> Result<Client> {
    static SHARED_CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();
    if let Some(client) = SHARED_CLIENT.get() {
        return Ok(client.clone());
    }
    let client = Client::builder()
        .no_proxy()
        .http1_only()
        .user_agent(PROXY_USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(20))
        .build()
        .context("failed to build SOOP signed-proxy HTTP client")?;
    let _ = SHARED_CLIENT.set(client.clone());
    Ok(client)
}

pub async fn start_soop_signed_hls_proxy(
    auth: SoopSignedAuth,
    logs: LogBuffer,
) -> Result<(String, SoopSignedHlsProxyGuard)> {
    let client = proxy_http_client()?;
    start_proxy_internal(client, auth, SOOP_PRIVATE_AUTH_URL.to_string(), logs, true).await
}

async fn start_proxy_internal(
    client: Client,
    auth: SoopSignedAuth,
    private_auth_url: String,
    logs: LogBuffer,
    enforce_soop_host: bool,
) -> Result<(String, SoopSignedHlsProxyGuard)> {
    let parsed = if enforce_soop_host {
        validate_soop_cdn_url(&auth.playlist_url)?
    } else {
        Url::parse(&auth.playlist_url).context("invalid playlist URL")?
    };
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("playlist URL is missing host"))?;
    let upstream_host_port = match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    let upstream_origin = format!("{}://{upstream_host_port}", parsed.scheme());
    let upstream_scheme_origin = parsed.origin();

    let dir_path = match parsed.path().rfind('/') {
        Some(idx) => &parsed.path()[..=idx],
        None => "/",
    };

    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .context("failed to bind local loopback SOOP HLS proxy")?;
    let local_port = listener.local_addr()?.port();
    let token = Uuid::new_v4().simple().to_string();
    let entry_local_path = format!("/{token}{dir_path}entry.m3u8");
    let local_prefix = format!("http://127.0.0.1:{local_port}/{token}/");
    let local_entry_url = format!("http://127.0.0.1:{local_port}{entry_local_path}");
    let refresh_interval = auth
        .refresh_interval
        .clamp(MIN_REFRESH_INTERVAL, MAX_REFRESH_INTERVAL);

    let state = Arc::new(ProxyState {
        client,
        referer: format!("https://play.sooplive.com/{}", auth.account),
        origin: "https://play.sooplive.com".to_string(),
        account: auth.account,
        bno: auth.bno,
        playlist_url: auth.playlist_url,
        login_cookie_header: auth.login_cookie_header,
        private_auth_url,
        upstream_origin,
        upstream_host_port,
        upstream_scheme_origin,
        token,
        entry_local_path,
        local_prefix,
        signed_cookie_header: RwLock::new(auth.initial_signed_cookies),
        refresh_lock: Mutex::new(None),
        logs,
    });

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let accept_task = tokio::spawn(run_accept_loop(
        listener,
        Arc::clone(&state),
        shutdown_rx.clone(),
    ));
    let refresh_task = tokio::spawn(run_refresh_loop(state, refresh_interval, shutdown_rx));

    Ok((
        local_entry_url,
        SoopSignedHlsProxyGuard {
            shutdown_tx,
            accept_task,
            refresh_task,
        },
    ))
}

async fn run_refresh_loop(
    state: Arc<ProxyState>,
    initial_interval: Duration,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = initial_interval;
    let mut consecutive_failures = 0u32;

    loop {
        let sleep_dur = if consecutive_failures == 0 {
            interval
        } else {
            Duration::from_secs(match consecutive_failures {
                1 => 5,
                2 => 10,
                _ => 15,
            })
        };

        tokio::select! {
            biased;
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = tokio::time::sleep(sleep_dur) => {
                if *shutdown_rx.borrow() {
                    break;
                }
                match refresh_signed_cookies(&state, None).await {
                    Ok(next_interval) => {
                        consecutive_failures = 0;
                        interval = next_interval;
                    }
                    Err(err) => {
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        if consecutive_failures == 1 || consecutive_failures.is_multiple_of(5) {
                            state.logs.push(format!(
                                "[RUST:WARN] SOOP signed cookie refresh failed account={} bno={} (attempt={}): {err}",
                                state.account, state.bno, consecutive_failures
                            )).await;
                        }
                    }
                }
            }
        }
    }
}

async fn refresh_signed_cookies(
    state: &ProxyState,
    failed_cookie: Option<&str>,
) -> std::result::Result<Duration, PrivateAuthError> {
    let mut last_refreshed = state.refresh_lock.lock().await;

    if let Some(failed) = failed_cookie {
        let current = state.signed_cookie_header.read().await;
        if *current != failed {
            return Ok(DEFAULT_REFRESH_INTERVAL);
        }
        if let Some(last) = *last_refreshed
            && last.elapsed() < ON_DEMAND_REFRESH_COOLDOWN
        {
            return Ok(DEFAULT_REFRESH_INTERVAL);
        }
    }

    let fresh = request_subscription_signed_cookies(
        &state.client,
        &state.private_auth_url,
        &state.login_cookie_header,
        &state.account,
        &state.bno,
        &state.playlist_url,
    )
    .await?;

    *state.signed_cookie_header.write().await = fresh.cookie_header;
    *last_refreshed = Some(Instant::now());
    Ok(fresh.refresh_interval)
}

async fn run_accept_loop(
    listener: TcpListener,
    state: Arc<ProxyState>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let permits = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
    loop {
        tokio::select! {
            biased;
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    break;
                };
                let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
                    break;
                };
                let state = Arc::clone(&state);
                let conn_shutdown = shutdown_rx.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ = handle_connection(stream, state, conn_shutdown).await;
                });
            }
        }
    }
}

struct ParsedRequest {
    method: Method,
    target: String,
    keep_alive: bool,
    range: Option<String>,
}

async fn read_request_head<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
) -> Result<Option<ParsedRequest>> {
    let mut first_line = String::new();
    let n = tokio::time::timeout(IDLE_READ_TIMEOUT, reader.read_line(&mut first_line))
        .await
        .map_err(|_| anyhow!("idle read timeout"))??;
    if n == 0 {
        return Ok(None);
    }
    if first_line.len() > MAX_HEADER_BYTES {
        bail!("request line too large");
    }

    let mut parts = first_line.trim_end_matches(['\r', '\n']).split_whitespace();
    let method_str = parts.next().ok_or_else(|| anyhow!("missing HTTP method"))?;
    let target = parts
        .next()
        .ok_or_else(|| anyhow!("missing HTTP target"))?
        .to_string();
    let version = parts
        .next()
        .ok_or_else(|| anyhow!("missing HTTP version"))?;
    if !matches!(version, "HTTP/1.1" | "HTTP/1.0") {
        bail!("unsupported HTTP version");
    }
    let method = match method_str {
        "GET" => Method::GET,
        "HEAD" => Method::HEAD,
        _ => bail!("unsupported method"),
    };

    let mut total_bytes = first_line.len();
    let mut keep_alive = version == "HTTP/1.1";
    let mut range = None;

    for _ in 0..MAX_HEADER_LINES {
        let mut line = String::new();
        let bytes = tokio::time::timeout(IDLE_READ_TIMEOUT, reader.read_line(&mut line))
            .await
            .map_err(|_| anyhow!("header read timeout"))??;
        if bytes == 0 {
            return Ok(None);
        }
        total_bytes = total_bytes.saturating_add(bytes);
        if total_bytes > MAX_HEADER_BYTES {
            bail!("HTTP headers too large");
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            return Ok(Some(ParsedRequest {
                method,
                target,
                keep_alive,
                range,
            }));
        }
        if let Some((name, val)) = trimmed.split_once(':') {
            let name = name.trim();
            let val = val.trim();
            if name.eq_ignore_ascii_case("connection") {
                if val.to_ascii_lowercase().contains("close") {
                    keep_alive = false;
                } else if val.to_ascii_lowercase().contains("keep-alive") {
                    keep_alive = true;
                }
            } else if name.eq_ignore_ascii_case("range") {
                range = Some(val.to_string());
            }
        }
    }

    bail!("too many HTTP headers")
}

fn resolve_upstream_url(state: &ProxyState, target: &str) -> Result<String> {
    if target.contains(['\r', '\n', '\\']) {
        bail!("invalid target characters");
    }
    let prefix = format!("/{}/", state.token);
    if !target.starts_with(&prefix) {
        bail!("unauthorized proxy token");
    }
    let (path_part, query_part) = match target.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (target, None),
    };
    let lower_path = path_part.to_ascii_lowercase();
    if lower_path.contains("%2e") || lower_path.contains("%5c") || lower_path.contains("%2f") {
        bail!("encoded path traversal rejected");
    }
    if path_part.split('/').any(|seg| seg == "..") {
        bail!("path traversal rejected");
    }

    if path_part == state.entry_local_path && query_part.is_none() {
        return Ok(state.playlist_url.clone());
    }

    let rest = &path_part[state.token.len() + 1..]; // retains leading '/'
    if rest.starts_with("//") {
        bail!("protocol-relative path rejected");
    }
    let candidate = match query_part {
        Some(q) => format!("{}{rest}?{q}", state.upstream_origin),
        None => format!("{}{rest}", state.upstream_origin),
    };
    let parsed = Url::parse(&candidate).context("invalid upstream URL")?;
    if parsed.origin() != state.upstream_scheme_origin {
        bail!("upstream origin mismatch");
    }
    Ok(candidate)
}

fn rewrite_playlist_body(body: &str, state: &ProxyState) -> String {
    let https_origin = format!("https://{}/", state.upstream_host_port);
    let http_origin = format!("http://{}/", state.upstream_host_port);
    let rewritten = body.replace(&https_origin, &state.local_prefix);
    rewritten.replace(&http_origin, &state.local_prefix)
}

async fn send_upstream(
    state: &ProxyState,
    method: &Method,
    upstream_url: &str,
    range: Option<&str>,
    cookie_header: &str,
) -> reqwest::Result<Response> {
    let mut req = state
        .client
        .request(method.clone(), upstream_url)
        .header("Referer", &state.referer)
        .header("Origin", &state.origin)
        .header(COOKIE, cookie_header);
    if let Some(range) = range {
        req = req.header(RANGE, range);
    }
    req.send().await
}

async fn fetch_upstream_with_refresh(
    state: &ProxyState,
    method: &Method,
    upstream_url: &str,
    range: Option<&str>,
) -> Result<Response> {
    let cookie_snapshot = state.signed_cookie_header.read().await.clone();
    let response = send_upstream(state, method, upstream_url, range, &cookie_snapshot).await?;

    if matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        if let Err(err) = refresh_signed_cookies(state, Some(&cookie_snapshot)).await {
            state
                .logs
                .push(format!(
                    "[RUST:WARN] SOOP on-demand signed cookie refresh failed account={} bno={}: {err}",
                    state.account, state.bno
                ))
                .await;
            return Ok(response);
        }
        let updated_cookie = state.signed_cookie_header.read().await.clone();
        if updated_cookie != cookie_snapshot {
            drop(response);
            return Ok(send_upstream(state, method, upstream_url, range, &updated_cookie).await?);
        }
    }

    Ok(response)
}

async fn write_simple_error<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    status: StatusCode,
    body: &str,
) {
    let reason = status.canonical_reason().unwrap_or("Error");
    let response = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        status.as_u16(),
        body.len()
    );
    let _ = writer.write_all(response.as_bytes()).await;
    let _ = writer.flush().await;
}

async fn handle_connection(
    stream: TcpStream,
    state: Arc<ProxyState>,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    let _ = stream.set_nodelay(true);
    let (read_half, mut writer) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    while !*shutdown_rx.borrow() {
        let req = match read_request_head(&mut reader).await {
            Ok(Some(req)) => req,
            Ok(None) => break,
            Err(err) => {
                let msg = err.to_string();
                if msg.contains("unsupported method") {
                    write_simple_error(
                        &mut writer,
                        StatusCode::METHOD_NOT_ALLOWED,
                        "Method Not Allowed",
                    )
                    .await;
                } else if !msg.contains("idle read timeout") {
                    write_simple_error(&mut writer, StatusCode::BAD_REQUEST, "Bad Request").await;
                }
                break;
            }
        };

        let upstream_url = match resolve_upstream_url(&state, &req.target) {
            Ok(url) => url,
            Err(_) => {
                write_simple_error(&mut writer, StatusCode::FORBIDDEN, "Forbidden").await;
                break;
            }
        };

        let mut upstream_resp = match fetch_upstream_with_refresh(
            &state,
            &req.method,
            &upstream_url,
            req.range.as_deref(),
        )
        .await
        {
            Ok(resp) => resp,
            Err(_) => {
                write_simple_error(&mut writer, StatusCode::BAD_GATEWAY, "Bad Gateway").await;
                break;
            }
        };

        let status = upstream_resp.status();
        let reason = status.canonical_reason().unwrap_or("OK");
        let content_type = upstream_resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.contains(['\r', '\n']))
            .unwrap_or("application/octet-stream")
            .to_string();
        let path_only = req.target.split('?').next().unwrap_or("");
        let is_playlist = path_only.to_ascii_lowercase().ends_with(".m3u8")
            || content_type.to_ascii_lowercase().contains("mpegurl");
        let conn_hdr = if req.keep_alive {
            "keep-alive"
        } else {
            "close"
        };

        if is_playlist {
            let mut body_bytes = Vec::new();
            let mut too_large = false;
            while let Some(chunk) = upstream_resp.chunk().await? {
                if body_bytes.len().saturating_add(chunk.len()) > MAX_PLAYLIST_BYTES {
                    too_large = true;
                    break;
                }
                body_bytes.extend_from_slice(&chunk);
            }
            if too_large {
                write_simple_error(&mut writer, StatusCode::BAD_GATEWAY, "Playlist too large")
                    .await;
                break;
            }
            let body_out = if status.is_success() {
                match std::str::from_utf8(&body_bytes) {
                    Ok(text) => rewrite_playlist_body(text, &state).into_bytes(),
                    Err(_) => body_bytes,
                }
            } else {
                body_bytes
            };
            let header = format!(
                "HTTP/1.1 {} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: {conn_hdr}\r\n\r\n",
                status.as_u16(),
                body_out.len()
            );
            writer.write_all(header.as_bytes()).await?;
            if req.method != Method::HEAD && !body_out.is_empty() {
                writer.write_all(&body_out).await?;
            }
            writer.flush().await?;
        } else if req.method == Method::HEAD {
            let mut header = format!(
                "HTTP/1.1 {} {reason}\r\nContent-Type: {content_type}\r\n",
                status.as_u16()
            );
            if let Some(len) = upstream_resp.content_length() {
                header.push_str(&format!("Content-Length: {len}\r\n"));
            }
            header.push_str(&format!("Connection: {conn_hdr}\r\n\r\n"));
            writer.write_all(header.as_bytes()).await?;
            writer.flush().await?;
        } else if let Some(expected_len) = upstream_resp.content_length() {
            let header = format!(
                "HTTP/1.1 {} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {expected_len}\r\nConnection: {conn_hdr}\r\n\r\n",
                status.as_u16()
            );
            writer.write_all(header.as_bytes()).await?;
            let mut written = 0u64;
            while let Some(chunk) = upstream_resp.chunk().await? {
                writer.write_all(&chunk).await?;
                written = written.saturating_add(chunk.len() as u64);
            }
            writer.flush().await?;
            if written != expected_len {
                break;
            }
        } else {
            let header = format!(
                "HTTP/1.1 {} {reason}\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\nConnection: {conn_hdr}\r\n\r\n",
                status.as_u16()
            );
            writer.write_all(header.as_bytes()).await?;
            while let Some(chunk) = upstream_resp.chunk().await? {
                if !chunk.is_empty() {
                    let size_line = format!("{:x}\r\n", chunk.len());
                    writer.write_all(size_line.as_bytes()).await?;
                    writer.write_all(&chunk).await?;
                    writer.write_all(b"\r\n").await?;
                }
            }
            writer.write_all(b"0\r\n\r\n").await?;
            writer.flush().await?;
        }

        if !req.keep_alive {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn validates_soop_cdn_hosts_strictly() {
        assert!(is_allowed_soop_cdn_host(
            "live-pcweb-sub-kr-cdn-z02.sooplive.com"
        ));
        assert!(is_allowed_soop_cdn_host(
            "live-sub-global-cdn-v02.sooplive.com"
        ));
        assert!(is_allowed_soop_cdn_host("pc-web-sub.stream.sooplive.com"));
        for rejected in [
            "",
            "sooplive.com",
            ".sooplive.com",
            "evilsooplive.com",
            "live.sooplive.com.evil.test",
            "127.0.0.1",
            "live..sooplive.com",
            "-bad.sooplive.com",
        ] {
            assert!(
                !is_allowed_soop_cdn_host(rejected),
                "should reject {rejected}"
            );
        }
        assert!(
            validate_soop_cdn_url(
                "https://live-sub-global-cdn-v02.sooplive.com/live-stm-13/123/auth_master_playlist.m3u8"
            )
            .is_ok()
        );
        assert!(
            validate_soop_cdn_url(
                "http://live-sub-global-cdn-v02.sooplive.com/live-stm-13/123/auth_master_playlist.m3u8"
            )
            .is_err()
        );
        assert!(
            validate_soop_cdn_url("https://user@live-sub-global-cdn-v02.sooplive.com/a.m3u8")
                .is_err()
        );
    }

    #[test]
    fn debug_redacts_secrets_from_signed_auth() {
        let auth = SoopSignedAuth {
            account: "streamer1".into(),
            bno: "12345".into(),
            playlist_url: "https://live-pcweb-sub-kr-cdn-z02.sooplive.com/live-stm-13/12345/auth_master_playlist.m3u8?aid=SUPER_SECRET_AID".into(),
            login_cookie_header: "AuthTicket=SUPER_SECRET_LOGIN".into(),
            initial_signed_cookies: "CloudFront-Signature=SUPER_SECRET_SIG".into(),
            refresh_interval: Duration::from_secs(45),
        };
        let rendered = format!("{auth:?}");
        assert!(!rendered.contains("SUPER_SECRET_AID"));
        assert!(!rendered.contains("SUPER_SECRET_LOGIN"));
        assert!(!rendered.contains("SUPER_SECRET_SIG"));
        assert!(rendered.contains("live-pcweb-sub-kr-cdn-z02.sooplive.com"));
    }

    #[test]
    fn merges_signed_cookies_and_ignores_abroad_flags() {
        let mut target = BTreeMap::new();
        merge_cookie_pairs_from_str(
            &mut target,
            "AbroadChk=FAIL; CloudFront-Signature=sig1; CloudFront-Key-Pair-Id=sooplive; CloudFront-Policy=pol1;",
        );
        assert_eq!(target.len(), 3);
        assert!(!target.contains_key("AbroadChk"));
        assert_eq!(
            target.get("CloudFront-Key-Pair-Id").map(String::as_str),
            Some("sooplive")
        );
    }

    #[tokio::test]
    async fn proxy_streams_playlists_rewrites_absolute_urls_and_refreshes_on_401() {
        // Start a mock upstream server that acts as both CDN and private_auth.php.
        let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_port = mock_listener.local_addr().unwrap().port();
        let refresh_calls = Arc::new(AtomicUsize::new(0));
        let refresh_calls_srv = Arc::clone(&refresh_calls);

        let mock_task = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = mock_listener.accept().await else {
                    break;
                };
                let refresh_calls_conn = Arc::clone(&refresh_calls_srv);
                tokio::spawn(async move {
                    let mut reader = BufReader::new(&mut sock);
                    let mut req_lines = Vec::new();
                    let mut content_len = 0usize;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                            break;
                        }
                        if line.to_ascii_lowercase().starts_with("content-length:")
                            && let Some((_, v)) = line.split_once(':')
                        {
                            content_len = v.trim().parse().unwrap_or(0);
                        }
                        let done = line == "\r\n" || line == "\n";
                        req_lines.push(line);
                        if done {
                            break;
                        }
                    }
                    if content_len > 0 {
                        let mut body_discard = vec![0u8; content_len];
                        let _ = tokio::io::AsyncReadExt::read_exact(&mut reader, &mut body_discard)
                            .await;
                    }
                    let req = req_lines.concat();
                    let first_line = req.lines().next().unwrap_or("");
                    let target = first_line.split_whitespace().nth(1).unwrap_or("");
                    let cookie_line = req
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("cookie:"))
                        .unwrap_or("");

                    if target == "/api/private_auth.php" {
                        refresh_calls_conn.fetch_add(1, Ordering::SeqCst);
                        let body = r#"{"result":1,"data":{"signed_cookie":"CloudFront-Signature=FRESH_SIG;CloudFront-Key-Pair-Id=sooplive;CloudFront-Policy=FRESH_POL;"}}"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: CloudFront-Signature=FRESH_SIG; Max-Age=90; path=/\r\nSet-Cookie: CloudFront-Key-Pair-Id=sooplive; Max-Age=90; path=/\r\nSet-Cookie: CloudFront-Policy=FRESH_POL; Max-Age=90; path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = sock.write_all(resp.as_bytes()).await;
                    } else if target
                        .starts_with("/live-stm-13/999/auth_master_playlist.m3u8?aid=TEST_AID")
                    {
                        if !cookie_line.contains("FRESH_SIG") {
                            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
                            let _ = sock.write_all(resp.as_bytes()).await;
                        } else {
                            let body = format!(
                                "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=8000000\nhttp://127.0.0.1:{mock_port}/live-stm-13/999/auth_playlist.m3u8?aid=VAR_AID\n"
                            );
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.apple.mpegurl\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );
                            let _ = sock.write_all(resp.as_bytes()).await;
                        }
                    } else if target == "/live-stm-13/999/seg1.ts" {
                        if !cookie_line.contains("FRESH_SIG") {
                            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nConnection: close\r\n\r\nUnauthorized";
                            let _ = sock.write_all(resp.as_bytes()).await;
                        } else {
                            let seg: Vec<u8> = vec![0x47; 188];
                            let hdr = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: video/mp2t\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                seg.len()
                            );
                            let _ = sock.write_all(hdr.as_bytes()).await;
                            let _ = sock.write_all(&seg).await;
                        }
                    } else {
                        let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nNot Found";
                        let _ = sock.write_all(resp.as_bytes()).await;
                    }
                });
            }
        });

        let client = Client::builder().no_proxy().http1_only().build().unwrap();
        let auth = SoopSignedAuth {
            account: "tester".into(),
            bno: "999".into(),
            playlist_url: format!(
                "http://127.0.0.1:{mock_port}/live-stm-13/999/auth_master_playlist.m3u8?aid=TEST_AID"
            ),
            login_cookie_header: "AuthTicket=login_cookie".into(),
            initial_signed_cookies: "CloudFront-Signature=EXPIRED_SIG".into(),
            refresh_interval: Duration::from_secs(45),
        };
        let (local_url, guard) = start_proxy_internal(
            client.clone(),
            auth,
            format!("http://127.0.0.1:{mock_port}/api/private_auth.php"),
            LogBuffer::new(),
            false,
        )
        .await
        .unwrap();

        // Entry URL must NOT expose the master AID on the command line.
        assert!(!local_url.contains("TEST_AID"));

        // First request hits expired cookie -> triggers on-demand private_auth refresh -> retries -> 200 OK!
        let master_resp = client.get(&local_url).send().await.unwrap();
        assert_eq!(master_resp.status(), StatusCode::OK);
        let master_text = master_resp.text().await.unwrap();
        assert!(master_text.contains("#EXTM3U"));
        // Absolute upstream URL inside playlist must be rewritten to local loopback proxy!
        assert!(!master_text.contains(&format!("127.0.0.1:{mock_port}")));
        assert!(master_text.contains("/live-stm-13/999/auth_playlist.m3u8?aid=VAR_AID"));
        assert_eq!(refresh_calls.load(Ordering::SeqCst), 1);

        // Segment request uses already-refreshed cookie without redundant private_auth calls.
        let seg_url = Url::parse(&local_url).unwrap().join("seg1.ts").unwrap();
        let seg_resp = client.get(seg_url).send().await.unwrap();
        assert_eq!(seg_resp.status(), StatusCode::OK);
        let seg_bytes = seg_resp.bytes().await.unwrap();
        assert_eq!(seg_bytes.len(), 188);
        assert_eq!(seg_bytes[0], 0x47);
        assert_eq!(refresh_calls.load(Ordering::SeqCst), 1);

        // Unauthorized token and traversal paths must be rejected with 403.
        let parsed = Url::parse(&local_url).unwrap();
        let bad_token = format!(
            "http://127.0.0.1:{}/wrongtoken/live-stm-13/999/seg1.ts",
            parsed.port().unwrap()
        );
        assert_eq!(
            client.get(bad_token).send().await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let traversal = local_url.replace("entry.m3u8", "..%2fsecret");
        assert_eq!(
            client.get(traversal).send().await.unwrap().status(),
            StatusCode::FORBIDDEN
        );

        drop(guard);
        mock_task.abort();
    }
}
