//! KICK playback authentication and parallel native HLS download to MPEG-TS.
use crate::{
    backend::LogBuffer,
    model::{
        VodAnalysisView, VodAnalyzeRequest, VodDownloadRequest, VodJobStatus, VodPartInfo,
        VodQualityOption,
    },
    support::platform::PlatformId,
};
use anyhow::{Result, bail};
use chrono::Utc;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
};
use url::Url;
use uuid::Uuid;
const TERMINAL_CACHE_LIMIT: usize = 32;
struct JobRuntime {
    task: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
}

pub struct VodManager {
    backend_dir: PathBuf,
    logs: LogBuffer,
    runtime: Mutex<JobRuntime>,
    status: Arc<RwLock<VodJobStatus>>,
    terminal: Arc<Mutex<VecDeque<(String, VodJobStatus)>>>,
    events: crate::download_events::DownloadEvents,
    notification_epoch: Arc<std::sync::atomic::AtomicU64>,
}

impl VodManager {
    pub fn new(backend_dir: PathBuf, logs: LogBuffer) -> Self {
        Self::new_with_events(backend_dir, logs, Default::default())
    }

    pub(crate) fn new_with_events(
        backend_dir: PathBuf,
        logs: LogBuffer,
        events: crate::download_events::DownloadEvents,
    ) -> Self {
        Self {
            backend_dir,
            logs,
            runtime: Mutex::new(JobRuntime {
                task: None,
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            status: Arc::new(RwLock::new(VodJobStatus {
                platform: PlatformId::Kick,
                ..Default::default()
            })),
            terminal: Arc::new(Mutex::new(VecDeque::new())),
            events,
            notification_epoch: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub(crate) fn notification_state(
        &self,
    ) -> (&RwLock<VodJobStatus>, &std::sync::atomic::AtomicU64) {
        (&self.status, &self.notification_epoch)
    }

    pub async fn status(&self) -> VodJobStatus {
        {
            let mut runtime = self.runtime.lock().await;
            if runtime.task.as_ref().is_some_and(|task| task.is_finished()) {
                runtime.task.take();
            }
        }
        self.status.read().await.clone()
    }

    pub async fn terminal_status(&self, job_id: &str) -> Option<VodJobStatus> {
        self.terminal
            .lock()
            .await
            .iter()
            .rev()
            .find(|(id, _)| id == job_id)
            .map(|(_, status)| status.clone())
    }

    pub async fn analyze(&self, req: VodAnalyzeRequest) -> Result<VodJobStatus> {
        self.start_job(VodJobKind::Analyze(req)).await
    }

    pub async fn download(&self, req: VodDownloadRequest) -> Result<VodJobStatus> {
        self.start_job(VodJobKind::Download(req)).await
    }

    async fn start_job(&self, kind: VodJobKind) -> Result<VodJobStatus> {
        let mut runtime = self.runtime.lock().await;
        if runtime
            .task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            bail!("다른 KICK VOD 작업이 이미 실행 중입니다.");
        }
        runtime.task.take();

        let cancel = Arc::new(AtomicBool::new(false));
        runtime.cancel = cancel.clone();
        let backend = self.backend_dir.clone();
        let logs = self.logs.clone();
        let status = self.status.clone();
        let terminal = self.terminal.clone();
        let job_id = Uuid::new_v4().to_string();
        let terminal_job_id = job_id.clone();
        {
            let mut current = status.write().await;
            *current = VodJobStatus {
                platform: PlatformId::Kick,
                state: "STARTING".into(),
                running: true,
                job_id: Some(job_id),
                message: "KICK VOD 작업 준비 중…".into(),
                started_at: Some(Utc::now().to_rfc3339()),
                ..Default::default()
            };
            self.notification_epoch
                .store(self.events.epoch(), Ordering::Release);
        }

        let events = self.events.clone();
        let event_epoch = self.notification_epoch.clone();
        let download = matches!(&kind, VodJobKind::Download(_));
        let task = tokio::spawn(async move {
            let result = match kind {
                VodJobKind::Analyze(req) => {
                    run_analysis(&backend, req, &logs, &status, &cancel).await
                }
                VodJobKind::Download(req) => {
                    run_download(&backend, req, &logs, &status, &cancel).await
                }
            };
            let final_status = {
                let mut current = status.write().await;
                current.running = false;
                current.finished_at = Some(Utc::now().to_rfc3339());
                if should_mark_cancelled(&cancel, &current.state) {
                    current.state = "CANCELLED".into();
                    current.message = "KICK VOD 작업이 취소되었습니다. 생성된 partial.ts / .part 부분 파일은 보존됩니다. 재시도는 새 파일로 시작합니다.".into();
                    logs.push("[VOD:KICK] job cancelled").await;
                } else if let Err(err) = result {
                    current.state = "FAILED".into();
                    current.message = format!("{err:#}");
                    logs.push(format!(
                        "[VOD:KICK:ERR] job={terminal_job_id} {}",
                        current.message
                    ))
                    .await;
                }
                current.clone()
            };
            let mut terminal = terminal.lock().await;
            if terminal.len() >= TERMINAL_CACHE_LIMIT {
                terminal.pop_front();
            }
            events.terminal(&final_status, download, event_epoch.load(Ordering::Acquire));
            terminal.push_back((terminal_job_id, final_status));
        });
        runtime.task = Some(task);
        Ok(self.status.read().await.clone())
    }

    pub async fn cancel(&self) -> Result<VodJobStatus> {
        let task = {
            let mut runtime = self.runtime.lock().await;
            if runtime
                .task
                .as_ref()
                .is_some_and(|task| !task.is_finished())
            {
                runtime.cancel.store(true, Ordering::SeqCst);
            }
            runtime.task.take()
        };
        {
            let mut current = self.status.write().await;
            if current.running {
                current.state = "CANCELLING".into();
                current.message = "KICK VOD 프로세스 종료 중…".into();
            }
        }
        if let Some(task) = task {
            let _ = task.await;
        }
        Ok(self.status.read().await.clone())
    }
}

fn should_mark_cancelled(cancel: &AtomicBool, state: &str) -> bool {
    cancel.load(Ordering::SeqCst) && state != "COMPLETED"
}

enum VodJobKind {
    Analyze(VodAnalyzeRequest),
    Download(VodDownloadRequest),
}

#[cfg(test)]
const TOKEN_KEY: &str = "KICK_SESSION_TOKEN";
const BODY_LIMIT: usize = 2 * 1024 * 1024;

pub(crate) fn parse_url(raw: &str) -> Result<(String, String)> {
    let url = Url::parse(raw.trim()).map_err(|_| anyhow::anyhow!("KICK VOD URL을 확인하세요."))?;
    let parts: Vec<_> = url.path().trim_end_matches('/').split('/').collect();
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("kick.com" | "www.kick.com"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || parts.len() != 4
        || parts[2] != "videos"
        || parts[3].len() != 36
        || Uuid::parse_str(parts[3]).is_err()
    {
        bail!("KICK VOD URL은 https://kick.com/채널/videos/UUID 형식이어야 합니다.");
    }
    use crate::support::platform::PlatformProvider;
    super::KICK.validate_account(parts[1])?;
    Ok((parts[1].to_ascii_lowercase(), parts[3].to_ascii_lowercase()))
}

fn media_url(raw: &str) -> Result<Url> {
    let url =
        Url::parse(raw).map_err(|_| anyhow::anyhow!("KICK 재생 주소 형식이 변경되었습니다."))?;
    if url.scheme() != "https"
        || url.host_str() != Some("stream.kick.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !url.path().ends_with(".m3u8")
        || url.fragment().is_some()
    {
        bail!("허용되지 않은 KICK VOD 재생 주소입니다.");
    }
    Ok(url)
}

// Keep cookie encoding intact; percent-decode only the Bearer value (not '+' as space).
fn auth_headers(token: &str) -> Result<reqwest::header::HeaderMap> {
    use reqwest::header::{AUTHORIZATION, COOKIE, HeaderMap, HeaderValue};
    if token.is_empty() || token.len() > 16384 || token.contains([';', '\r', '\n', '\0']) {
        bail!("KICK session_token 입력값을 확인하세요.");
    }
    let bytes = token.as_bytes();
    let mut decoded = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                bail!("KICK session_token 인코딩을 확인하세요.");
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|s| u8::from_str_radix(s, 16).ok())
                .ok_or_else(|| anyhow::anyhow!("KICK session_token 인코딩을 확인하세요."))?;
            decoded.push(hex);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8(decoded)
        .map_err(|_| anyhow::anyhow!("KICK session_token 인코딩을 확인하세요."))?;
    let mut headers = HeaderMap::new();
    let mut bearer = HeaderValue::from_str(&format!("Bearer {decoded}"))
        .map_err(|_| anyhow::anyhow!("KICK session_token 헤더 형식을 확인하세요."))?;
    bearer.set_sensitive(true);
    headers.insert(AUTHORIZATION, bearer);
    let mut cookie = HeaderValue::from_str(&format!("session_token={token}"))
        .map_err(|_| anyhow::anyhow!("KICK session_token Cookie 형식을 확인하세요."))?;
    cookie.set_sensitive(true);
    headers.insert(COOKIE, cookie);
    Ok(headers)
}

fn playback_body(channel: &str, id: &str) -> serde_json::Value {
    serde_json::json!({
        "video_player": {
            "player": {"player_name":"web", "player_version":"web_6cd7cc6b", "player_software":"IVS Player", "player_software_version":"1.56.1"},
            "mux_sdk":{"sdk_available":true}, "pal_sdk":{"sdk_available":false,"nonce":""},
            "datazoom_sdk":{"sdk_available":true,"datazoom_sdk_version":"2.33.0","om_sdk_version":"1.6.6"},
            "google_ads_sdk":{"sdk_available":false}
        },
        "video_session":{"page_type":"video","player_remote_played":false,"enable_sampling":false,
            "url_path":format!("{channel}/videos/{id}"),"autoplay_behaviour":"click","play_muted":false,"viewer_connection_type":""},
        "user_session":{"session_id":Uuid::new_v4().to_string(),"player_device_id":Uuid::new_v4().to_string(),"browser_lang":"ko","non_personalised_ads":true}
    })
}

struct Metadata {
    view: VodAnalysisView,
    source: Url,
    duration: u64,
    variants: Vec<(VodQualityOption, Url, u64)>,
}

fn parse_playback(raw: &str, value: &serde_json::Value, authenticated: bool) -> Result<Metadata> {
    let (channel, id) = parse_url(raw)?;
    let session = &value["video_session"];
    if session["video_id"].as_str() != Some(id.as_str())
        || !session["video_series"]
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case(&channel))
        || session["video_stream_status"].as_str() != Some("vod")
    {
        bail!("[playback.identity] KICK 응답이 요청한 VOD와 일치하지 않습니다.");
    }
    if session["video_encryption_type"].as_str() != Some("NONE") {
        bail!("[playback.encryption] 암호화된 KICK VOD는 지원하지 않습니다.");
    }
    let source = value.pointer("/playback_url/vod").and_then(serde_json::Value::as_str).filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!(if authenticated {
            "[playback.vod_missing] KICK 재생 주소가 없습니다. 계정의 시청 권한, session_token 유효성 또는 API 응답 변경을 확인하세요."
        } else { "[playback.vod_missing] KICK 재생 주소가 없습니다. 구독 전용 영상이면 설정에서 session_token을 저장하세요." }))?;
    let duration = session["video_duration"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(|| anyhow::anyhow!("KICK 영상 길이를 받지 못했습니다."))?;
    let title = session["video_title"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("KICK 영상 제목을 받지 못했습니다."))?;
    Ok(Metadata {
        source: media_url(source)?,
        duration,
        variants: Vec::new(),
        view: VodAnalysisView {
            vod_url: raw.into(),
            title: title.into(),
            streamer: channel.clone(),
            streamer_id: channel,
            // Sprite sheets are not covers; the core extracts one bounded frame from HLS.
            thumbnail_url: Some(media_url(source)?.to_string()),
            part_count: 1,
            parts: vec![VodPartInfo {
                part: 1,
                duration_seconds: duration,
            }],
            qualities: Vec::new(),
        },
    })
}

async fn wait_cancel(cancel: &AtomicBool) {
    while !cancel.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn body(response: reqwest::Response) -> Result<Vec<u8>> {
    let mut response = response;
    if !response.status().is_success() {
        match response.status().as_u16() {
            401 => bail!(
                "KICK 인증을 확인하지 못했습니다. 세션 유효성 또는 계정 권한을 확인하세요 (HTTP 401)."
            ),
            403 => {
                let challenge = response
                    .headers()
                    .get("cf-mitigated")
                    .is_some_and(|value| value == "challenge");
                let reason = if challenge {
                    "cloudflare_challenge"
                } else {
                    "access_denied"
                };
                bail!(
                    "KICK 접근이 거부되었습니다. 시청 권한 또는 Cloudflare 제한을 확인하세요 (HTTP 403; reason={reason})."
                );
            }
            404 => {
                bail!("KICK 영상을 찾지 못했습니다. 삭제 또는 API 변경을 확인하세요 (HTTP 404).")
            }
            429 => bail!("KICK 요청 제한입니다. 잠시 후 다시 시도하세요 (HTTP 429)."),
            status => bail!("KICK 요청 실패 (HTTP {status})."),
        }
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("KICK 응답 수신 실패"))?
    {
        if chunk.len() > BODY_LIMIT.saturating_sub(bytes.len()) {
            bail!("KICK 응답 크기 제한을 초과했습니다.");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn request_failure(stage: &str, error: &reqwest::Error) -> anyhow::Error {
    let reason = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_builder() {
        "request_config"
    } else {
        "transport"
    };
    // reqwest errors can carry credential-bearing URLs; log only fixed categories.
    anyhow::anyhow!("[{stage}] KICK 요청 실패 ({reason})")
}

fn playback_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(crate::support::PROVIDER_USER_AGENT)
        .http1_only()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
}

fn playback_request(
    client: &reqwest::Client,
    channel: &str,
    id: &str,
    token: &str,
) -> Result<reqwest::RequestBuilder> {
    let mut request = client
        .post(format!("https://web.kick.com/api/v1/stream/{id}/playback"))
        .header(reqwest::header::ACCEPT, "application/json")
        .header("Origin", "https://kick.com")
        .header("Referer", format!("https://kick.com/{channel}/videos/{id}"))
        .json(&playback_body(channel, id));
    if !token.is_empty() {
        request = request.headers(auth_headers(token)?);
    }
    Ok(request)
}

fn media_request(client: &reqwest::Client, url: Url) -> reqwest::RequestBuilder {
    client
        .get(url)
        .header("Origin", "https://kick.com")
        .header("Referer", "https://kick.com/")
}

async fn load_metadata(raw: &str, cancel: &AtomicBool) -> Result<Metadata> {
    let (channel, id) = parse_url(raw)?;
    let store = crate::store::global()?;
    let token = crate::security::read_kick_token_cancellable(&store, cancel).await?;
    let client = playback_client_builder()
        .build()
        .map_err(|_| anyhow::anyhow!("KICK 요청 초기화 실패"))?;
    let operation = async {
        let request = playback_request(&client, &channel, &id, &token)?;
        let bytes = body(
            request
                .send()
                .await
                .map_err(|error| request_failure("playback.request", &error))?,
        )
        .await
        .map_err(|error| error.context("[playback.http]"))?;
        let value = serde_json::from_slice(&bytes).map_err(|error| {
            anyhow::anyhow!(
                "[playback.json] KICK 응답 JSON 형식 오류 (line={}, column={})",
                error.line(),
                error.column()
            )
        })?;
        let mut metadata = parse_playback(raw, &value, !token.is_empty())?;
        // Never attach account credentials to CDN requests.
        let bytes = body(
            media_request(&client, metadata.source.clone())
                .send()
                .await
                .map_err(|error| request_failure("cdn.hls.request", &error))?,
        )
        .await
        .map_err(|error| error.context("[cdn.hls.http]"))?;
        let playlist =
            std::str::from_utf8(&bytes).map_err(|_| anyhow::anyhow!("KICK HLS 형식 오류"))?;
        metadata.variants = parse_variants(&metadata.source, playlist)?;
        metadata.view.thumbnail_url = Some(select_variant(&metadata, "best")?.to_string());
        metadata.view.qualities = metadata.variants.iter().map(|v| v.0.clone()).collect();
        metadata.view.qualities.insert(
            0,
            VodQualityOption {
                value: "best".into(),
                label: "최고 화질".into(),
            },
        );
        Ok(metadata)
    };
    tokio::select! { result = operation => result, _ = wait_cancel(cancel) => bail!("KICK VOD 조회가 취소되었습니다.") }
}

fn parse_variants(base: &Url, playlist: &str) -> Result<Vec<(VodQualityOption, Url, u64)>> {
    if !playlist.trim_start().starts_with("#EXTM3U") {
        bail!("KICK HLS 응답이 아닙니다.");
    }
    let mut pending = None;
    let mut result = Vec::new();
    for line in playlist.lines().map(str::trim) {
        if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let bandwidth = attrs
                .split(',')
                .find_map(|a| a.strip_prefix("BANDWIDTH="))
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let height = attrs
                .split(',')
                .find_map(|a| a.strip_prefix("RESOLUTION="))
                .and_then(|s| s.split_once('x'))
                .and_then(|(_, h)| h.parse::<u32>().ok());
            let fps = attrs
                .split(',')
                .find_map(|a| a.strip_prefix("FRAME-RATE="))
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            pending = Some((bandwidth, height, fps));
        } else if !line.is_empty()
            && !line.starts_with('#')
            && let Some((bandwidth, height, fps)) = pending.take()
        {
            let url = base
                .join(line)
                .map_err(|_| anyhow::anyhow!("KICK 화질 주소 형식 오류"))?;
            let url = media_url(url.as_str())?;
            let value = format!(
                "variant-{}-{}-{}",
                height.unwrap_or(0),
                fps.round() as u32,
                bandwidth
            );
            let label = height
                .map(|h| {
                    format!(
                        "{h}p{}",
                        if fps > 30.0 {
                            format!("{}", fps.round() as u32)
                        } else {
                            String::new()
                        }
                    )
                })
                .unwrap_or_else(|| format!("{} kbps", bandwidth / 1000));
            result.push((VodQualityOption { value, label }, url, bandwidth));
        }
    }
    if result.is_empty() {
        bail!("KICK HLS 화질 목록이 없습니다.");
    }
    Ok(result)
}

pub(crate) fn validate_download_request(req: &VodDownloadRequest) -> Result<()> {
    parse_url(&req.vod_url)?;
    if req.output_directory.trim().is_empty() {
        bail!("VOD 저장 폴더를 선택하세요.");
    }
    if req.parts.iter().any(|n| *n != 1) {
        bail!("KICK VOD는 PART 1만 지원합니다.");
    }
    if req.max_retries > 20 {
        bail!("재시도 횟수는 20 이하여야 합니다.");
    }
    if req.quality != "best"
        && req.quality != "worst"
        && !req.quality.strip_prefix("variant-").is_some_and(|s| {
            s.split('-').count() == 3 && s.split('-').all(|v| v.parse::<u64>().is_ok())
        })
    {
        bail!("지원하지 않는 KICK 화질입니다.");
    }
    Ok(())
}

async fn run_analysis(
    _backend: &Path,
    req: VodAnalyzeRequest,
    _logs: &LogBuffer,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
) -> Result<()> {
    let metadata = load_metadata(&req.vod_url, cancel).await?;
    let mut state = status.write().await;
    state.analysis = Some(metadata.view);
    state.state = "READY".into();
    state.message = "KICK VOD 분석 완료 · 화질을 선택하세요.".into();
    state.percent = 100.0;
    state.part_count = 1;
    Ok(())
}

async fn run_download(
    backend: &Path,
    req: VodDownloadRequest,
    logs: &LogBuffer,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
) -> Result<()> {
    validate_download_request(&req)?;
    let yt_dlp = crate::tool_discovery::resolve_tool(
        crate::tool_discovery::ToolKind::YtDlp,
        backend,
        &[("YT_DLP_PATH", &req.yt_dlp_path)],
    )
    .path
    .ok_or_else(|| anyhow::anyhow!("yt-dlp 경로를 설정하세요."))?;
    let metadata = load_metadata(&req.vod_url, cancel).await?;
    let variant = select_variant(&metadata, &req.quality)?;
    let fragment_count = expected_fragment_count(variant, metadata.duration, cancel).await?;
    let dir = PathBuf::from(req.output_directory.trim());
    std::fs::create_dir_all(&dir)?;
    download_with_retries(
        &yt_dlp,
        &metadata,
        &req,
        fragment_count,
        status,
        cancel,
        logs,
    )
    .await
}

async fn download_with_retries(
    tool: &Path,
    metadata: &Metadata,
    req: &VodDownloadRequest,
    fragment_count: u64,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<()> {
    let variant = select_variant(metadata, &req.quality)?;
    let dir = PathBuf::from(req.output_directory.trim());
    let attempts = req.max_retries.max(1);
    for attempt in 1..=attempts {
        if cancel.load(Ordering::Acquire) {
            return Ok(());
        }
        let stem = output_stem(
            &metadata.view.streamer,
            &metadata.view.title,
            Uuid::new_v4(),
        );
        let output = dir.join(format!("{stem}.partial.ts"));
        // Each attempt owns a new file; failed attempts and sidecars remain untouched.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)?;
        {
            let mut state = status.write().await;
            state.analysis = Some(metadata.view.clone());
            state.current_part = 1;
            state.part_count = 1;
            state.state = "DOWNLOADING".into();
            state.percent = 0.0;
            state.message = format!("KICK VOD 다운로드 중 ({attempt}/{attempts})");
            state.output_file = Some(output.display().to_string());
        }
        match download_ts(
            tool,
            variant,
            &output,
            metadata.duration,
            fragment_count,
            status,
            cancel,
        )
        .await
        {
            Ok(()) => {
                if cancel.load(Ordering::Acquire) {
                    return Ok(());
                }
                // Publication failures are not download retries.
                let final_path = dir.join(format!("{stem}.ts"));
                publish_ts(&output, &final_path)?;
                let mut state = status.write().await;
                state.state = "COMPLETED".into();
                state.percent = 100.0;
                state.output_file = Some(final_path.display().to_string());
                state.message = "KICK VOD 다운로드가 완료되었습니다.".into();
                return Ok(());
            }
            Err(error) => {
                if cancel.load(Ordering::Acquire) {
                    return Ok(());
                }
                // Raw tool stderr/URLs/credentials must never enter the log.
                logs.push(format!(
                    "[VOD:KICK:WARN] 다운로드 시도 실패 ({attempt}/{attempts}); 부분 파일 보존"
                ))
                .await;
                if attempt == attempts {
                    return Err(error);
                }
                {
                    let mut state = status.write().await;
                    state.message = format!(
                        "KICK VOD 재시도 대기 ({}/{attempts}) · 이전 부분 파일 보존",
                        attempt + 1
                    );
                }
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(u64::from(attempt).min(5))) => {},
                    _ = wait_cancel(cancel) => return Ok(()),
                }
            }
        }
    }
    unreachable!("at least one attempt")
}

fn publish_ts(source: &Path, target: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        // MoveFileW has no replace-existing flag; existing files are never overwritten.
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileW(source.as_ptr(), target.as_ptr())
        } == 0
        {
            bail!("KICK TS 저장 마무리 실패. partial.ts 파일은 보존됩니다.");
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::hard_link(source, target).map_err(|_| {
            anyhow::anyhow!("KICK TS 저장 마무리 실패. partial.ts 파일은 보존됩니다.")
        })?;
        std::fs::remove_file(source)?;
    }
    Ok(())
}

fn select_variant<'a>(metadata: &'a Metadata, quality: &str) -> Result<&'a Url> {
    let chosen = match quality {
        "best" => metadata.variants.iter().max_by_key(|v| v.2),
        "worst" => metadata.variants.iter().min_by_key(|v| v.2),
        s => metadata.variants.iter().find(|v| v.0.value == s),
    };
    chosen
        .map(|v| &v.1)
        .ok_or_else(|| anyhow::anyhow!("KICK 화질 목록이 변경되었습니다. 다시 분석하세요."))
}

fn output_stem(streamer: &str, title: &str, id: Uuid) -> String {
    let id = id.to_string();
    let mut prefix = format!("{}_{}", safe_name(streamer), safe_name(title));
    // Reserve the longest suffix and UUID within the common Unix component limit.
    let budget = 255 - "_".len() - id.len() - ".partial.ts.part-Frag1234567890.part".len();
    let mut end = prefix.len().min(budget);
    while !prefix.is_char_boundary(end) {
        end -= 1;
    }
    prefix.truncate(end);
    format!("{}_{}", prefix.trim_end_matches([' ', '.']), id)
}

fn safe_name(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(60)
        .collect::<String>()
        .trim_matches([' ', '.'])
        .to_owned()
}

fn validate_media_playlist(playlist: &str, duration: u64) -> Result<u64> {
    if !playlist.trim_start().starts_with("#EXTM3U")
        || !playlist.lines().any(|line| line.trim() == "#EXT-X-ENDLIST")
    {
        bail!("[download.playlist] KICK VOD의 완료된 playlist가 아닙니다.");
    }
    let mut count = 0u64;
    let mut total = 0.0;
    let mut pending = false;
    for line in playlist.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("#EXTINF:") {
            if pending {
                bail!("KICK HLS 조각 주소가 누락되었습니다.");
            }
            let seconds = value.split(',').next().unwrap_or("").parse::<f64>()?;
            if !seconds.is_finite() || seconds <= 0.0 {
                bail!("KICK HLS 조각 길이가 잘못되었습니다.");
            }
            total += seconds;
            pending = true;
        } else if !line.is_empty() && !line.starts_with('#') {
            if !pending {
                bail!("KICK HLS 조각 길이가 누락되었습니다.");
            }
            count += 1;
            pending = false;
        }
    }
    if pending || count == 0 || !total.is_finite() || (total - duration as f64).abs() > 10.0 {
        bail!(
            "[download.playlist] KICK playlist가 영상 전체 길이와 일치하지 않습니다. 다시 분석하세요."
        );
    }
    Ok(count)
}

async fn expected_fragment_count(source: &Url, duration: u64, cancel: &AtomicBool) -> Result<u64> {
    let operation = async {
        let client = playback_client_builder().build()?;
        let bytes = body(
            media_request(&client, source.clone())
                .send()
                .await
                .map_err(|error| request_failure("download.playlist.request", &error))?,
        )
        .await?;
        let playlist =
            std::str::from_utf8(&bytes).map_err(|_| anyhow::anyhow!("KICK HLS 형식 오류"))?;
        validate_media_playlist(playlist, duration)
    };
    tokio::select! { result = operation => result, _ = wait_cancel(cancel) => bail!("KICK VOD 조회가 취소되었습니다.") }
}

fn yt_dlp_command(tool: &Path, source: &Url, output: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(tool);
    command
        .args([
            "--ignore-config",
            "--no-playlist",
            "--downloader",
            "m3u8:native",
            "--concurrent-fragments",
            "4",
            "--hls-use-mpegts",
            "--fixup",
            "never",
            "--abort-on-unavailable-fragments",
            "--force-overwrites",
            "--newline",
            "--progress",
            "--progress-template",
            "download:KICK_PROGRESS:%(progress.fragment_index)s:%(progress.fragment_count)s",
            "--print",
            "after_video:KICK_DONE %(id)s",
            "--no-simulate",
            "--user-agent",
            crate::support::PROVIDER_USER_AGENT,
            "--add-header",
            "Origin:https://kick.com",
            "--referer",
            "https://kick.com/",
            "-o",
        ])
        .arg(output.to_string_lossy().replace('%', "%%"))
        .arg(source.as_str())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    command
}

fn partial_size(output: &Path) -> u64 {
    let mut part = output.as_os_str().to_os_string();
    part.push(".part");
    std::fs::metadata(output).map(|m| m.len()).unwrap_or(0).max(
        std::fs::metadata(Path::new(&part))
            .map(|m| m.len())
            .unwrap_or(0),
    )
}

fn fragment_progress(line: &str, expected: u64) -> Option<f64> {
    let (index, count) = line.strip_prefix("KICK_PROGRESS:")?.split_once(':')?;
    let index = index.parse::<u64>().ok()?;
    let count = count.parse::<u64>().ok()?;
    (count > 0 && count == expected && index <= count).then_some(index as f64 / count as f64)
}

async fn download_ts(
    tool: &Path,
    source: &Url,
    output: &Path,
    duration: u64,
    fragment_count: u64,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    if cancel.load(Ordering::Acquire) {
        return Ok(());
    }
    let (mut child, mut tree) =
        crate::platform_runtime::spawn_owned(&mut yt_dlp_command(tool, source, output))
            .await
            .map_err(|_| anyhow::anyhow!("KICK yt-dlp 시작 실패"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("KICK 진행률 파이프를 열지 못했습니다."))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("KICK 진행률 파이프 오류"))?;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<f64>(32);
    fn reader_task<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
        stream: R,
        tx: tokio::sync::mpsc::Sender<f64>,
        expected: u64,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let progress = if line.starts_with("KICK_DONE ") {
                    Some(2.0)
                } else {
                    fragment_progress(&line, expected)
                };
                if let Some(progress) = progress
                    && tx.send(progress).await.is_err()
                {
                    break;
                }
            }
        })
    }
    let reader = reader_task(stdout, tx.clone(), fragment_count);
    let error_reader = reader_task(stderr, tx, fragment_count);
    let mut seconds = 0.0;
    let mut completed = false;
    let mut cancel_deadline = None;
    let result = loop {
        while let Ok(time) = rx.try_recv() {
            completed |= time == 2.0;
            if time != 2.0 {
                seconds = time * duration as f64;
            }
            let mut state = status.write().await;
            state.percent = (seconds / duration as f64 * 100.0).clamp(0.0, 99.0);
            let size = partial_size(output);
            state.message = format!(
                "KICK VOD 다운로드 중 · {:.1}% · {:.1} MB",
                state.percent,
                size as f64 / 1_048_576.0
            );
        }
        if cancel.load(Ordering::Acquire) && cancel_deadline.is_none() {
            let _ = tree.terminate(&mut child).await;
            cancel_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(5));
        }
        match child.try_wait() {
            Ok(Some(exit)) => {
                let _ = tree.terminate_now();
                if cancel.load(Ordering::Acquire) {
                    break Ok(());
                }
                if !exit.success() {
                    let size = partial_size(output);
                    break Err(anyhow::anyhow!(
                        "[download.ytdlp.exit] KICK yt-dlp 다운로드 실패 (exit={exit}, bytes={size}, media_seconds={seconds:.1}). partial.ts / .part 부분 파일은 보존됩니다. 재시도는 새 파일로 시작합니다."
                    ));
                }
                break Ok(());
            }
            Err(_) => {
                let _ = tree.terminate(&mut child).await;
                break Err(anyhow::anyhow!("KICK yt-dlp 상태 확인 실패"));
            }
            _ => {}
        }
        if cancel_deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
            let _ = tree.terminate(&mut child).await;
            break Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    // Drain progress already queued before EOF. Do not wait on orphan pipe handles.
    let mut reader = reader;
    let mut error_reader = error_reader;
    let drain = async {
        while let Some(time) = rx.recv().await {
            completed |= time == 2.0;
            if time != 2.0 {
                seconds = time * duration as f64;
            }
        }
        let _ = (&mut reader).await;
        let _ = (&mut error_reader).await;
    };
    if tokio::time::timeout(Duration::from_secs(2), drain)
        .await
        .is_err()
    {
        reader.abort();
        error_reader.abort();
        let _ = reader.await;
        let _ = error_reader.await;
    }
    result?;
    if cancel.load(Ordering::Acquire) {
        return Ok(());
    }
    if std::fs::metadata(output).map(|m| m.len()).unwrap_or(0) == 0
        || !completed
        || seconds < duration as f64
    {
        bail!(
            "[download.incomplete] KICK 다운로드가 영상 끝까지 도달하지 못했습니다. partial.ts / .part 부분 파일은 보존됩니다."
        );
    }
    use std::io::Read;
    let mut header = [0u8; 377];
    std::fs::File::open(output)?.read_exact(&mut header)?;
    if header[0] != 0x47 || header[188] != 0x47 || header[376] != 0x47 {
        bail!("KICK 출력이 MPEG-TS가 아닙니다. 부분 파일은 보존됩니다.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn playback_wire_headers_and_cdn_credential_boundary() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for authenticated in [true, false] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 2048];
                    let count = socket.read(&mut buffer).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    assert!(bytes.len() < 65536);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(bytes).unwrap();
                let headers = request.split("\r\n\r\n").next().unwrap();
                assert!(headers.lines().next().unwrap().ends_with("HTTP/1.1"));
                let get = |name: &str| {
                    headers.lines().find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case(name).then(|| value.trim())
                    })
                };
                assert_eq!(get("user-agent"), Some(crate::support::PROVIDER_USER_AGENT));
                if authenticated {
                    assert_eq!(get("accept"), Some("application/json"));
                    assert_eq!(get("origin"), Some("https://kick.com"));
                    assert_eq!(get("authorization"), Some("Bearer 123|fixture"));
                    assert_eq!(get("cookie"), Some("session_token=123%7Cfixture"));
                    assert!(get("referer").unwrap().contains("/example/videos/"));
                    let payload: serde_json::Value =
                        serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
                    assert_eq!(payload["video_session"]["page_type"], "video");
                } else {
                    assert_eq!(get("authorization"), None);
                    assert_eq!(get("cookie"), None);
                }
                socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                    )
                    .await
                    .unwrap();
            }
        });
        let client = playback_client_builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let mut request = playback_request(
            &client,
            "example",
            "01a106d1-f328-750c-a31b-16a5df570460",
            "123%7Cfixture",
        )
        .unwrap()
        .build()
        .unwrap();
        *request.url_mut() = Url::parse(&format!("http://{address}/playback")).unwrap();
        assert_eq!(client.execute(request).await.unwrap().status(), 200);
        assert_eq!(
            media_request(
                &client,
                Url::parse(&format!("http://{address}/cdn")).unwrap()
            )
            .send()
            .await
            .unwrap()
            .status(),
            200
        );
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn composed_output_names_fit_byte_limit_without_losing_uuid_or_utf8() {
        let id = Uuid::new_v4();
        for title in ["한".repeat(60), "🎥".repeat(60), "é".repeat(60)] {
            let stem = output_stem(&"a".repeat(60), &title, id);
            let partial = format!("{stem}.partial.ts");
            let final_name = format!("{stem}.ts");
            assert!(partial.len() <= 255);
            assert!(final_name.len() <= 255);
            assert!(stem.ends_with(&id.to_string()));
            assert_ne!(stem, output_stem(&"a".repeat(60), &title, Uuid::new_v4()));
            #[cfg(unix)]
            {
                let dir = tempfile::tempdir().unwrap();
                let source = dir.path().join(partial);
                let target = dir.path().join(final_name);
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&source)
                    .unwrap();
                publish_ts(&source, &target).unwrap();
                assert!(target.exists());
                assert!(!source.exists());
            }
        }
    }

    #[test]
    fn request_error_diagnostics_never_echo_urls_or_tokens() {
        let error = reqwest::Client::new()
            .get("https://example.invalid/?token=private-value")
            .header("authorization", "invalid\nprivate-value")
            .build()
            .unwrap_err();
        let message = request_failure("playback.request", &error).to_string();
        assert!(message.contains("[playback.request]"));
        assert!(message.contains("request_config"));
        assert!(!message.contains("private-value"));
        assert!(!message.contains("example.invalid"));
    }

    #[test]
    fn observer_token_changes_reach_owner_without_watcher() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let owner = crate::store::Store::open(path.clone()).unwrap();
        let observer = crate::store::Store::open_observer(path).unwrap();
        assert_eq!(crate::security::read_kick_token(&owner).unwrap(), "");
        for value in ["first%7Ctoken", "replacement%7Ctoken", ""] {
            let cached = owner.setting_value(TOKEN_KEY).unwrap().unwrap_or_default();
            observer
                .sync_settings(
                    &std::collections::BTreeMap::from([(TOKEN_KEY.into(), value.into())]),
                    "test-observer",
                )
                .unwrap();
            assert_eq!(
                owner.setting_value(TOKEN_KEY).unwrap().unwrap_or_default(),
                cached
            );
            assert_eq!(crate::security::read_kick_token(&owner).unwrap(), value);
        }
    }

    const VOD: &str = "https://kick.com/example/videos/01a106d1-f328-750c-a31b-16a5df570460";
    fn playback() -> serde_json::Value {
        serde_json::json!({"playback_url":{"vod":"https://stream.kick.com/media/hls/master.m3u8"},
            "video_session":{"video_id":"01a106d1-f328-750c-a31b-16a5df570460","video_series":"example",
                "video_stream_status":"vod","video_encryption_type":"NONE","video_title":"Example","video_duration":300}})
    }
    #[test]
    fn auth_decodes_only_bearer_and_never_allows_header_injection() {
        let headers = auth_headers("123%7Cexample+value").unwrap();
        assert_eq!(headers["authorization"], "Bearer 123|example+value");
        assert_eq!(headers["cookie"], "session_token=123%7Cexample+value");
        assert!(headers["authorization"].is_sensitive());
        for token in ["", "x;y", "x%0D%0Ay", "x%GG", "x\ny"] {
            assert!(auth_headers(token).is_err());
        }
    }
    #[test]
    fn playback_checks_identity_access_and_vod_not_live() {
        let mut value = playback();
        value["playback_url"]["live"] = "https://example.com/not-vod.m3u8".into();
        assert_eq!(parse_playback(VOD, &value, true).unwrap().duration, 300);
        value["playback_url"]["vod"] = "".into();
        assert!(
            parse_playback(VOD, &value, true)
                .err()
                .unwrap()
                .to_string()
                .contains("session_token")
        );
    }
    #[test]
    fn rejects_wrong_video_drm_and_untrusted_cdn() {
        for (key, val) in [
            ("video_id", "other"),
            ("video_series", "other"),
            ("video_encryption_type", "DRM"),
            ("video_stream_status", "live"),
        ] {
            let mut value = playback();
            value["video_session"][key] = val.into();
            assert!(parse_playback(VOD, &value, false).is_err());
        }
        for url in [
            "http://stream.kick.com/a.m3u8",
            "https://stream.kick.com.evil/a.m3u8",
            "https://user:secret@stream.kick.com/a.m3u8",
            "https://localhost/a.m3u8",
        ] {
            assert!(media_url(url).is_err());
        }
        assert!(parse_url("https://kick.com/example").is_err());
        assert!(parse_url("https://kick.com/example/videos/not-a-uuid").is_err());
    }
    #[test]
    fn qualities_resolve_relative_urls_and_reject_foreign_variants() {
        let base = media_url("https://stream.kick.com/hls/master.m3u8").unwrap();
        let variants = parse_variants(&base,"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=8000000,RESOLUTION=1920x1080,FRAME-RATE=60\n1080/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\n720/index.m3u8").unwrap();
        assert_eq!(variants[0].0.label, "1080p60");
        assert_eq!(
            variants[0].1.as_str(),
            "https://stream.kick.com/hls/1080/index.m3u8"
        );
        let mut metadata = parse_playback(VOD, &playback(), false).unwrap();
        metadata.variants = variants;
        assert!(
            select_variant(&metadata, "best")
                .unwrap()
                .path()
                .contains("1080")
        );
        assert!(
            select_variant(&metadata, "variant-720-0-2000000")
                .unwrap()
                .path()
                .contains("720")
        );
        metadata.variants.reverse();
        assert!(
            select_variant(&metadata, "variant-720-0-2000000")
                .unwrap()
                .path()
                .contains("720")
        );
        assert!(select_variant(&metadata, "variant-480-0-1").is_err());
        assert!(
            parse_variants(
                &base,
                "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nhttps://localhost/x.m3u8"
            )
            .is_err()
        );
    }
    #[test]
    fn media_playlist_requires_endlist_full_duration_and_each_segment() {
        let full = "#EXTM3U\n#EXTINF:30,\n0.ts\n#EXTINF:30,\n1.ts\n#EXT-X-ENDLIST\n";
        assert_eq!(validate_media_playlist(full, 60).unwrap(), 2);
        for bad in [
            full.replace("#EXT-X-ENDLIST", ""),
            full.replace("#EXTINF:30,", "#EXTINF:1,"),
            full.replace("#EXTINF:30,", "#EXTINF:NaN,"),
            full.replace("0.ts\n", ""),
        ] {
            assert!(validate_media_playlist(&bad, 60).is_err());
        }
        assert_eq!(fragment_progress("KICK_PROGRESS:7:7", 8), None);
    }
    #[test]
    fn fragment_progress_rejects_missing_zero_and_invalid_counts() {
        assert_eq!(fragment_progress("KICK_PROGRESS:4:8", 8), Some(0.5));
        assert_eq!(fragment_progress("KICK_PROGRESS:8:8", 8), Some(1.0));
        for line in [
            "KICK_PROGRESS:NA:NA",
            "KICK_PROGRESS:1:0",
            "KICK_PROGRESS:9:8",
            "http://secret",
        ] {
            assert_eq!(fragment_progress(line, 8), None);
        }
    }
    #[test]
    fn output_template_treats_percent_in_user_paths_as_literal() {
        let command = yt_dlp_command(
            Path::new("yt-dlp"),
            &media_url("https://stream.kick.com/hls/index.m3u8").unwrap(),
            Path::new("folder 100%/title %(id)s.partial.ts"),
        );
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(
            args.windows(2)
                .any(|a| a == ["-o", "folder 100%%/title %%(id)s.partial.ts"])
        );
    }
    #[test]
    fn command_is_parallel_ts_without_credentials_or_second_full_file() {
        let command = yt_dlp_command(
            Path::new("yt-dlp"),
            &media_url("https://stream.kick.com/hls/index.m3u8").unwrap(),
            Path::new("out.partial.ts"),
        );
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(
            args.windows(2)
                .any(|a| a == ["--concurrent-fragments", "4"])
        );
        assert!(
            args.windows(2)
                .any(|a| { a == ["--user-agent", crate::support::PROVIDER_USER_AGENT] })
        );
        assert!(
            args.windows(2)
                .any(|a| a == ["--add-header", "Origin:https://kick.com"])
        );
        assert!(args.iter().any(|a| a == "--hls-use-mpegts"));
        assert!(args.iter().any(|a| a == "--abort-on-unavailable-fragments"));
        assert!(
            !args
                .iter()
                .any(|a| a.contains("Cookie") || a.contains("Bearer") || a.contains("faststart"))
        );
    }
    #[tokio::test]
    async fn direct_process_checks_ts_and_complete_fragments_and_preserves_failures() {
        use crate::{test_support::ProviderFixture, tool_discovery::ToolKind};
        let fixture = ProviderFixture::new();
        let ffmpeg = fixture.tool(ToolKind::YtDlp);
        let source = media_url("https://stream.kick.com/hls/index.m3u8").unwrap();
        let status = Arc::new(RwLock::new(VodJobStatus::default()));
        let cancel = AtomicBool::new(false);
        for (mode, success) in [
            ("kick-direct-success", true),
            ("kick-direct-truncated", false),
            ("kick-direct-invalid", false),
            ("run-partial-fail", false),
        ] {
            fixture.set_mode(&ffmpeg, mode);
            let output = fixture.root().join(format!("{mode}.partial.ts"));
            let result = download_ts(&ffmpeg, &source, &output, 60, 10, &status, &cancel).await;
            assert_eq!(result.is_ok(), success, "{mode}");
            assert!(output.exists(), "partial output must be preserved");
        }
        assert!(!fixture.invocations().contains("session_token"));
        let source = fixture.root().join("kick-direct-success.partial.ts");
        let target = fixture.root().join("complete.ts");
        publish_ts(&source, &target).unwrap();
        assert!(!source.exists());
        assert!(target.exists());
        let another = fixture.root().join("another.ts");
        std::fs::write(&another, b"other").unwrap();
        assert!(publish_ts(&another, &target).is_err());
        assert_eq!(std::fs::read(&another).unwrap(), b"other");
    }

    #[tokio::test]
    async fn retries_use_fresh_files_and_honor_attempt_limit() {
        use crate::{test_support::ProviderFixture, tool_discovery::ToolKind};
        for (mode, limit, expected, succeeds) in [
            ("kick-retry-success", 3, 2, true),
            ("run-partial-fail", 2, 2, false),
            ("run-partial-fail", 0, 1, false),
        ] {
            let fixture = ProviderFixture::new();
            let tool = fixture.tool(ToolKind::YtDlp);
            fixture.set_mode(&tool, mode);
            let mut metadata = parse_playback(
                "https://kick.com/example/videos/01a106d1-f328-750c-a31b-16a5df570460",
                &playback(),
                false,
            )
            .unwrap();
            metadata.duration = 60;
            metadata.variants = parse_variants(
                &metadata.source,
                "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=100,RESOLUTION=1920x1080\nindex.m3u8\n",
            )
            .unwrap();
            let req = VodDownloadRequest {
                output_directory: fixture.root().display().to_string(),
                quality: "best".into(),
                max_retries: limit,
                ..serde_json::from_value(serde_json::json!({"vod_url":"", "output_directory":""}))
                    .unwrap()
            };
            let status = Arc::new(RwLock::new(VodJobStatus::default()));
            let cancel = AtomicBool::new(false);
            let result = download_with_retries(
                &tool,
                &metadata,
                &req,
                10,
                &status,
                &cancel,
                &LogBuffer::new(),
            )
            .await;
            assert_eq!(result.is_ok(), succeeds, "{mode}, limit={limit}");
            let outputs: Vec<_> = std::fs::read_dir(fixture.root())
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().is_some_and(|e| e == "ts"))
                .collect();
            assert_eq!(outputs.len(), expected);
            assert_eq!(
                outputs
                    .iter()
                    .filter(|p| !p.to_string_lossy().ends_with(".partial.ts"))
                    .count(),
                usize::from(succeeds)
            );
            for path in outputs
                .iter()
                .filter(|p| p.to_string_lossy().ends_with(".partial.ts"))
            {
                assert_eq!(std::fs::metadata(path).unwrap().len(), 64 * 1024);
            }
            assert_eq!(status.read().await.state == "COMPLETED", succeeds);
        }
    }

    #[tokio::test]
    async fn cancellation_during_retry_wait_does_not_start_another_attempt() {
        use crate::{test_support::ProviderFixture, tool_discovery::ToolKind};
        let fixture = ProviderFixture::new();
        let tool = fixture.tool(ToolKind::YtDlp);
        fixture.set_mode(&tool, "run-partial-fail");
        let mut metadata = parse_playback(
            "https://kick.com/example/videos/01a106d1-f328-750c-a31b-16a5df570460",
            &playback(),
            false,
        )
        .unwrap();
        metadata.variants = parse_variants(
            &metadata.source,
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=100\nindex.m3u8\n",
        )
        .unwrap();
        let req = VodDownloadRequest {
            output_directory: fixture.root().display().to_string(),
            quality: "best".into(),
            max_retries: 5,
            ..serde_json::from_value(serde_json::json!({"vod_url":"", "output_directory":""}))
                .unwrap()
        };
        let status = Arc::new(RwLock::new(VodJobStatus::default()));
        let cancel = AtomicBool::new(false);
        let logs = LogBuffer::new();
        let operation = download_with_retries(&tool, &metadata, &req, 10, &status, &cancel, &logs);
        let stop = async {
            tokio::time::timeout(Duration::from_secs(10), async {
                while !status.read().await.message.contains("재시도 대기") {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            cancel.store(true, Ordering::Release);
        };
        let (result, ()) = tokio::join!(operation, stop);
        result.unwrap();
        assert_ne!(status.read().await.state, "COMPLETED");
        let partials = std::fs::read_dir(fixture.root())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .to_string_lossy()
                    .ends_with(".partial.ts")
            })
            .count();
        assert_eq!(partials, 1);
    }

    #[tokio::test]
    async fn cancel_terminates_only_owned_descendants() {
        use crate::{test_support::ProviderFixture, tool_discovery::ToolKind};
        let fixture = ProviderFixture::new();
        let ffmpeg = fixture.tool(ToolKind::YtDlp);
        fixture.set_mode(&ffmpeg, "run-spawn-child");
        let mut unrelated = fixture.spawn_unrelated();
        fixture.wait_for_unrelated().await;
        let source = media_url("https://stream.kick.com/hls/index.m3u8").unwrap();
        let output = fixture.root().join("cancel.partial.ts");
        let status = Arc::new(RwLock::new(VodJobStatus::default()));
        let cancel = AtomicBool::new(false);
        let operation = download_ts(&ffmpeg, &source, &output, 60, 10, &status, &cancel);
        let trigger = async {
            fixture
                .wait_for_path(&fixture.child_ready_path(&ffmpeg))
                .await;
            cancel.store(true, Ordering::Release);
        };
        let (result, ()) = tokio::join!(operation, trigger);
        result.unwrap();
        fixture.assert_child_stopped(&ffmpeg).await;
        assert!(unrelated.try_wait().unwrap().is_none());
        unrelated.kill().unwrap();
        unrelated.wait().unwrap();
    }
}
