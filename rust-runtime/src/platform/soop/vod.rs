mod cookies;
mod parser;

use cookies::*;
use parser::*;

use super::super::{
    PlatformId,
    vod::{ProviderVodConfig, ProviderVodRunner},
};
use crate::platform_runtime::spawn_owned;
use crate::{
    backend::LogBuffer,
    media_process::{MediaProcessOutcome, MediaProcessSpec, run_media_process_with_atomic_cancel},
    model::{
        VodAnalysisView, VodAnalyzeRequest, VodDownloadRequest, VodJobStatus, VodQualityOption,
    },
    security::read_provider_settings,
    store,
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use regex::Regex;
use reqwest::{
    Client,
    header::{COOKIE, REFERER},
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    process::Command,
    sync::{Mutex, RwLock},
};
use url::Url;
use uuid::Uuid;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36";
const PROVIDER_CAPTURE_TIMEOUT: Duration = Duration::from_secs(120);
const PROVIDER_CAPTURE_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
struct Tools {
    yt_dlp: PathBuf,
    ffmpeg: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct VodMetadata {
    title: String,
    thumbnail_url: Option<String>,
    streamer: String,
    streamer_id: String,
    date: String,
    entries: Vec<VodEntry>,
    requires_private_auth: bool,
}

#[derive(Debug, Clone)]
struct VodEntry {
    manifest_url: String,
    duration_seconds: u64,
}

struct PartialDownloadGuard {
    output: PathBuf,
    completed: bool,
}

impl Drop for PartialDownloadGuard {
    fn drop(&mut self) {
        if !self.completed {
            cleanup_incomplete(&self.output);
        }
    }
}

/// A resumed PART gets a new failure budget only after retained media advances.
#[derive(Default)]
struct RetryBudget {
    consecutive_failures: u32,
    retained_bytes: u64,
}

impl RetryBudget {
    fn failed(&mut self, output: &Path) {
        let retained = fs::metadata(format!("{}.part", output.display()))
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if retained > self.retained_bytes {
            self.retained_bytes = retained;
            self.consecutive_failures = 0;
        }
        self.consecutive_failures += 1;
    }
}

struct JobDirGuard(PathBuf);
impl Drop for JobDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub struct VodManager(ProviderVodRunner);

impl VodManager {
    pub fn new(backend_dir: PathBuf, logs: LogBuffer) -> Self {
        Self::new_with_events(backend_dir, logs, Default::default())
    }

    pub(crate) fn new_with_events(
        backend_dir: PathBuf,
        logs: LogBuffer,
        events: crate::download_events::DownloadEvents,
    ) -> Self {
        Self(ProviderVodRunner::new(
            backend_dir,
            logs,
            events,
            ProviderVodConfig {
                platform: PlatformId::Soop,
                busy_message: "다른 VOD 작업이 이미 실행 중입니다.",
                starting_message: "VOD 작업 준비 중…",
                cancelling_message: "외부 프로세스 종료 중…",
                cancelled_message: "VOD 작업이 취소되었습니다.",
                cancel_log: "[VOD] job cancelled",
                error_log_prefix: "[VOD:ERR]",
                log_job_id_on_error: false,
                redact_error: redact,
            },
        ))
    }

    pub(crate) fn notification_state(
        &self,
    ) -> (&RwLock<VodJobStatus>, &std::sync::atomic::AtomicU64) {
        self.0.notification_state()
    }

    pub async fn status(&self) -> VodJobStatus {
        self.0.status().await
    }

    pub async fn terminal_status(&self, job_id: &str) -> Option<VodJobStatus> {
        self.0.terminal_status(job_id).await
    }

    pub async fn analyze(&self, req: VodAnalyzeRequest) -> Result<VodJobStatus> {
        self.0
            .start_job(false, move |backend, logs, status, cancel| async move {
                run_analysis(&backend, req, &logs, &status, &cancel)
                    .await
                    .map(|_| ())
            })
            .await
    }

    pub async fn download(&self, req: VodDownloadRequest) -> Result<VodJobStatus> {
        self.0
            .start_job(true, move |backend, logs, status, cancel| async move {
                run_download(&backend, req, &logs, &status, &cancel).await
            })
            .await
    }

    pub async fn cancel(&self) -> Result<VodJobStatus> {
        self.0.cancel().await
    }
}

#[derive(Clone, Copy)]
struct CookieSource<'a> {
    mode: &'a str,
    cookie_file: &'a str,
    browser: &'a str,
    job_dir: &'a Path,
}

async fn run_analysis(
    backend: &Path,
    req: VodAnalyzeRequest,
    logs: &LogBuffer,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
) -> Result<VodAnalysisView> {
    validate_url(&req.vod_url)?;
    validate_retries(req.max_retries)?;
    set_status(status, "ANALYZING", "VOD 분석 중…").await;
    let tools = resolve_tools(backend, &req.yt_dlp_path, &req.ffmpeg_path)?;
    let job_dir = job_dir(backend)?;
    let _job_guard = JobDirGuard(job_dir.clone());
    let mut jar = init_cookie(
        backend,
        &tools,
        &req.vod_url,
        CookieSource {
            mode: &req.cookie_mode,
            cookie_file: &req.cookie_file,
            browser: &req.browser_name,
            job_dir: &job_dir,
        },
        cancel,
        logs,
    )
    .await?;
    let cookie_file = job_dir.join("cookies.txt");
    jar.write_file(&cookie_file)?;
    let metadata =
        load_metadata(&tools, &req.vod_url, &cookie_file, &mut jar, cancel, logs).await?;
    let first = metadata
        .entries
        .first()
        .ok_or_else(|| anyhow!("VOD PART를 찾지 못했습니다."))?
        .manifest_url
        .clone();
    if first.is_empty() {
        bail!("첫 번째 PART manifest URL이 없습니다.");
    }
    let qualities = prepare_manifest(&req.vod_url, &metadata, &first, &mut jar, 1, logs).await?;
    jar.write_file(&cookie_file)?;
    let view = analysis_view(&req.vod_url, &metadata, qualities);
    {
        let mut s = status.write().await;
        s.state = "READY".into();
        s.running = false;
        s.message = "분석 완료 · PART와 화질을 선택하세요.".into();
        s.part_count = view.part_count;
        s.analysis = Some(view.clone());
        s.percent = 100.0;
        s.finished_at = Some(Utc::now().to_rfc3339());
    }
    logs.push(format!(
        "[VOD] analysis ready title={} streamer={} parts={}",
        metadata.title,
        metadata.streamer,
        metadata.entries.len()
    ))
    .await;
    Ok(view)
}

pub(crate) fn validate_download_request(req: &VodDownloadRequest) -> Result<()> {
    validate_url(&req.vod_url)?;
    validate_retries(req.max_retries)?;
    if !req.quality.is_empty() && {
        static QUALITY_RE: std::sync::LazyLock<Regex> =
            std::sync::LazyLock::new(|| Regex::new(r"^best(?:\[height<=\d+\])?$").unwrap());
        !QUALITY_RE.is_match(&req.quality)
    } {
        bail!("지원하지 않는 VOD 화질 선택입니다.");
    }
    if req.output_directory.trim().is_empty() {
        bail!("VOD 출력 폴더가 비어 있습니다.");
    }
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
    let tools = resolve_tools(backend, &req.yt_dlp_path, &req.ffmpeg_path)?;
    let output_dir = PathBuf::from(&req.output_directory);
    fs::create_dir_all(&output_dir)?;
    let job_dir = job_dir(backend)?;
    let _job_guard = JobDirGuard(job_dir.clone());
    let mut jar = init_cookie(
        backend,
        &tools,
        &req.vod_url,
        CookieSource {
            mode: &req.cookie_mode,
            cookie_file: &req.cookie_file,
            browser: &req.browser_name,
            job_dir: &job_dir,
        },
        cancel,
        logs,
    )
    .await?;
    let cookie_file = job_dir.join("cookies.txt");
    jar.write_file(&cookie_file)?;
    let metadata =
        load_metadata(&tools, &req.vod_url, &cookie_file, &mut jar, cancel, logs).await?;
    let selected = resolve_parts(&req.parts, metadata.entries.len())?;
    {
        let mut s = status.write().await;
        s.state = "DOWNLOADING".into();
        s.part_count = metadata.entries.len();
        s.message = "VOD 다운로드 시작".into();
        s.analysis = Some(analysis_view(
            &req.vod_url,
            &metadata,
            vec![VodQualityOption {
                value: "best".into(),
                label: "최고 화질 (자동)".into(),
            }],
        ));
    }
    let mut files = Vec::new();
    for part in selected {
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        let output = collision_path(
            &output_dir,
            &format!(
                "{}_{}_{:02}",
                metadata.date,
                safe_name(&metadata.streamer),
                part
            ),
            ".mp4",
        )?;
        let mut partial = PartialDownloadGuard {
            output: output.clone(),
            completed: false,
        };
        let mut complete = false;
        let mut last_error = String::new();
        let mut expired_download_auth = false;
        let mut resume_identity: Option<(String, usize, String)> = None;
        let mut budget = RetryBudget::default();
        let mut attempt = 0u32;
        while budget.consecutive_failures < req.max_retries {
            attempt = attempt.saturating_add(1);
            if cancel.load(Ordering::SeqCst) {
                cleanup_incomplete(&output);
                return Ok(());
            }
            if attempt > 1 {
                jar = renew_base_cookie(
                    backend,
                    &tools,
                    &req.vod_url,
                    CookieSource {
                        mode: &req.cookie_mode,
                        cookie_file: &req.cookie_file,
                        browser: &req.browser_name,
                        job_dir: &job_dir,
                    },
                    cancel,
                    logs,
                )
                .await?;
                jar.write_file(&cookie_file)?;
            }
            {
                let mut s = status.write().await;
                s.current_part = part;
                s.state = "REFRESHING".into();
                s.message = format!(
                    "PART {part} 최신 URL/인증 갱신 중 (시도 {attempt}, 연속 실패 {}/{})",
                    budget.consecutive_failures, req.max_retries
                );
            }
            let refreshed =
                load_metadata(&tools, &req.vod_url, &cookie_file, &mut jar, cancel, logs).await?;
            let url = refreshed
                .entries
                .get(part - 1)
                .map(|e| e.manifest_url.clone())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("PART {part} URL이 없습니다."))?;
            let mut media_url = Url::parse(&url)?;
            media_url.set_query(None);
            media_url.set_fragment(None);
            let identity = (
                refreshed.streamer_id.clone(),
                refreshed.entries.len(),
                media_url.to_string(),
            );
            if resume_identity
                .as_ref()
                .is_some_and(|previous| previous != &identity)
            {
                bail!(
                    "PART {part} 영상 주소나 목록이 변경되어 기존 조각을 안전하게 이어받을 수 없습니다. 다시 분석해 주세요."
                );
            }
            resume_identity = Some(identity);
            if expired_download_auth
                && refreshed.requires_private_auth
                && jar.has_login()
                && let Err(error) = refresh_authorization(
                    &req.vod_url,
                    &refreshed.streamer_id,
                    &url,
                    &mut jar,
                    attempt,
                    logs,
                )
                .await
            {
                last_error = error.to_string();
                budget.failed(&output);
                if budget.consecutive_failures < req.max_retries {
                    sleep_retry(budget.consecutive_failures, cancel).await;
                }
                continue;
            }
            if let Err(err) =
                prepare_manifest(&req.vod_url, &refreshed, &url, &mut jar, attempt, logs).await
            {
                last_error = err.to_string();
                logs.push(format!(
                    "[VOD:WARN] PART {part} manifest/auth attempt {attempt} (연속 실패 상한 {}): {}",
                    req.max_retries,
                    redact(&last_error)
                ))
                .await;
                budget.failed(&output);
                if budget.consecutive_failures < req.max_retries {
                    sleep_retry(budget.consecutive_failures, cancel).await;
                }
                continue;
            }
            jar.write_file(&cookie_file)?;
            {
                let mut s = status.write().await;
                s.state = "DOWNLOADING".into();
                s.message = format!("PART {part}/{} 다운로드 중…", metadata.entries.len());
            }
            match download_part(
                &tools,
                &req,
                &cookie_file,
                &url,
                &output,
                part,
                metadata.entries.len(),
                status,
                cancel,
                logs,
            )
            .await
            {
                Ok(()) if !cancel.load(Ordering::SeqCst) => {
                    files.push(output.clone());
                    last_error.clear();
                    complete = true;
                    partial.completed = true;
                    break;
                }
                Ok(()) => {
                    cleanup_incomplete(&output);
                    return Ok(());
                }
                Err(err) => {
                    last_error = err.to_string();
                    expired_download_auth = download_authorization_expired(&last_error);
                    // A nonzero postprocessor exit may leave a corrupt final file.
                    // Keep fragment checkpoints, but never let --no-overwrites
                    // mistake that failed final output for a successful download.
                    let _ = fs::remove_file(&output);
                    logs.push(format!(
                        "[VOD:WARN] PART {part} attempt {attempt} (연속 실패 상한 {}): {}",
                        req.max_retries,
                        redact(&last_error)
                    ))
                    .await;
                    budget.failed(&output);
                    if budget.consecutive_failures < req.max_retries {
                        sleep_retry(budget.consecutive_failures, cancel).await;
                    }
                }
            }
        }
        if !complete {
            if last_error.is_empty() {
                last_error = "상세 오류를 확인하지 못했습니다.".into();
            }
            bail!("PART {part} 다운로드 실패: {last_error}");
        }
    }
    if cancel.load(Ordering::SeqCst) {
        return Ok(());
    }
    let final_file = if files.len() > 1 && req.merge {
        merge_parts(&tools, &metadata, &files, &output_dir, status, cancel, logs).await?
    } else {
        files
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("다운로드된 PART가 없습니다."))?
    };
    {
        let mut s = status.write().await;
        s.state = "COMPLETED".into();
        s.running = false;
        s.message = "VOD 다운로드가 완료되었습니다.".into();
        s.output_file = Some(final_file.display().to_string());
        s.percent = 100.0;
        s.finished_at = Some(Utc::now().to_rfc3339());
    }
    logs.push(format!("[VOD] completed file={}", final_file.display()))
        .await;
    Ok(())
}

fn download_authorization_expired(error: &str) -> bool {
    static AUTH_EXPIRED_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(
            r"(?i)(?:HTTP(?: Error| status)?[:\s]+(?:401|403)\b|\bForbidden\b|\bUnauthorized\b)",
        )
        .unwrap()
    });
    AUTH_EXPIRED_RE.is_match(error)
}

async fn init_cookie(
    backend: &Path,
    tools: &Tools,
    vod_url: &str,
    source: CookieSource<'_>,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<CookieJar> {
    match source.mode.to_ascii_uppercase().as_str() {
        "SOOP_LOGIN" => login_cookie(backend, vod_url, logs).await,
        "FILE" => CookieJar::parse_file(Path::new(source.cookie_file)),
        "BROWSER" => {
            let target = source.job_dir.join("browser-cookies.txt");
            let args = vec![
                "--cookies-from-browser".to_string(),
                source.browser.to_string(),
                "--cookies".to_string(),
                target.display().to_string(),
                "--skip-download".to_string(),
                vod_url.to_string(),
            ];
            run_capture(&tools.yt_dlp, &args, cancel, logs, "yt-dlp cookie").await?;
            CookieJar::parse_file(&target)
        }
        other => bail!("지원하지 않는 Cookie 모드입니다: {other}"),
    }
}

async fn renew_base_cookie(
    backend: &Path,
    tools: &Tools,
    vod_url: &str,
    source: CookieSource<'_>,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<CookieJar> {
    init_cookie(backend, tools, vod_url, source, cancel, logs).await
}

async fn login_cookie(_backend: &Path, vod_url: &str, logs: &LogBuffer) -> Result<CookieJar> {
    let db = store::global()?;
    let mut settings =
        read_provider_settings(&db, &["SOOP_USERNAME", "SOOP_PASSWORD"], &["SOOP_PASSWORD"])?;
    let username = settings.remove("SOOP_USERNAME").unwrap_or_default();
    let password = settings.remove("SOOP_PASSWORD").unwrap_or_default();
    if username.is_empty() || password.is_empty() {
        bail!("SOOP 아이디와 비밀번호를 먼저 설정해 주세요.");
    }
    let client = http_client()?;
    let response = client
        .post("https://login.sooplive.com/app/LoginAction.php")
        .header(REFERER, "https://www.sooplive.com/")
        .form(&[
            ("szWork", "login"),
            ("szType", "json"),
            ("szUid", username.as_str()),
            ("szPassword", password.as_str()),
            ("isSaveId", "true"),
            ("isSavePw", "false"),
            ("isSaveJoin", "false"),
            ("isLoginRetain", "Y"),
        ])
        .send()
        .await?;
    let mut jar = CookieJar::default();
    collect_set_cookies(&mut jar, &response, "login.sooplive.com");
    let value: Value = super::super::read_bounded_json(
        response.error_for_status()?,
        super::super::DEFAULT_API_BODY_LIMIT,
        "SOOP VOD login",
    )
    .await?;
    if value.get("RESULT").and_then(Value::as_i64) != Some(1) {
        bail!("SOOP 로그인 실패 RESULT={:?}", value.get("RESULT"));
    }
    let cookie = jar.header_for("https://afevent2.sooplive.com/")?;
    let verify = client
        .get("https://afevent2.sooplive.com/api/get_private_info.php")
        .header(REFERER, "https://www.sooplive.com/")
        .header(COOKIE, cookie)
        .send()
        .await?;
    collect_set_cookies(&mut jar, &verify, "afevent2.sooplive.com");
    let auth: Value = super::super::read_bounded_json(
        verify.error_for_status()?,
        super::super::DEFAULT_API_BODY_LIMIT,
        "SOOP VOD get_private_info",
    )
    .await?;
    if auth
        .pointer("/CHANNEL/LOGIN_ID")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        bail!("SOOP 로그인 검증에 실패했습니다.");
    }
    let cookie = jar.header_for(vod_url)?;
    let mut request = client
        .get(vod_url)
        .header(REFERER, "https://vod.sooplive.com/");
    if !cookie.is_empty() {
        request = request.header(COOKIE, cookie);
    }
    let visit = request.send().await?;
    collect_set_cookies(&mut jar, &visit, "vod.sooplive.com");
    visit.error_for_status()?;
    jar.remove_cloudfront();
    logs.push(format!("[VOD:AUTH] SOOP login session ready: {username}"))
        .await;
    Ok(jar)
}

async fn refresh_authorization(
    vod_url: &str,
    streamer_id: &str,
    manifest_url: &str,
    jar: &mut CookieJar,
    attempt: u32,
    logs: &LogBuffer,
) -> Result<()> {
    jar.remove_cloudfront();
    let title_no = title_no(vod_url)?;
    let client = http_client()?;
    let cookie = jar.header_for("https://live.sooplive.com/api/private_auth.php")?;
    let mut request = client
        .post("https://live.sooplive.com/api/private_auth.php")
        .header(REFERER, vod_url)
        .header("Origin", "https://vod.sooplive.com")
        .header("Accept", "application/json, text/plain, */*");
    if !cookie.is_empty() {
        request = request.header(COOKIE, cookie);
    }
    let response = request
        .form(&[
            ("type", "vod"),
            ("strm_id", streamer_id),
            ("title_no", title_no.as_str()),
            ("url", manifest_url),
        ])
        .send()
        .await?;
    collect_cloudfront_headers(jar, &response, manifest_url)?;
    let text = super::super::read_bounded_text(
        response.error_for_status()?,
        super::super::DEFAULT_API_BODY_LIMIT,
        "SOOP VOD private_auth",
    )
    .await?;
    let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let success = value.get("result").and_then(Value::as_i64) == Some(1)
        || value.get("result").and_then(Value::as_str) == Some("1");
    if !success {
        let code = value
            .pointer("/data/code")
            .map(value_as_string)
            .unwrap_or_default();
        let msg = value
            .pointer("/data/message")
            .and_then(Value::as_str)
            .unwrap_or("private_auth 실패");
        bail!("private_auth {code}: {msg}");
    }
    collect_cloudfront_json(jar, &value, manifest_url);
    jar.repair_cloudfront_scope(manifest_url)?;
    logs.push(format!(
        "[VOD:AUTH] private_auth OK attempt={attempt} host={}",
        Url::parse(manifest_url)?.host_str().unwrap_or("")
    ))
    .await;
    Ok(())
}

async fn probe_manifest(
    vod_url: &str,
    url: &str,
    jar: &CookieJar,
) -> Result<Vec<VodQualityOption>> {
    let client = http_client()?;
    let cookie = jar.header_for(url)?;
    let mut request = client
        .get(url)
        .header(REFERER, vod_url)
        .header("Origin", "https://vod.sooplive.com");
    let direct_mp4 = Url::parse(url)?
        .path()
        .to_ascii_lowercase()
        .ends_with(".mp4");
    if direct_mp4 {
        request = request.header(reqwest::header::RANGE, "bytes=0-63");
    }
    if !cookie.is_empty() {
        request = request.header(COOKIE, cookie);
    }
    let mut response = request
        .send()
        .await?
        .error_for_status()
        .map_err(|error| error.without_url())?;
    if !direct_mp4
        && response
            .content_length()
            .is_some_and(|len| len > 1024 * 1024)
    {
        bail!("VOD manifest가 최대 크기(1 MiB)를 초과했습니다.");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > 1024 * 1024 {
            bail!("VOD manifest가 최대 크기(1 MiB)를 초과했습니다.");
        }
        body.extend_from_slice(&chunk);
        if direct_mp4 && body.len() >= 12 {
            if body.get(4..8) != Some(b"ftyp") {
                bail!("영상 주소가 정상 MP4 파일을 반환하지 않았습니다.");
            }
            return Ok(vec![VodQualityOption {
                value: "best".into(),
                label: "최고 화질 (자동)".into(),
            }]);
        }
    }
    let text = String::from_utf8(body).context("VOD manifest가 올바른 UTF-8 텍스트가 아닙니다.")?;
    if !text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with("#EXTM3U")
    {
        bail!("영상 주소가 정상 HLS manifest를 반환하지 않았습니다.");
    }
    static RESOLUTION_HEIGHT_RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"RESOLUTION=\d+x(?P<h>\d+)").unwrap());
    let mut heights = RESOLUTION_HEIGHT_RE
        .captures_iter(&text)
        .filter_map(|c| c.name("h")?.as_str().parse::<u32>().ok())
        .collect::<Vec<_>>();
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();
    let mut out = vec![VodQualityOption {
        value: "best".into(),
        label: "최고 화질 (자동)".into(),
    }];
    for height in heights {
        out.push(VodQualityOption {
            value: format!("best[height<={height}]"),
            label: format!("{height}p"),
        });
    }
    Ok(out)
}

// Metadata from the authorized provider/extractor decides whether signed
// cookies are required. A public HLS response must not require private_auth.
async fn prepare_manifest(
    vod_url: &str,
    metadata: &VodMetadata,
    url: &str,
    jar: &mut CookieJar,
    attempt: u32,
    logs: &LogBuffer,
) -> Result<Vec<VodQualityOption>> {
    if jar.has_cloudfront() {
        jar.repair_cloudfront_scope(url)?;
    }
    if metadata.requires_private_auth && !jar.has_cloudfront() {
        if !jar.has_login() {
            bail!(
                "구독 VOD 인증을 갱신할 수 없습니다. 구독 계정으로 로그인하거나 새 signed Cookie 파일을 선택해 주세요."
            );
        }
        refresh_authorization(vod_url, &metadata.streamer_id, url, jar, attempt, logs).await?;
    }
    match probe_manifest(vod_url, url, jar).await {
        Ok(qualities) => Ok(qualities),
        Err(error)
            if error
                .downcast_ref::<reqwest::Error>()
                .and_then(|e| e.status())
                .is_some_and(|s| {
                    s == reqwest::StatusCode::UNAUTHORIZED || s == reqwest::StatusCode::FORBIDDEN
                }) =>
        {
            if !jar.has_login() {
                bail!(
                    "영상 접근 인증이 만료되었거나 권한이 없습니다. SOOP 로그인 또는 Cookie를 확인해 주세요."
                );
            }
            refresh_authorization(vod_url, &metadata.streamer_id, url, jar, attempt, logs).await?;
            probe_manifest(vod_url, url, jar).await
        }
        Err(error) => Err(error),
    }
}

async fn get_metadata(
    tools: &Tools,
    vod_url: &str,
    cookie_file: &Path,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<VodMetadata> {
    let args = vec![
        "--cookies".to_string(),
        cookie_file.display().to_string(),
        "--flat-playlist".into(),
        "--ignore-no-formats-error".into(),
        "--dump-single-json".into(),
        "--no-warnings".into(),
        vod_url.into(),
    ];
    let output = run_capture(&tools.yt_dlp, &args, cancel, logs, "yt-dlp metadata").await?;
    let info: Value = serde_json::from_str(&output).context("VOD JSON 파싱 실패")?;
    parse_metadata(&info)
}

pub(crate) fn valid_thumbnail_url(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some(
            "videoimg.sooplive.com"
                | "stimg.sooplive.com"
                | "videoimg.sooplive.co.kr"
                | "stimg.sooplive.co.kr"
                | "videoimg.afreecatv.com"
                | "stimg.afreecatv.com"
        )
    )
}

async fn load_metadata(
    tools: &Tools,
    vod_url: &str,
    cookie_file: &Path,
    jar: &mut CookieJar,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<VodMetadata> {
    let extracted = get_metadata(tools, vod_url, cookie_file, cancel, logs).await;
    if cancel.load(Ordering::SeqCst) {
        bail!("VOD 분석이 취소되었습니다.");
    }
    // yt-dlp can acquire fresh signed cookies while extracting a subscriber VOD.
    // Keep those cookies instead of overwriting them with the pre-extraction jar.
    *jar = CookieJar::parse_file(cookie_file)?;
    match extracted {
        Ok(metadata)
            if metadata
                .entries
                .iter()
                .all(|entry| !entry.manifest_url.is_empty() && entry.duration_seconds > 0) =>
        {
            Ok(metadata)
        }
        Ok(metadata) => {
            let response = fetch_metadata_api(vod_url, jar).await;
            if let Err(error) = &response {
                logs.push(format!(
                    "[VOD:WARN] metadata completion: {}",
                    redact(&error.to_string())
                ))
                .await;
            }
            complete_api_result(metadata, response)
        }
        Err(error) => {
            logs.push(format!(
                "[VOD:WARN] metadata fallback: {}",
                redact(&error.to_string())
            ))
            .await;
            let value = fetch_metadata_api(vod_url, jar).await?;
            parse_api_metadata(&value)
        }
    }
}

async fn fetch_metadata_api(vod_url: &str, jar: &CookieJar) -> Result<Value> {
    let client = http_client()?;
    let title = title_no(vod_url)?;
    // Use the current SOOP domain so the login cookies retain their real scope.
    let cookie = jar.header_for("https://api.m.sooplive.com/")?;
    let mut request = client
        .post("https://api.m.sooplive.com/station/video/a/view")
        .header(REFERER, vod_url);
    if !cookie.is_empty() {
        request = request.header(COOKIE, cookie);
    }
    let response = request
        .form(&[("nTitleNo", title.as_str()), ("nApiLevel", "10")])
        .send()
        .await?
        .error_for_status()?;
    super::super::read_bounded_json(
        response,
        super::super::DEFAULT_API_BODY_LIMIT,
        "SOOP VOD view API",
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn download_part(
    tools: &Tools,
    req: &VodDownloadRequest,
    cookie_file: &Path,
    url: &str,
    output: &Path,
    part: usize,
    part_count: usize,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<()> {
    let mut args = vec![
        "--cookies".into(),
        cookie_file.display().to_string(),
        "--referer".into(),
        req.vod_url.clone(),
        "--add-header".into(),
        "Origin:https://vod.sooplive.com".into(),
        "--user-agent".into(),
        USER_AGENT.into(),
        "-f".into(),
        if req.quality.is_empty() {
            "best".into()
        } else {
            req.quality.clone()
        },
        "--continue".into(),
        "--fragment-retries".into(),
        "1".into(),
        "--retries".into(),
        "1".into(),
        "--abort-on-unavailable-fragments".into(),
        "--no-overwrites".into(),
        "--merge-output-format".into(),
        "mp4".into(),
        "--newline".into(),
        "-o".into(),
        output.display().to_string().replace('%', "%%"),
    ];
    if let Some(ffmpeg) = &tools.ffmpeg {
        args.push("--ffmpeg-location".into());
        args.push(ffmpeg.display().to_string());
    }
    args.push(url.into());
    run_progress(&tools.yt_dlp, &args, part, part_count, status, cancel, logs).await?;
    if !cancel.load(Ordering::SeqCst) && !output.is_file() {
        bail!("yt-dlp 완료 후 출력 파일이 없습니다.");
    }
    Ok(())
}

async fn merge_parts(
    tools: &Tools,
    metadata: &VodMetadata,
    files: &[PathBuf],
    output_dir: &Path,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<PathBuf> {
    let ffmpeg = tools
        .ffmpeg
        .as_ref()
        .ok_or_else(|| anyhow!("ffmpeg를 찾을 수 없어 PART를 병합할 수 없습니다."))?;
    let concat = output_dir.join(format!(".soop-vod-concat-{}.txt", Uuid::new_v4().simple()));
    let lines = files
        .iter()
        .map(|p| ffconcat_line(p))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&concat, lines)?;
    let target = collision_path(
        output_dir,
        &format!("{}_{}", metadata.date, safe_name(&metadata.streamer)),
        ".mp4",
    )?;
    {
        let mut s = status.write().await;
        s.state = "MERGING".into();
        s.message = "선택한 PART를 병합 중…".into();
    }
    let args = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "warning".into(),
        "-f".into(),
        "concat".into(),
        "-safe".into(),
        "0".into(),
        "-i".into(),
        concat.display().to_string(),
        "-c".into(),
        "copy".into(),
        "-n".into(),
        target.display().to_string(),
    ];
    let result = run_capture_without_timeout(ffmpeg, &args, cancel, logs, "ffmpeg merge").await;
    let _ = fs::remove_file(&concat);
    if let Err(err) = result {
        let _ = fs::remove_file(&target);
        return Err(err);
    }
    if cancel.load(Ordering::SeqCst) {
        let _ = fs::remove_file(&target);
        bail!("cancelled");
    }
    if fs::metadata(&target).map(|m| m.len()).unwrap_or(0) < 1024 * 1024 {
        let _ = fs::remove_file(&target);
        bail!("FFmpeg 병합에 실패했습니다. 원본 PART 파일은 유지합니다.");
    }
    for file in files {
        let _ = fs::remove_file(file);
    }
    Ok(target)
}

async fn run_progress(
    program: &Path,
    args: &[String],
    part: usize,
    part_count: usize,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<()> {
    let mut command = Command::new(program);
    crate::platform_runtime::configure_utf8_cli(&mut command);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let (mut child, mut owned_tree) = spawn_owned(&mut command)
        .await
        .context("yt-dlp 실행 실패")?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("yt-dlp stdout 없음"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("yt-dlp stderr 없음"))?;
    let err_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let err_copy = err_lines.clone();
    let log_copy = logs.clone();
    let mut stderr_task = tokio::spawn(async move {
        let mut lines = crate::bounded_lines::BoundedLines::new(stderr);
        while let Ok(Some(line)) = lines.next_line_lossy().await {
            let redacted = redact(&line);
            if !redacted.trim().is_empty() {
                log_copy.push(format!("[VOD:yt-dlp] {redacted}")).await;
            }
            let mut values = err_copy.lock().await;
            values.push(redacted);
            if values.len() > 30 {
                values.remove(0);
            }
        }
    });
    let mut lines = crate::bounded_lines::BoundedLines::new(stdout);
    static PROGRESS_RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"(?P<p>\d+(?:\.\d+)?)%").unwrap());
    let mut stdout_open = true;
    loop {
        if cancel.load(Ordering::SeqCst) {
            owned_tree.terminate(&mut child).await?;
            stderr_task.abort();
            return Ok(());
        }
        tokio::select! {
            line = lines.next_line_lossy(), if stdout_open => {
                match line {
                    Ok(Some(line)) => {
                        if let Some(captures) = PROGRESS_RE.captures(&line)
                            && let Ok(percent) = captures["p"].parse::<f64>()
                        {
                            let mut s = status.write().await;
                            s.percent = percent;
                            s.current_part = part;
                            s.part_count = part_count;
                            s.message = format!("PART {part}: {percent:.1}%");
                        }
                    }
                    _ => stdout_open = false,
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
        if let Some(exit) = child.try_wait()? {
            owned_tree.terminate_now()?;
            // Include the terminal HTTP/auth failure before classifying retry.
            if tokio::time::timeout(Duration::from_secs(2), &mut stderr_task)
                .await
                .is_err()
            {
                stderr_task.abort();
            }
            if exit.success() {
                return Ok(());
            }
            let detail = err_lines
                .lock()
                .await
                .iter()
                .rev()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" | ");
            bail!("yt-dlp exit={:?}: {}", exit.code(), redact(&detail));
        }
    }
}

async fn run_capture(
    program: &Path,
    args: &[String],
    cancel: &AtomicBool,
    logs: &LogBuffer,
    label: &str,
) -> Result<String> {
    run_capture_with_timeout(program, args, cancel, logs, label, PROVIDER_CAPTURE_TIMEOUT).await
}

async fn run_capture_without_timeout(
    program: &Path,
    args: &[String],
    cancel: &AtomicBool,
    logs: &LogBuffer,
    label: &str,
) -> Result<String> {
    let result = run_media_process_with_atomic_cancel(
        MediaProcessSpec::new(program)
            .args(args.iter().cloned())
            .without_timeout()
            .capture_limit(PROVIDER_CAPTURE_LIMIT),
        cancel,
    )
    .await?;

    match result.outcome {
        MediaProcessOutcome::Success => {
            logs.push(format!("[VOD] {label} OK")).await;
            Ok(result.stdout.text)
        }
        MediaProcessOutcome::ProcessFailure => bail!(
            "{label} process failure (exit={:?}): {}",
            result.exit_code,
            redact(result.stderr.text.trim())
        ),
        MediaProcessOutcome::Timeout => {
            bail!("{label} unexpectedly timed out without a deadline")
        }
        MediaProcessOutcome::Cancelled => bail!("{label} cancelled"),
        MediaProcessOutcome::SpawnFailure => bail!(
            "{label} spawn failure: {}",
            result
                .spawn_error
                .unwrap_or_else(|| "unknown spawn failure".into())
        ),
    }
}

async fn run_capture_with_timeout(
    program: &Path,
    args: &[String],
    cancel: &AtomicBool,
    logs: &LogBuffer,
    label: &str,
    timeout: Duration,
) -> Result<String> {
    let result = run_media_process_with_atomic_cancel(
        MediaProcessSpec::new(program)
            .args(args.iter().cloned())
            .timeout(timeout)
            .capture_limit(PROVIDER_CAPTURE_LIMIT),
        cancel,
    )
    .await?;

    match result.outcome {
        MediaProcessOutcome::Success => {
            logs.push(format!("[VOD] {label} OK")).await;
            Ok(result.stdout.text)
        }
        MediaProcessOutcome::ProcessFailure => bail!(
            "{label} process failure (exit={:?}): {}",
            result.exit_code,
            redact(result.stderr.text.trim())
        ),
        MediaProcessOutcome::Timeout => {
            bail!("{label} timeout after {}ms", timeout.as_millis())
        }
        MediaProcessOutcome::Cancelled => bail!("{label} cancelled"),
        MediaProcessOutcome::SpawnFailure => bail!(
            "{label} spawn failure: {}",
            result
                .spawn_error
                .unwrap_or_else(|| "unknown spawn failure".into())
        ),
    }
}

fn resolve_tools(backend: &Path, yt_dlp: &str, ffmpeg: &str) -> Result<Tools> {
    let vod = backend.join("vod");
    let yt_dlp = resolve_executable(yt_dlp, &[vod.join("yt-dlp.exe")], &["yt-dlp.exe", "yt-dlp"])
        .ok_or_else(|| anyhow!("yt-dlp를 찾을 수 없습니다."))?;
    let ffmpeg = resolve_executable(
        ffmpeg,
        &[
            vod.join("ffmpeg.exe"),
            PathBuf::from(r"C:\Program Files\Streamlink\ffmpeg\ffmpeg.exe"),
        ],
        &["ffmpeg.exe", "ffmpeg"],
    );
    Ok(Tools { yt_dlp, ffmpeg })
}

fn resolve_executable(config: &str, locals: &[PathBuf], commands: &[&str]) -> Option<PathBuf> {
    if !config.trim().is_empty() {
        let path = PathBuf::from(config);
        if path.is_file() {
            return Some(path);
        }
    }
    for path in locals {
        if path.is_file() {
            return Some(path.clone());
        }
    }
    for command in commands {
        if let Some(path) = find_on_path(command) {
            return Some(path);
        }
    }
    None
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    })
}

fn incomplete_artifacts(target: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for suffix in [".part", ".ytdl", ".temp"] {
        let path = PathBuf::from(format!("{}{}", target.display(), suffix));
        if path.exists() {
            paths.push(path);
        }
    }
    if let (Some(parent), Some(name)) =
        (target.parent(), target.file_name().and_then(|v| v.to_str()))
        && let Ok(entries) = fs::read_dir(parent)
    {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if file_name.starts_with(&format!("{name}.part"))
                || file_name.starts_with(&format!("{name}.ytdl"))
                || file_name.starts_with(&format!("{name}.temp"))
            {
                let path = entry.path();
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
    }
    paths
}

fn cleanup_incomplete(target: &Path) {
    // On Windows, yt-dlp/ffmpeg can release the .part handle a few hundred
    // milliseconds after the parent process has exited. Retry instead of
    // silently giving up on the first sharing-violation error.
    for attempt in 0..20 {
        let paths = incomplete_artifacts(target);
        if paths.is_empty() {
            break;
        }
        for path in paths {
            let _ = fs::remove_file(path);
        }
        if incomplete_artifacts(target).is_empty() {
            break;
        }
        if attempt < 19 {
            std::thread::sleep(Duration::from_millis(150));
        }
    }
}

fn job_dir(backend: &Path) -> Result<PathBuf> {
    let path = backend
        .join(".stream-archive")
        .join("vod")
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn http_client() -> Result<Client> {
    #[cfg(test)]
    {
        Ok(Client::builder()
            .no_proxy()
            .http1_only()
            .pool_max_idle_per_host(0)
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .build()?)
    }
    #[cfg(not(test))]
    {
        static SHARED_CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();
        if let Some(client) = SHARED_CLIENT.get() {
            return Ok(client.clone());
        }
        let client = Client::builder()
            .no_proxy()
            .http1_only()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .build()?;
        let _ = SHARED_CLIENT.set(client.clone());
        Ok(client)
    }
}

async fn set_status(status: &Arc<RwLock<VodJobStatus>>, state: &str, message: &str) {
    let mut value = status.write().await;
    value.state = state.into();
    value.message = message.into();
}

async fn sleep_retry(attempt: u32, cancel: &AtomicBool) {
    let seconds = 2u64.pow((attempt.saturating_sub(1)).min(4));
    for _ in 0..seconds * 4 {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn retry_budget_requires_a_new_retained_high_water_mark() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("owned.mp4");
        let partial = output.with_extension("mp4.part");
        let mut budget = RetryBudget::default();
        fs::write(&partial, [0; 20]).unwrap();
        budget.failed(&output);
        budget.failed(&output);
        assert_eq!(budget.consecutive_failures, 2);
        fs::write(&partial, [0; 10]).unwrap();
        budget.failed(&output);
        fs::write(&partial, [0; 20]).unwrap();
        budget.failed(&output);
        assert_eq!(budget.consecutive_failures, 4);
        fs::write(&partial, [0; 21]).unwrap();
        budget.failed(&output);
        assert_eq!(budget.consecutive_failures, 1);
        fs::remove_file(partial).unwrap();
        budget.failed(&output);
        assert_eq!(budget.consecutive_failures, 2);
    }

    #[test]
    fn download_auth_error_classification_and_terminal_partial_cleanup() {
        assert!(download_authorization_expired(
            "yt-dlp exit=1: HTTP Error 403: Forbidden"
        ));
        assert!(download_authorization_expired("HTTP 401 Unauthorized"));
        assert!(!download_authorization_expired(
            "HTTP Error 500: server error"
        ));
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("owned.mp4");
        let part = output.with_extension("mp4.part");
        let checkpoint = output.with_extension("mp4.ytdl");
        fs::write(&part, b"fragments").unwrap();
        fs::write(&checkpoint, b"209").unwrap();
        {
            let _guard = PartialDownloadGuard {
                output,
                completed: false,
            };
        }
        assert!(!part.exists());
        assert!(!checkpoint.exists());
    }

    #[test]
    fn analysis_thumbnail_uses_extractor_or_api_without_changing_download_metadata() {
        let metadata = parse_metadata(&serde_json::json!({
            "title": "영상", "uploader_id": "fixture", "url": "https://cdn.example/master.m3u8", "duration": 60,
            "thumbnails": [{"url": "https://videoimg.sooplive.com/thumb.php?id=123"}]
        })).unwrap();
        assert_eq!(
            analysis_view("https://vod.sooplive.com/player/123", &metadata, vec![])
                .thumbnail_url
                .as_deref(),
            Some("https://videoimg.sooplive.com/thumb.php?id=123")
        );
        let mut api = api_fixture();
        api["data"]["thumb"] = serde_json::json!("https://stimg.sooplive.com/a.jpg");
        assert_eq!(
            parse_api_metadata(&api).unwrap().thumbnail_url.as_deref(),
            Some("https://stimg.sooplive.com/a.jpg")
        );
    }

    #[tokio::test]
    async fn restore_epoch_blocks_a_result_waiting_after_idle_is_observable() {
        let dir = tempfile::tempdir().unwrap();
        let events = crate::download_events::DownloadEvents::default();
        let mut receiver = events.subscribe();
        let manager = super::VodManager::new_with_events(
            dir.path().to_path_buf(),
            crate::backend::LogBuffer::new(),
            events.clone(),
        );
        let terminal_guard = manager.0.terminal.lock().await;
        let missing = dir.path().join("missing-media-tool").display().to_string();
        let started = manager
            .download(crate::model::VodDownloadRequest {
                vod_url: "https://vod.sooplive.com/player/123456789".into(),
                output_directory: dir.path().join("out").display().to_string(),
                parts: vec![],
                quality: "best".into(),
                merge: true,
                cookie_mode: "SOOP_LOGIN".into(),
                cookie_file: String::new(),
                browser_name: "firefox".into(),
                yt_dlp_path: missing.clone(),
                ffmpeg_path: missing,
                max_retries: 0,
            })
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if !manager.status().await.running {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        // The old job is idle, but cannot publish until terminal_guard is dropped.
        events.invalidate();
        drop(terminal_guard);
        let task = manager.0.take_task().await.unwrap();
        task.await.unwrap();
        let terminal = manager
            .terminal_status(started.job_id.as_deref().unwrap())
            .await
            .unwrap();
        assert_eq!(terminal.state, "FAILED");
        assert!(receiver.try_recv().is_err());
    }

    use super::*;

    #[test]
    fn part_selection_validates() {
        assert_eq!(resolve_parts(&[], 3).unwrap(), vec![1, 2, 3]);
        assert_eq!(resolve_parts(&[2], 3).unwrap(), vec![2]);
        assert_eq!(resolve_parts(&[3, 1, 3], 3).unwrap(), vec![1, 3]);
        assert!(resolve_parts(&[4], 3).is_err());
    }

    #[test]
    fn safe_filename_replaces_reserved_chars() {
        assert_eq!(safe_name("a:b/c"), "a_b_c");
        assert_eq!(safe_name("CON"), "_CON");
    }

    #[test]
    fn manifest_quality_parser_contract() {
        let text = "#EXT-X-STREAM-INF:RESOLUTION=1920x1080\n#EXT-X-STREAM-INF:RESOLUTION=1280x720";
        let mut heights = Regex::new(r"RESOLUTION=\d+x(?P<h>\d+)")
            .unwrap()
            .captures_iter(text)
            .filter_map(|c| c.name("h")?.as_str().parse::<u32>().ok())
            .collect::<Vec<_>>();
        heights.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(heights, vec![1080, 720]);
    }

    #[test]
    fn parses_set_cookie_parent_domain() {
        let cookie = parse_set_cookie(
            "AuthTicket=abc; Domain=sooplive.com; Path=/; Secure; HttpOnly",
            "login.sooplive.com",
        )
        .unwrap();
        assert_eq!(cookie.domain, ".sooplive.com");
        assert!(cookie.include_subdomains);
        assert!(cookie.secure);
    }

    #[test]
    fn parses_duration_strings() {
        assert_eq!(parse_duration_string("01:02:03"), Some(3723));
        assert_eq!(parse_duration_string("05:30"), Some(330));
    }

    #[test]
    fn ffconcat_escapes_single_quote() {
        let line = ffconcat_line(Path::new("C:/tmp/a'b.mp4"));
        assert_eq!(line, "file 'C:/tmp/a'\\''b.mp4'");
    }

    #[test]
    fn single_video_and_playlist_metadata_keep_part_order_and_seconds() {
        let single = serde_json::json!({
            "title": "한 PART", "uploader_id": "fixture", "upload_date": "20261003",
            "formats": [{"manifest_url": "https://cdn.example/master.m3u8"}], "duration": 60
        });
        let parsed = parse_metadata(&single).unwrap();
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].duration_seconds, 60);
        assert_eq!(parsed.date, "261003");
        assert!(!parsed.requires_private_auth);
        let selected_format = serde_json::json!({
            "uploader_id": "fixture", "manifest_url": "https://cdn.example/master.m3u8",
            "url": "https://cdn.example/720p.m3u8"
        });
        assert!(
            parse_metadata(&selected_format).unwrap().entries[0]
                .manifest_url
                .ends_with("master.m3u8")
        );
        let playlist = serde_json::json!({"uploader_id": "fixture", "entries": [
            {"url": "https://cdn.example/first.m3u8", "duration": 20},
            {"url": "https://cdn.example/second.m3u8", "duration": 30}
        ]});
        let parsed = parse_metadata(&playlist).unwrap();
        assert_eq!(parsed.entries.len(), 2);
        assert!(parsed.entries[1].manifest_url.ends_with("second.m3u8"));
        assert_eq!(parsed.entries[1].duration_seconds, 30);
    }

    #[test]
    fn extractor_subscription_marker_is_preserved() {
        let single = serde_json::json!({"uploader_id": "fixture", "formats": [{
            "url": "https://cdn.example/video.m3u8", "protocol": "soopvod",
            "_cookie_refresh_params": {"video_id": "123456789"}
        }]});
        assert!(parse_metadata(&single).unwrap().requires_private_auth);
        assert!(parse_metadata(&serde_json::json!({"entries": []})).is_err());
        assert!(parse_metadata(&serde_json::json!({"uploader_id": "fixture", "url": "https://vod.sooplive.com/player/123456789"})).is_err());
    }

    #[test]
    fn nested_master_manifest_wins_over_any_selected_rendition() {
        for collection in ["formats", "requested_formats"] {
            let mut value = serde_json::json!({"uploader_id": "fixture", "url": "https://cdn.example/720p.m3u8"});
            value[collection] = serde_json::json!([
                {"url": "https://cdn.example/1080p.m3u8"},
                {"manifest_url": "https://cdn.example/master.m3u8", "url": "https://cdn.example/540p.m3u8"}
            ]);
            let parsed = parse_metadata(&value).unwrap();
            assert_eq!(
                parsed.entries[0].manifest_url,
                "https://cdn.example/master.m3u8"
            );
        }
    }

    fn api_fixture() -> Value {
        serde_json::json!({"result": 1, "data": {
            "bj_id": "fixture", "title": "API 영상", "writer_nick": "채널",
            "write_timestamp": 1791036685, "subscribed_view": 1, "sub_upload_type": "",
            "files": [{"file": "https://cdn.example/first.m3u8", "duration": 60000},
                      {"file": "https://cdn.example/second.m3u8", "duration": 120000}]
        }})
    }

    #[test]
    fn api_fallback_builds_all_parts_with_millisecond_duration() {
        let metadata = parse_api_metadata(&api_fixture()).unwrap();
        assert_eq!(metadata.entries.len(), 2);
        assert_eq!(metadata.entries[0].duration_seconds, 60);
        assert_eq!(metadata.entries[1].duration_seconds, 120);
        assert_eq!(metadata.title, "API 영상");
        assert_eq!(metadata.date, "261003");
        let mut subscribed = api_fixture();
        subscribed["data"]["sub_upload_type"] = Value::String("all_board".into());
        assert!(
            parse_api_metadata(&subscribed)
                .unwrap()
                .requires_private_auth
        );
    }

    #[test]
    fn api_completion_fills_missing_duration_without_replacing_valid_extractor_fields() {
        let extracted = parse_metadata(&serde_json::json!({
            "title": "추출 제목", "uploader_id": "fixture", "entries": [
                {"url": "https://cdn.example/first.m3u8?fresh=1"},
                {"url": "https://cdn.example/second.m3u8?fresh=2", "duration": 121}
            ]
        }))
        .unwrap();
        assert_eq!(extracted.entries[0].duration_seconds, 0);
        let mut value = api_fixture();
        value["data"]["sub_upload_type"] = serde_json::json!("all_board");
        let completed = complete_api_metadata(extracted, parse_api_metadata(&value).unwrap());
        assert_eq!(completed.entries[0].duration_seconds, 60);
        assert_eq!(completed.entries[1].duration_seconds, 121);
        assert_eq!(
            completed.entries[0].manifest_url,
            "https://cdn.example/first.m3u8?fresh=1"
        );
        assert_eq!(completed.title, "추출 제목");
        assert!(completed.requires_private_auth);
    }

    #[test]
    fn duration_enrichment_failure_keeps_usable_urls_but_not_api_access_denials() {
        let extracted = || {
            parse_metadata(&serde_json::json!({
                "title": "fixture", "uploader_id": "fixture",
                "url": "https://cdn.example/master.m3u8", "protocol": "soopvod"
            }))
            .unwrap()
        };
        let retained =
            complete_api_result(extracted(), Err(anyhow!("temporary API failure"))).unwrap();
        assert_eq!(retained.entries[0].duration_seconds, 0);
        assert_eq!(
            retained.entries[0].manifest_url,
            "https://cdn.example/master.m3u8"
        );
        assert!(retained.requires_private_auth);
        let mut missing_url = extracted();
        missing_url.entries[0].manifest_url.clear();
        assert!(complete_api_result(missing_url, Err(anyhow!("temporary API failure"))).is_err());
        let mut denied = api_fixture();
        denied["data"]["subscribed_view"] = serde_json::json!(-6303);
        assert!(complete_api_result(extracted(), Ok(denied)).is_err());
    }

    #[tokio::test]
    async fn duration_enrichment_keeps_http_server_failures_but_rejects_http_auth_denials() {
        for (status, allowed) in [(500, true), (401, false), (403, false)] {
            let server = crate::test_support::LocalManifestServer::start_with_response(status, "");
            let response = http_client()
                .unwrap()
                .get(server.url())
                .send()
                .await
                .unwrap();
            let error = response.error_for_status().unwrap_err();
            let metadata = parse_metadata(&serde_json::json!({
                "title": "fixture", "uploader_id": "fixture",
                "url": "https://cdn.example/master.m3u8"
            }))
            .unwrap();
            assert_eq!(
                complete_api_result(metadata, Err(error.into())).is_ok(),
                allowed
            );
        }
    }

    #[test]
    fn duration_enrichment_keeps_incomplete_payloads_but_rejects_explicit_access_denials() {
        let metadata = || {
            parse_metadata(&serde_json::json!({
                "title": "fixture", "uploader_id": "fixture",
                "url": "https://cdn.example/master.m3u8", "protocol": "soopvod"
            }))
            .unwrap()
        };
        for value in [
            serde_json::json!({"result": 0}),
            serde_json::json!({"result": 0, "data": {"code": -9999}}),
            serde_json::json!({"result": 1, "data": {"code": 1}}),
            serde_json::json!({"result": 1, "data": {"files": [{}]}}),
        ] {
            let retained = complete_api_result(metadata(), Ok(value.clone())).unwrap();
            assert_eq!(retained.entries[0].duration_seconds, 0);
            assert!(retained.requires_private_auth);
            let mut missing_url = metadata();
            missing_url.entries[0].manifest_url.clear();
            assert!(complete_api_result(missing_url, Ok(value)).is_err());
        }
        for data in [
            serde_json::json!({"code": -6221}),
            serde_json::json!({"code": "-6205"}),
            serde_json::json!({"subscribed_view": "-6303"}),
            serde_json::json!({"adult_status": "notLogin", "files": []}),
            serde_json::json!({"sub_upload_type": "all_board", "files": []}),
        ] {
            assert!(
                complete_api_result(
                    metadata(),
                    Ok(serde_json::json!({
                        "result": 0, "data": data
                    }))
                )
                .is_err()
            );
        }
        let public = parse_metadata(&serde_json::json!({
            "uploader_id": "fixture", "url": "https://cdn.example/master.m3u8"
        }))
        .unwrap();
        let retained = complete_api_result(
            public,
            Ok(serde_json::json!({
                "result": 1, "data": {"sub_upload_type": "all_board", "files": [{}]}
            })),
        )
        .unwrap();
        assert!(retained.requires_private_auth);
    }

    #[test]
    fn api_fallback_never_uses_files_when_access_is_denied() {
        let mut value = api_fixture();
        value["data"]["subscribed_view"] = serde_json::json!(-6303);
        assert!(
            parse_api_metadata(&value)
                .unwrap_err()
                .to_string()
                .contains("구독")
        );
        value["data"]["subscribed_view"] = serde_json::json!(1);
        for (code, message) in [(-6221, "존재하지"), (-6205, "비공개"), (-9999, "접근 실패")]
        {
            value["data"]["code"] = serde_json::json!(code);
            assert!(
                parse_api_metadata(&value)
                    .unwrap_err()
                    .to_string()
                    .contains(message)
            );
        }
        value["data"]["code"] = Value::Null;
        value["data"]["files"] = serde_json::json!([]);
        value["data"]["adult_status"] = serde_json::json!("notLogin");
        assert!(
            parse_api_metadata(&value)
                .unwrap_err()
                .to_string()
                .contains("성인 인증")
        );
        value["data"]["adult_status"] = Value::Null;
        value["data"]["sub_upload_type"] = serde_json::json!("all_board");
        assert!(
            parse_api_metadata(&value)
                .unwrap_err()
                .to_string()
                .contains("구독")
        );
    }

    #[test]
    fn metadata_does_not_accept_player_urls_or_credentials_as_media() {
        let mut value = api_fixture();
        for url in [
            "https://vod.sooplive.com/player/123",
            "https://user:password@cdn.example/video.m3u8",
            "file:///private/media",
        ] {
            value["data"]["files"][0]["file"] = serde_json::json!(url);
            assert!(parse_api_metadata(&value).is_err());
        }
        let jar = CookieJar {
            items: vec![
                parse_set_cookie(
                    "AuthTicket=fixture; Domain=sooplive.com; Path=/; Secure",
                    "login.sooplive.com",
                )
                .unwrap(),
            ],
        };
        assert!(
            jar.header_for("https://api.m.sooplive.com/")
                .unwrap()
                .contains("AuthTicket=")
        );
        assert!(
            jar.header_for("https://api.m.sooplive.co.kr/")
                .unwrap()
                .is_empty()
        );
        assert!(jar.header_for("https://cdn.example/").unwrap().is_empty());
    }

    #[tokio::test]
    async fn public_manifest_does_not_need_private_auth_even_on_retry() {
        let manifest = crate::test_support::LocalManifestServer::start();
        let metadata = parse_metadata(&serde_json::json!({
            "uploader_id": "fixture", "url": manifest.url(), "duration": 60
        }))
        .unwrap();
        let mut jar = CookieJar::default();
        let qualities = prepare_manifest(
            "https://vod.sooplive.com/player/123456789",
            &metadata,
            manifest.url(),
            &mut jar,
            2,
            &LogBuffer::new(),
        )
        .await
        .unwrap();
        assert_eq!(qualities[1].label, "1080p");
        assert!(!jar.has_cloudfront());
    }

    #[tokio::test]
    async fn direct_mp4_still_supports_automatic_quality_without_loading_the_video() {
        let server = crate::test_support::LocalManifestServer::start_with_response(
            206,
            "\0\0\0\u{18}ftypisomfixture",
        );
        let url = server.url().replace("master.m3u8", "video.mp4");
        let qualities = probe_manifest(
            "https://vod.sooplive.com/player/123456789",
            &url,
            &CookieJar::default(),
        )
        .await
        .unwrap();
        assert_eq!(qualities.len(), 1);
        assert_eq!(qualities[0].value, "best");
    }

    #[test]
    fn manifest_fixture_waits_for_complete_request_headers() {
        use std::io::{Read, Write};
        let server = crate::test_support::LocalManifestServer::start();
        let url = Url::parse(server.url()).unwrap();
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", url.port().unwrap())).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        stream
            .write_all(b"GET /master.m3u8 HTTP/1.1\r\nHost: localhost\r\n")
            .unwrap();
        let mut response = [0u8; 13];
        let error = stream.read(&mut response).unwrap_err();
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ));
        stream.write_all(b"\r\n").unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.read_exact(&mut response).unwrap();
        assert_eq!(&response, b"HTTP/1.1 200 ");
    }

    #[tokio::test]
    async fn subscription_marker_requires_auth_before_a_publicly_reachable_manifest() {
        let manifest = crate::test_support::LocalManifestServer::start();
        let metadata = parse_metadata(&serde_json::json!({
            "uploader_id": "fixture", "url": manifest.url(), "protocol": "soopvod"
        }))
        .unwrap();
        let error = prepare_manifest(
            "https://vod.sooplive.com/player/123456789",
            &metadata,
            manifest.url(),
            &mut CookieJar::default(),
            1,
            &LogBuffer::new(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("구독 VOD 인증"));
    }

    #[tokio::test]
    async fn manifest_rejects_http_denial_html_and_oversized_bodies() {
        for (status, body, expected) in [
            (403, "denied".to_string(), "인증이 만료"),
            (500, "server failure".to_string(), "500"),
            (200, "<html>login</html>".to_string(), "정상 HLS"),
            (
                200,
                format!("#EXTM3U\n{}", "x".repeat(1024 * 1024)),
                "최대 크기",
            ),
        ] {
            let manifest =
                crate::test_support::LocalManifestServer::start_with_response(status, &body);
            let metadata = parse_metadata(&serde_json::json!({
                "uploader_id": "fixture", "url": manifest.url()
            }))
            .unwrap();
            let error = prepare_manifest(
                "https://vod.sooplive.com/player/123456789",
                &metadata,
                manifest.url(),
                &mut CookieJar::default(),
                1,
                &LogBuffer::new(),
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
        }
    }

    #[test]
    fn api_string_codes_and_expired_signed_cookies_are_not_accepted() {
        let mut value = api_fixture();
        value["data"]["subscribed_view"] = serde_json::json!("-6303");
        assert!(
            parse_api_metadata(&value)
                .unwrap_err()
                .to_string()
                .contains("구독")
        );
        value["data"]["subscribed_view"] = serde_json::json!("1");
        value["result"] = serde_json::json!("1");
        assert!(parse_api_metadata(&value).is_ok());
        let mut jar = CookieJar::default();
        for name in CF_NAMES {
            jar.items.push(NetscapeCookie {
                domain: "cdn.example".into(),
                include_subdomains: false,
                path: "/".into(),
                secure: true,
                expires: 1,
                name: (*name).into(),
                value: "fixture".into(),
            });
        }
        assert!(!jar.has_cloudfront());
        assert!(
            jar.header_for("https://cdn.example/video.m3u8")
                .unwrap()
                .is_empty()
        );
    }
}

#[cfg(test)]
mod provider_e2e {
    use super::*;
    use crate::{
        platform_runtime::test_process_running, test_support::ProviderFixture,
        tool_discovery::ToolKind,
    };

    fn request(output: &Path, yt_dlp: &Path, ffmpeg: &Path) -> VodDownloadRequest {
        VodDownloadRequest {
            vod_url: "https://vod.sooplive.com/player/123456789".into(),
            output_directory: output.display().to_string(),
            parts: vec![1],
            quality: "best[height<=1080]".into(),
            merge: true,
            cookie_mode: "FILE".into(),
            cookie_file: String::new(),
            browser_name: "firefox".into(),
            yt_dlp_path: yt_dlp.display().to_string(),
            ffmpeg_path: ffmpeg.display().to_string(),
            max_retries: 1,
        }
    }

    #[tokio::test]
    async fn single_part_public_and_subscriber_cookie_refresh_complete_full_pipeline() {
        for mode in [
            "single-video",
            "single-video-signed",
            "single-video-retry",
            "single-video-signed-retry",
        ] {
            let fixture = ProviderFixture::new();
            let manifest = crate::test_support::LocalManifestServer::start();
            let yt_dlp = fixture.tool(ToolKind::YtDlp);
            let ffmpeg = fixture.tool(ToolKind::Ffmpeg);
            fixture.set_mode(&yt_dlp, mode);
            fixture.set_manifest_url(&yt_dlp, manifest.url());
            let backend = fixture.root().join("backend");
            fs::create_dir_all(&backend).unwrap();
            let cookie = fixture.root().join("input cookies.txt");
            fs::write(&cookie, "# Netscape HTTP Cookie File\n.sooplive.com\tTRUE\t/\tTRUE\t4102444800\tfixture\tcookie\n").unwrap();
            let analyze = VodAnalyzeRequest {
                vod_url: "https://vod.sooplive.com/player/123456789".into(),
                cookie_mode: "FILE".into(),
                cookie_file: cookie.display().to_string(),
                browser_name: "firefox".into(),
                yt_dlp_path: yt_dlp.display().to_string(),
                ffmpeg_path: ffmpeg.display().to_string(),
                max_retries: 1,
            };
            let status = Arc::new(RwLock::new(VodJobStatus::default()));
            let view = run_analysis(
                &backend,
                analyze,
                &LogBuffer::new(),
                &status,
                &AtomicBool::new(false),
            )
            .await
            .unwrap();
            assert_eq!(view.part_count, 1);
            assert_eq!(view.parts[0].duration_seconds, 60);
            let output = fixture.root().join("out");
            let mut req = request(&output, &yt_dlp, &ffmpeg);
            req.cookie_file = cookie.display().to_string();
            if mode.ends_with("-retry") {
                req.max_retries = 2;
            }
            run_download(
                &backend,
                req,
                &LogBuffer::new(),
                &status,
                &AtomicBool::new(false),
            )
            .await
            .unwrap();
            let state = status.read().await;
            assert_eq!(state.state, "COMPLETED", "{mode}: {}", state.message);
            assert!(Path::new(state.output_file.as_deref().unwrap()).is_file());
            if mode.ends_with("-retry") {
                assert!(fixture.invocations().matches("--dump-single-json").count() >= 4);
                assert!(
                    fs::read(state.output_file.as_deref().unwrap())
                        .unwrap()
                        .starts_with(b"retained first fragments")
                );
            }
        }
    }

    #[tokio::test]
    async fn repeated_expiry_resets_only_on_retained_progress_and_stalls_are_bounded() {
        for (mode, expected_attempts, succeeds) in [
            ("single-video-repeated-expiry", 7, true),
            ("single-video-stalled", 5, false),
        ] {
            let fixture = ProviderFixture::new();
            let manifest = crate::test_support::LocalManifestServer::start();
            let yt_dlp = fixture.tool(ToolKind::YtDlp);
            let ffmpeg = fixture.tool(ToolKind::Ffmpeg);
            fixture.set_mode(&yt_dlp, mode);
            fixture.set_manifest_url(&yt_dlp, manifest.url());
            let backend = fixture.root().join("backend");
            fs::create_dir_all(&backend).unwrap();
            let cookie = fixture.root().join("cookies.txt");
            fs::write(&cookie, "# Netscape HTTP Cookie File\n.sooplive.com\tTRUE\t/\tTRUE\t4102444800\tfixture\tcookie\n").unwrap();
            let output = fixture.root().join("out");
            let mut req = request(&output, &yt_dlp, &ffmpeg);
            req.cookie_file = cookie.display().to_string();
            req.max_retries = if succeeds { 2 } else { 5 };
            let status = Arc::new(RwLock::new(VodJobStatus::default()));
            let result = run_download(
                &backend,
                req,
                &LogBuffer::new(),
                &status,
                &AtomicBool::new(false),
            )
            .await;
            assert_eq!(result.is_ok(), succeeds, "{mode}: {result:?}");
            let marker = fs::read_dir(&output)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| path.extension().is_some_and(|ext| ext == "attempts"))
                .unwrap();
            assert_eq!(
                fs::read_to_string(marker).unwrap(),
                expected_attempts.to_string()
            );
            if succeeds {
                let state = status.read().await;
                assert_eq!(state.state, "COMPLETED");
                assert_eq!(
                    fs::read(state.output_file.as_ref().unwrap()).unwrap(),
                    vec![b'R'; 6 * 1024]
                );
            } else {
                assert!(fs::read_dir(&output).unwrap().all(|entry| {
                    let path = entry.unwrap().path();
                    !path
                        .extension()
                        .is_some_and(|ext| ext == "part" || ext == "ytdl" || ext == "mp4")
                }));
            }
        }
    }

    #[tokio::test]
    async fn soop_vod_provider_e2e_runs_full_manager_pipeline_without_internet() {
        use crate::test_support::LocalManifestServer;

        let fixture = ProviderFixture::new();
        let manifest = LocalManifestServer::start();
        let yt_dlp = fixture.tool(ToolKind::YtDlp);
        let ffmpeg = fixture.tool(ToolKind::Ffmpeg);
        fixture.set_manifest_url(&yt_dlp, manifest.url());

        let backend = fixture.root().join("backend");
        fs::create_dir_all(&backend).unwrap();
        let output_dir = fixture.root().join("SOOP VOD 저장 한글 🎬");
        fs::create_dir_all(&output_dir).unwrap();
        let cookie = fixture.root().join("CloudFront fixture cookies.txt");
        fs::write(
            &cookie,
            concat!(
                "# Netscape HTTP Cookie File\n",
                "fixture.invalid\tTRUE\t/\tFALSE\t4102444800\tCloudFront-Key-Pair-Id\tkey\n",
                "fixture.invalid\tTRUE\t/\tFALSE\t4102444800\tCloudFront-Policy\tpolicy\n",
                "fixture.invalid\tTRUE\t/\tFALSE\t4102444800\tCloudFront-Signature\tsignature\n"
            ),
        )
        .unwrap();

        let mut req = request(&output_dir, &yt_dlp, &ffmpeg);
        req.cookie_file = cookie.display().to_string();
        let manager = VodManager::new(backend, LogBuffer::new());
        let initial = manager.download(req).await.unwrap();
        assert!(initial.job_id.is_some());

        let started = std::time::Instant::now();
        let completed = loop {
            let current = manager.status().await;
            if !current.running {
                break current;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "SOOP VOD manager fixture did not complete"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        };

        assert_eq!(completed.state, "COMPLETED");
        assert_eq!(completed.percent, 100.0);
        let output = PathBuf::from(completed.output_file.unwrap());
        assert!(output.is_file());
        assert!(output.starts_with(&output_dir));

        let invocation = fixture.invocations();
        assert!(invocation.contains("tool=YtDlp"));
        assert!(invocation.contains("--dump-single-json"));
        assert!(invocation.contains("--referer"));
        assert!(invocation.contains("Origin:https://vod.sooplive.com"));
        assert!(invocation.contains("best[height<=1080]"));
        assert!(invocation.contains("--ffmpeg-location"));
        assert!(invocation.contains(&ffmpeg.display().to_string()));
        assert!(invocation.contains("SOOP VOD 저장 한글 🎬"));
        assert!(invocation.contains(manifest.url()));
    }

    #[tokio::test]
    async fn soop_vod_merge_is_cancel_only_and_removes_partial_target_on_failure() {
        let fixture = ProviderFixture::new();
        let yt_dlp = fixture.tool(ToolKind::YtDlp);
        let ffmpeg = fixture.tool(ToolKind::Ffmpeg);
        fixture.set_mode(&ffmpeg, "run-partial-fail");

        let output_dir = fixture.root().join("merge output 한글");
        fs::create_dir_all(&output_dir).unwrap();
        let part1 = output_dir.join("part1.mp4");
        let part2 = output_dir.join("part2.mp4");
        fs::write(&part1, vec![b'A'; 2048]).unwrap();
        fs::write(&part2, vec![b'B'; 2048]).unwrap();

        let tools = Tools {
            yt_dlp,
            ffmpeg: Some(ffmpeg),
        };
        let metadata = VodMetadata {
            title: "Fixture".into(),
            thumbnail_url: None,
            streamer: "FixtureBJ".into(),
            streamer_id: "fixture".into(),
            date: "260923".into(),
            entries: Vec::new(),
            requires_private_auth: false,
        };
        let status = Arc::new(RwLock::new(VodJobStatus::default()));
        let cancel = AtomicBool::new(false);
        let logs = LogBuffer::new();

        let err = merge_parts(
            &tools,
            &metadata,
            &[part1.clone(), part2.clone()],
            &output_dir,
            &status,
            &cancel,
            &logs,
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("process failure"));
        assert!(part1.is_file());
        assert!(part2.is_file());
        assert!(!output_dir.join("260923_FixtureBJ.mp4").exists());
        assert!(
            fs::read_dir(&output_dir)
                .unwrap()
                .filter_map(|entry| entry.ok())
                .all(|entry| !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".soop-vod-concat-"))
        );
    }

    #[tokio::test]
    async fn soop_vod_provider_e2e_maps_nonzero_spawn_failure_and_timeout() {
        let fixture = ProviderFixture::new();
        let yt_dlp = fixture.tool(ToolKind::YtDlp);
        let cancel = AtomicBool::new(false);
        let logs = LogBuffer::new();

        fixture.set_mode(&yt_dlp, "run-fail");
        let err = run_capture_with_timeout(
            &yt_dlp,
            &["--dump-single-json".into()],
            &cancel,
            &logs,
            "SOOP fixture",
            Duration::from_secs(2),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("process failure"));
        assert!(err.to_string().contains("exit=Some(7)"));

        let err = run_capture_with_timeout(
            &fixture.root().join("missing-yt-dlp"),
            &[],
            &cancel,
            &logs,
            "SOOP fixture",
            Duration::from_millis(300),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("spawn failure"));

        fixture.set_mode(&yt_dlp, "run-hang");
        let err = run_capture_with_timeout(
            &yt_dlp,
            &[],
            &cancel,
            &logs,
            "SOOP fixture",
            Duration::from_millis(250),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("timeout"));
    }

    #[tokio::test]
    async fn soop_vod_provider_e2e_cancel_cleans_owned_tree_not_unrelated_process() {
        let fixture = ProviderFixture::new();
        let yt_dlp = fixture.tool(ToolKind::YtDlp);
        fixture.set_mode(&yt_dlp, "run-spawn-child");
        let cancel = AtomicBool::new(false);
        let logs = LogBuffer::new();
        let mut unrelated = fixture.spawn_unrelated();
        fixture.wait_for_unrelated().await;

        let run = run_capture_with_timeout(
            &yt_dlp,
            &[],
            &cancel,
            &logs,
            "SOOP fixture",
            Duration::from_secs(20),
        );
        let cancel_when_ready = async {
            fixture
                .wait_for_path(&fixture.child_ready_path(&yt_dlp))
                .await;
            cancel.store(true, Ordering::SeqCst);
        };
        let (result, ()) = tokio::join!(run, cancel_when_ready);
        let err = result.unwrap_err();
        assert!(err.to_string().contains("cancelled"));
        fixture.assert_child_stopped(&yt_dlp).await;

        let unrelated_pid = unrelated.id();
        assert!(
            test_process_running(unrelated_pid),
            "unrelated process must survive SOOP VOD cancellation"
        );
        let _ = unrelated.kill();
        let _ = unrelated.wait();
    }
}
