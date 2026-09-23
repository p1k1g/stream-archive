use super::auth::{ChzzkAuth, ChzzkAuthState};
use crate::platform_runtime::{configure_utf8_cli, restrict_private_dir, spawn_owned};
use crate::{
    backend::{LogBuffer, read_safe_settings, settings_path},
    media_process::{MediaProcessOutcome, MediaProcessSpec, run_media_process_with_atomic_cancel},
    model::{
        VodAnalysisView, VodAnalyzeRequest, VodDownloadRequest, VodJobStatus, VodPartInfo,
        VodQualityOption,
    },
    recorder::resolve_timestamp_rebase_ffmpeg,
    support::platform::PlatformId,
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{Datelike, Local, Utc};
use fs2::FileExt;
use regex::Regex;
use reqwest::Client;
use serde_json::Value;
use std::{
    collections::VecDeque,
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{Mutex, RwLock, mpsc},
    task::JoinHandle,
};
use url::Url;
use uuid::Uuid;

const TERMINAL_CACHE_LIMIT: usize = 32;
const COOKIE_FILE_EXPIRES_UNIX: i64 = 4_102_444_800; // 2100-01-01 UTC
const COOKIE_FILE_NAME: &str = "chzzk-cookies.txt";
const JOB_LOCK_FILE_NAME: &str = "owner.lock";
const JOB_CREATION_LOCK_FILE_NAME: &str = ".chzzk-creation.lock";
const MEDIA_FILE_NAME: &str = "media.ts";
const DESTINATION_CLAIM_SUFFIX: &str = ".stream-archive.claim";
const FINALIZING_SUFFIX: &str = ".stream-archive.finalizing";
const COPY_BUFFER_SIZE: usize = 1024 * 1024;
const CHZZK_API_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CHZZK_API_TOTAL_TIMEOUT: Duration = Duration::from_secs(30);
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
struct ChzzkTools {
    streamlink: PathBuf,
    ffmpeg: PathBuf,
}

#[derive(Debug, Clone)]
struct Metadata {
    title: String,
    streamer: String,
    streamer_id: String,
    date: String,
    duration_seconds: u64,
    qualities: Vec<VodQualityOption>,
}

struct JobRuntime {
    task: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
}

struct JobDirGuard {
    path: PathBuf,
    lock: File,
}

impl JobDirGuard {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for JobDirGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.lock);
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct DestinationClaim {
    target: PathBuf,
    lock: Option<File>,
}

impl DestinationClaim {
    fn target(&self) -> &Path {
        &self.target
    }

    fn finalizing_path(&self) -> PathBuf {
        finalizing_path(&self.target)
    }
}

impl Drop for DestinationClaim {
    fn drop(&mut self) {
        // Keep the sidecar pathname as a reusable lock anchor. Unlinking it after
        // unlock lets a contender lock the old inode while a third process creates
        // and locks a new inode at the same pathname.
        let _ = fs::remove_file(self.finalizing_path());
        if let Some(lock) = self.lock.take() {
            let _ = FileExt::unlock(&lock);
            drop(lock);
        }
    }
}

pub struct VodManager {
    backend_dir: PathBuf,
    logs: LogBuffer,
    runtime: Mutex<JobRuntime>,
    status: Arc<RwLock<VodJobStatus>>,
    terminal: Arc<Mutex<VecDeque<(String, VodJobStatus)>>>,
}

impl VodManager {
    pub fn new(backend_dir: PathBuf, logs: LogBuffer) -> Self {
        let _ = cleanup_stale_job_dirs(&backend_dir);
        Self {
            backend_dir,
            logs,
            runtime: Mutex::new(JobRuntime {
                task: None,
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            status: Arc::new(RwLock::new(VodJobStatus {
                platform: PlatformId::Chzzk,
                ..Default::default()
            })),
            terminal: Arc::new(Mutex::new(VecDeque::new())),
        }
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
            bail!("다른 CHZZK VOD 작업이 이미 실행 중입니다.");
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
                platform: PlatformId::Chzzk,
                state: "STARTING".into(),
                running: true,
                job_id: Some(job_id),
                message: "CHZZK VOD 작업 준비 중…".into(),
                started_at: Some(Utc::now().to_rfc3339()),
                ..Default::default()
            };
        }

        let task = tokio::spawn(async move {
            let result = match kind {
                VodJobKind::Analyze(req) => run_analysis(&backend, req, &logs, &status, &cancel)
                    .await
                    .map(|_| ()),
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
                    current.message = "CHZZK VOD 작업이 취소되었습니다.".into();
                    logs.push("[VOD:CHZZK] job cancelled").await;
                } else if let Err(err) = result {
                    current.state = "FAILED".into();
                    current.message = redact(&format!("{err:#}"));
                    logs.push(format!("[VOD:CHZZK:ERR] {}", current.message))
                        .await;
                }
                current.clone()
            };
            let mut terminal = terminal.lock().await;
            if terminal.len() >= TERMINAL_CACHE_LIMIT {
                terminal.pop_front();
            }
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
                current.message = "CHZZK VOD 프로세스 종료 중…".into();
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

async fn run_analysis(
    backend: &Path,
    req: VodAnalyzeRequest,
    logs: &LogBuffer,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
) -> Result<VodAnalysisView> {
    validate_url(&req.vod_url)?;
    validate_retries(req.max_retries)?;
    set_status(status, "ANALYZING", "CHZZK VOD 분석 중…").await;

    let tools = resolve_chzzk_tools(backend, &req.ffmpeg_path)?;
    let job_guard = job_dir(backend)?;
    let job_dir = job_guard.path().to_path_buf();
    let _job_guard = job_guard;
    let cookie_file = chzzk_cookie_file(&job_dir)?;
    let metadata =
        load_chzzk_metadata(&tools, &req.vod_url, cookie_file.as_deref(), cancel, logs).await?;
    let view = analysis_view(&req.vod_url, &metadata);
    {
        let mut current = status.write().await;
        current.state = "READY".into();
        current.running = false;
        current.message = "CHZZK VOD 분석 완료 · 화질을 선택하세요.".into();
        current.current_part = 0;
        current.part_count = 1;
        current.percent = 100.0;
        current.analysis = Some(view.clone());
        current.finished_at = Some(Utc::now().to_rfc3339());
    }
    logs.push(format!(
        "[VOD:CHZZK] analysis ready title={} streamer={}",
        metadata.title, metadata.streamer
    ))
    .await;
    Ok(view)
}

pub(crate) fn validate_download_request(req: &VodDownloadRequest) -> Result<()> {
    validate_url(&req.vod_url)?;
    validate_retries(req.max_retries)?;
    if req.output_directory.trim().is_empty() {
        bail!("VOD 출력 폴더가 비어 있습니다.");
    }
    if req.parts.iter().any(|part| *part != 1) {
        bail!("CHZZK VOD는 단일 영상이므로 PART 1만 선택할 수 있습니다.");
    }
    if !req.quality.trim().is_empty()
        && !Regex::new(r"^(?:best|worst|\d+p(?:\d+)?|best\[height<=\d+\])$")
            .unwrap()
            .is_match(req.quality.trim())
    {
        bail!("지원하지 않는 CHZZK VOD 화질 선택입니다.");
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
    let tools = resolve_chzzk_tools(backend, &req.ffmpeg_path)?;
    let output_dir = PathBuf::from(req.output_directory.trim());
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("VOD 출력 폴더 생성 실패: {}", output_dir.display()))?;
    let job_guard = job_dir(backend)?;
    let job_dir = job_guard.path().to_path_buf();
    let _job_guard = job_guard;
    let cookie_file = chzzk_cookie_file(&job_dir)?;

    set_status(status, "ANALYZING", "CHZZK VOD 메타데이터 확인 중…").await;
    let metadata =
        load_chzzk_metadata(&tools, &req.vod_url, cookie_file.as_deref(), cancel, logs).await?;
    run_download_prepared(
        &tools,
        &req,
        cookie_file.as_deref(),
        metadata,
        &job_dir,
        &output_dir,
        logs,
        status,
        cancel,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_download_prepared(
    tools: &ChzzkTools,
    req: &VodDownloadRequest,
    cookie_file: Option<&Path>,
    metadata: Metadata,
    job_dir: &Path,
    output_dir: &Path,
    logs: &LogBuffer,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
) -> Result<()> {
    let view = analysis_view(&req.vod_url, &metadata);
    {
        let mut current = status.write().await;
        current.state = "DOWNLOADING".into();
        current.message = "CHZZK VOD 다운로드 시작".into();
        current.current_part = 1;
        current.part_count = 1;
        current.percent = 0.0;
        current.analysis = Some(view);
    }

    let base = format!(
        "{}_{}_{}",
        metadata.date,
        safe_name(&metadata.streamer, 60),
        safe_name(&metadata.title, 100)
    );
    let mut destination = claim_collision_path(&output_dir, &base, "ts")?;
    let staging_output = job_dir.join(MEDIA_FILE_NAME);
    let mut last_error = String::new();
    let attempts = req.max_retries.max(1);
    for attempt in 1..=attempts {
        if cancel.load(Ordering::SeqCst) {
            cleanup_job_media(&job_dir);
            return Ok(());
        }
        cleanup_job_media(&job_dir);
        {
            let mut current = status.write().await;
            current.state = "DOWNLOADING".into();
            current.message = format!("CHZZK VOD 다운로드 중 ({attempt}/{attempts})");
            current.current_part = 1;
            current.part_count = 1;
        }
        match download_video(
            &tools,
            &req,
            cookie_file.as_deref(),
            &staging_output,
            metadata.duration_seconds,
            status,
            cancel,
        )
        .await
        {
            Ok(()) => {
                if cancel.load(Ordering::SeqCst) {
                    cleanup_job_media(&job_dir);
                    return Ok(());
                }
                let staged_file = find_finished_output(&staging_output)?;
                loop {
                    match finalize_output(&staged_file, &destination, cancel)? {
                        PublishOutcome::Published => break,
                        PublishOutcome::Cancelled => {
                            cleanup_job_media(&job_dir);
                            return Ok(());
                        }
                        PublishOutcome::Collision => {
                            logs.push(format!(
                                "[VOD:CHZZK] destination appeared during publish; selecting next name: {}",
                                destination.target().display()
                            ))
                            .await;
                            drop(destination);
                            destination = claim_collision_path(&output_dir, &base, "ts")?;
                        }
                    }
                }
                let final_output = destination.target().to_path_buf();
                let mut current = status.write().await;
                current.state = "COMPLETED".into();
                current.running = false;
                current.message = "CHZZK VOD 다운로드가 완료되었습니다.".into();
                current.output_file = Some(final_output.display().to_string());
                current.percent = 100.0;
                current.finished_at = Some(Utc::now().to_rfc3339());
                logs.push(format!(
                    "[VOD:CHZZK] completed file={}",
                    final_output.display()
                ))
                .await;
                return Ok(());
            }
            Err(err) => {
                last_error = redact(&format!("{err:#}"));
                cleanup_job_media(&job_dir);
                logs.push(format!(
                    "[VOD:CHZZK:WARN] download retry {attempt}/{attempts}: {last_error}"
                ))
                .await;
                if attempt < attempts {
                    sleep_retry(attempt, cancel).await;
                }
            }
        }
    }
    bail!("CHZZK VOD 다운로드 실패: {last_error}")

}

async fn load_chzzk_metadata(
    tools: &ChzzkTools,
    vod_url: &str,
    cookie_file: Option<&Path>,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<Metadata> {
    if cancel.load(Ordering::SeqCst) {
        bail!("CHZZK VOD metadata request cancelled");
    }

    let auth = ChzzkAuth::load()?;
    if auth.partial() {
        bail!("CHZZK 인증정보가 일부만 설정되어 있습니다. NID_AUT/NID_SES를 모두 저장하세요.");
    }

    let video_no = video_id(vod_url)?;
    let client = Client::builder()
        .connect_timeout(CHZZK_API_CONNECT_TIMEOUT)
        .timeout(CHZZK_API_TOTAL_TIMEOUT)
        .build()
        .context("CHZZK video detail client build failed")?;
    let mut request = client.get(format!(
        "https://api.chzzk.naver.com/service/v3/videos/{video_no}"
    ));
    if let Some(cookie) = auth.cookie_header() {
        request = request.header(reqwest::header::COOKIE, cookie);
    }
    let response = tokio::select! {
        result = request.send() => result.context("CHZZK video detail request failed")?,
        _ = wait_for_cancel(cancel) => bail!("CHZZK VOD metadata request cancelled"),
    }
    .error_for_status()
    .context("CHZZK video detail HTTP error")?;
    let value: Value = tokio::select! {
        result = response.json() => result.context("CHZZK video detail JSON parse failed")?,
        _ = wait_for_cancel(cancel) => bail!("CHZZK VOD metadata request cancelled"),
    };
    if value.get("code").and_then(Value::as_i64) != Some(200) {
        bail!("CHZZK video detail API returned a non-success response");
    }
    let content = value
        .get("content")
        .filter(|value| !value.is_null())
        .with_context(|| match auth.state() {
            ChzzkAuthState::Missing => "CHZZK VOD 정보를 가져오지 못했습니다. 로그인/연령 확인이 필요한 VOD라면 NID_AUT/NID_SES를 저장하세요.",
            ChzzkAuthState::Partial => "CHZZK 인증정보가 일부만 설정되어 있습니다. NID_AUT/NID_SES를 모두 저장하세요.",
            ChzzkAuthState::Configured => "CHZZK 인증 쿠키가 만료되었거나 이 VOD를 볼 권한이 없을 수 있습니다.",
        })?;

    let mut metadata = metadata_from_chzzk_content(content)?;
    metadata.qualities = streamlink_quality_options(tools, vod_url, cookie_file, cancel, logs)
        .await
        .with_context(|| match auth.state() {
            ChzzkAuthState::Missing => "Streamlink가 CHZZK VOD 스트림을 찾지 못했습니다. 인증이 필요한 VOD인지 확인하세요.",
            ChzzkAuthState::Partial => "CHZZK 인증정보가 일부만 설정되어 있습니다.",
            ChzzkAuthState::Configured => "Streamlink가 CHZZK VOD 스트림을 열지 못했습니다. 쿠키 만료 또는 시청 권한을 확인하세요.",
        })?;
    logs.push(format!(
        "[VOD:CHZZK] metadata via CHZZK API; streamlink qualities={}",
        metadata.qualities.len()
    ))
    .await;
    Ok(metadata)
}

async fn wait_for_cancel(cancel: &AtomicBool) {
    while !cancel.load(Ordering::SeqCst) {
        tokio::time::sleep(CANCEL_POLL_INTERVAL).await;
    }
}

async fn streamlink_quality_options(
    tools: &ChzzkTools,
    vod_url: &str,
    cookie_file: Option<&Path>,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<Vec<VodQualityOption>> {
    let mut args = vec![
        "--no-config".to_string(),
        "--json".to_string(),
        "--ffmpeg-ffmpeg".to_string(),
        tools.ffmpeg.display().to_string(),
    ];
    append_streamlink_cookie_arg(&mut args, cookie_file);
    args.push(vod_url.to_string());
    let stdout = run_capture(
        &tools.streamlink,
        &args,
        cancel,
        logs,
        "CHZZK Streamlink analyze",
    )
    .await?;
    let value: Value =
        serde_json::from_str(stdout.trim()).context("Streamlink CHZZK JSON parse failed")?;
    quality_options_from_streamlink_json(&value)
}

fn quality_options_from_streamlink_json(value: &Value) -> Result<Vec<VodQualityOption>> {
    let streams = value
        .get("streams")
        .and_then(Value::as_object)
        .context("Streamlink CHZZK JSON has no streams")?;
    let quality_re = Regex::new(r"^(\d+)p(\d+)?$").unwrap();
    let mut names = streams
        .keys()
        .filter_map(|name| {
            let captures = quality_re.captures(name)?;
            let height = captures.get(1)?.as_str().parse::<u64>().ok()?;
            let fps = captures
                .get(2)
                .and_then(|value| value.as_str().parse::<u64>().ok())
                .unwrap_or(0);
            Some((height, fps, name.clone()))
        })
        .collect::<Vec<_>>();
    names.sort_by(|left, right| right.cmp(left));

    let mut options = vec![VodQualityOption {
        value: "best".into(),
        label: "최고 화질 (자동)".into(),
    }];
    for (_, _, name) in names {
        options.push(VodQualityOption {
            value: name.clone(),
            label: name,
        });
    }
    Ok(options)
}

fn metadata_from_chzzk_content(content: &Value) -> Result<Metadata> {
    let title = content
        .get("videoTitle")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        bail!("CHZZK API did not return a VOD title");
    }
    let streamer = content
        .pointer("/channel/channelName")
        .and_then(Value::as_str)
        .unwrap_or("CHZZK")
        .trim()
        .to_string();
    let streamer_id = content
        .pointer("/channel/channelId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let duration_seconds = content
        .get("duration")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_f64().map(|n| n.max(0.0) as u64))
        })
        .unwrap_or(0);
    let date = content
        .get("publishDate")
        .and_then(Value::as_str)
        .and_then(short_date)
        .or_else(|| {
            content
                .get("publishDateAt")
                .and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|n| n as i64)))
                .and_then(chrono::DateTime::<Utc>::from_timestamp_millis)
                .map(|timestamp| {
                    let local = timestamp.with_timezone(&Local);
                    format!(
                        "{:02}{:02}{:02}",
                        local.year() % 100,
                        local.month(),
                        local.day()
                    )
                })
        })
        .unwrap_or_else(today_short_date);
    Ok(Metadata {
        title,
        streamer,
        streamer_id,
        date,
        duration_seconds,
        qualities: vec![VodQualityOption {
            value: "best".into(),
            label: "최고 화질 (자동)".into(),
        }],
    })
}

fn analysis_view(vod_url: &str, metadata: &Metadata) -> VodAnalysisView {
    VodAnalysisView {
        vod_url: vod_url.to_string(),
        title: metadata.title.clone(),
        streamer: metadata.streamer.clone(),
        streamer_id: metadata.streamer_id.clone(),
        part_count: 1,
        qualities: metadata.qualities.clone(),
        parts: vec![VodPartInfo {
            part: 1,
            duration_seconds: metadata.duration_seconds,
        }],
    }
}

async fn download_video(
    tools: &ChzzkTools,
    req: &VodDownloadRequest,
    cookie_file: Option<&Path>,
    output: &Path,
    duration_seconds: u64,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
) -> Result<()> {
    let (mut sorting_args, stream_name) = streamlink_quality_args(req.quality.trim());
    let mut args = vec![
        "--no-config".to_string(),
        "--loglevel".to_string(),
        "info".to_string(),
        "--progress".to_string(),
        "no".to_string(),
        "--stream-segment-threads".to_string(),
        "3".to_string(),
        "--ffmpeg-ffmpeg".to_string(),
        tools.ffmpeg.display().to_string(),
        "--ffmpeg-fout".to_string(),
        "mpegts".to_string(),
        "--stdout".to_string(),
    ];
    append_streamlink_cookie_arg(&mut args, cookie_file);
    args.append(&mut sorting_args);
    args.push(req.vod_url.clone());
    args.push(stream_name);

    run_streamlink_download(
        &tools.streamlink,
        &tools.ffmpeg,
        &args,
        output,
        duration_seconds,
        status,
        cancel,
    )
    .await
}

fn streamlink_quality_args(quality: &str) -> (Vec<String>, String) {
    if let Some(captures) = Regex::new(r"^best\[height<=(\d+)\]$")
        .unwrap()
        .captures(quality)
    {
        let height = captures.get(1).unwrap().as_str();
        return (
            vec!["--stream-sorting-excludes".into(), format!(">{height}p")],
            "best".into(),
        );
    }
    let stream = if quality.is_empty() { "best" } else { quality };
    (Vec::new(), stream.to_string())
}

fn ffmpeg_progress_seconds(line: &str) -> Option<f64> {
    let raw = line.trim().strip_prefix("out_time_us=")?;
    let micros = raw.parse::<u64>().ok()?;
    Some(micros as f64 / 1_000_000.0)
}

fn download_progress_percent(media_seconds: f64, duration_seconds: u64) -> f64 {
    if duration_seconds == 0 {
        return 0.0;
    }
    (media_seconds.max(0.0) / duration_seconds as f64 * 100.0).clamp(0.0, 99.0)
}

fn format_media_time(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

async fn update_download_progress(
    status: &Arc<RwLock<VodJobStatus>>,
    media_seconds: f64,
    duration_seconds: u64,
) {
    let mut current = status.write().await;
    current.percent = download_progress_percent(media_seconds, duration_seconds);
    current.current_part = 1;
    current.part_count = 1;
    current.message = if duration_seconds > 0 {
        format!(
            "CHZZK VOD 다운로드 중 · {} / {}",
            format_media_time(media_seconds),
            format_media_time(duration_seconds as f64)
        )
    } else {
        format!(
            "CHZZK VOD 다운로드 중 · {}",
            format_media_time(media_seconds)
        )
    };
}

fn spawn_line_reader<R>(reader: R, tx: mpsc::Sender<String>) -> JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send(line).await.is_err() {
                break;
            }
        }
    })
}

async fn join_reader_tasks(tasks: Vec<JoinHandle<()>>) {
    for task in tasks {
        let _ = task.await;
    }
}

async fn abort_reader_tasks(tasks: Vec<JoinHandle<()>>) {
    for task in &tasks {
        task.abort();
    }
    join_reader_tasks(tasks).await;
}

async fn drain_failed_reader_tasks(
    tasks: Vec<JoinHandle<()>>,
    log_rx: &mut mpsc::Receiver<String>,
    progress_rx: &mut mpsc::Receiver<String>,
    tail: &mut VecDeque<String>,
) {
    while !tasks.iter().all(|task| task.is_finished()) {
        tokio::select! {
            line = log_rx.recv(), if !log_rx.is_closed() => {
                if let Some(line) = line {
                    push_tail(tail, &line);
                }
            }
            _ = progress_rx.recv(), if !progress_rx.is_closed() => {}
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }
    }
    while let Ok(line) = log_rx.try_recv() {
        push_tail(tail, &line);
    }
    while progress_rx.try_recv().is_ok() {}
    join_reader_tasks(tasks).await;
}

async fn run_streamlink_download(
    streamlink: &Path,
    ffmpeg: &Path,
    args: &[String],
    output: &Path,
    duration_seconds: u64,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
) -> Result<()> {
    let mut streamlink_command = Command::new(streamlink);
    configure_utf8_cli(&mut streamlink_command);
    streamlink_command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(parent) = streamlink.parent()
        && parent.is_dir()
    {
        streamlink_command.current_dir(parent);
    }
    let (mut streamlink_child, mut streamlink_tree) = spawn_owned(&mut streamlink_command)
        .await
        .with_context(|| format!("Streamlink start failed: {}", streamlink.display()))?;
    let mut streamlink_stdout = match streamlink_child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = streamlink_tree.terminate(&mut streamlink_child).await;
            bail!("Streamlink stdout unavailable");
        }
    };

    let mut ffmpeg_command = Command::new(ffmpeg);
    ffmpeg_command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("warning")
        .arg("-fflags")
        .arg("+genpts+discardcorrupt")
        .arg("-i")
        .arg("pipe:0")
        .arg("-map")
        .arg("0:v:0?")
        .arg("-map")
        .arg("0:a:0?")
        .arg("-c")
        .arg("copy")
        .arg("-bsf:v")
        .arg("h264_mp4toannexb")
        .arg("-f")
        .arg("mpegts")
        .arg("-mpegts_flags")
        .arg("resend_headers")
        .arg("-mpegts_copyts")
        .arg("0")
        .arg("-avoid_negative_ts")
        .arg("make_zero")
        .arg("-muxpreload")
        .arg("0")
        .arg("-muxdelay")
        .arg("0")
        .arg("-avioflags")
        .arg("direct")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg("-y")
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let (mut ffmpeg_child, mut ffmpeg_tree) = match spawn_owned(&mut ffmpeg_command).await {
        Ok(owned) => owned,
        Err(err) => {
            let _ = streamlink_tree.terminate(&mut streamlink_child).await;
            return Err(err).with_context(|| format!("FFmpeg start failed: {}", ffmpeg.display()));
        }
    };
    let mut ffmpeg_stdin = match ffmpeg_child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            let _ = streamlink_tree.terminate(&mut streamlink_child).await;
            let _ = ffmpeg_tree.terminate(&mut ffmpeg_child).await;
            bail!("FFmpeg stdin unavailable");
        }
    };

    let pump = tokio::spawn(async move {
        let copied = tokio::io::copy(&mut streamlink_stdout, &mut ffmpeg_stdin).await;
        let _ = ffmpeg_stdin.shutdown().await;
        copied
    });

    // External tools can write much faster than the status loop consumes. Keep
    // the pipe readers bounded so a noisy process cannot grow server memory
    // without limit; awaiting send also provides natural backpressure.
    let (log_tx, mut log_rx) = mpsc::channel::<String>(256);
    let mut reader_tasks = Vec::with_capacity(3);
    if let Some(stderr) = streamlink_child.stderr.take() {
        reader_tasks.push(spawn_line_reader(stderr, log_tx.clone()));
    }
    if let Some(stderr) = ffmpeg_child.stderr.take() {
        reader_tasks.push(spawn_line_reader(stderr, log_tx.clone()));
    }
    drop(log_tx);

    let (progress_tx, mut progress_rx) = mpsc::channel::<String>(256);
    if let Some(stdout) = ffmpeg_child.stdout.take() {
        reader_tasks.push(spawn_line_reader(stdout, progress_tx));
    }

    let mut tail = VecDeque::with_capacity(20);
    let mut streamlink_exit = None;
    let mut ffmpeg_exit = None;

    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = streamlink_tree.terminate(&mut streamlink_child).await;
            let _ = ffmpeg_tree.terminate(&mut ffmpeg_child).await;
            pump.abort();
            let _ = pump.await;
            abort_reader_tasks(reader_tasks).await;
            let _ = fs::remove_file(output);
            return Ok(());
        }

        while let Ok(line) = log_rx.try_recv() {
            push_tail(&mut tail, &line);
        }
        while let Ok(line) = progress_rx.try_recv() {
            if let Some(media_seconds) = ffmpeg_progress_seconds(&line) {
                update_download_progress(status, media_seconds, duration_seconds).await;
            }
        }

        if streamlink_exit.is_none() {
            streamlink_exit = streamlink_child
                .try_wait()
                .context("Streamlink CHZZK 상태 확인 실패")?;
            if let Some(exit) = streamlink_exit.as_ref()
                && !exit.success()
            {
                let _ = ffmpeg_tree.terminate(&mut ffmpeg_child).await;
                pump.abort();
                let _ = pump.await;
                drain_failed_reader_tasks(reader_tasks, &mut log_rx, &mut progress_rx, &mut tail)
                    .await;
                let _ = fs::remove_file(output);
                bail!(
                    "Streamlink CHZZK 다운로드 실패 (exit={}): {}",
                    exit_code(*exit),
                    redact(&tail.into_iter().collect::<Vec<_>>().join(" | "))
                );
            }
        }

        if ffmpeg_exit.is_none() {
            ffmpeg_exit = ffmpeg_child
                .try_wait()
                .context("FFmpeg CHZZK 상태 확인 실패")?;
            if let Some(exit) = ffmpeg_exit.as_ref()
                && !exit.success()
            {
                let _ = streamlink_tree.terminate(&mut streamlink_child).await;
                pump.abort();
                let _ = pump.await;
                drain_failed_reader_tasks(reader_tasks, &mut log_rx, &mut progress_rx, &mut tail)
                    .await;
                let _ = fs::remove_file(output);
                bail!(
                    "FFmpeg CHZZK MPEG-TS 저장 실패 (exit={}): {}",
                    exit_code(*exit),
                    redact(&tail.into_iter().collect::<Vec<_>>().join(" | "))
                );
            }
        }

        if streamlink_exit.is_some() && ffmpeg_exit.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    while let Some(line) = progress_rx.recv().await {
        if let Some(media_seconds) = ffmpeg_progress_seconds(&line) {
            update_download_progress(status, media_seconds, duration_seconds).await;
        }
    }
    while let Some(line) = log_rx.recv().await {
        push_tail(&mut tail, &line);
    }

    let copied = pump
        .await
        .context("CHZZK Streamlink-to-FFmpeg pipe task join failed")?
        .context("CHZZK Streamlink-to-FFmpeg pipe failed")?;
    join_reader_tasks(reader_tasks).await;
    if copied == 0 {
        let _ = fs::remove_file(output);
        bail!("CHZZK Streamlink MPEG-TS pipe produced no media bytes");
    }

    let size = fs::metadata(output)
        .with_context(|| format!("Streamlink CHZZK output missing: {}", output.display()))?
        .len();
    if size == 0 {
        let _ = fs::remove_file(output);
        bail!("Streamlink CHZZK output is empty");
    }
    let mut current = status.write().await;
    current.percent = 99.0;
    current.current_part = 1;
    current.part_count = 1;
    current.message = "CHZZK VOD 저장 마무리 중…".into();
    Ok(())
}

async fn run_capture(
    program: &Path,
    args: &[String],
    cancel: &AtomicBool,
    logs: &LogBuffer,
    label: &str,
) -> Result<String> {
    run_capture_with_timeout(program, args, cancel, logs, label, Duration::from_secs(30)).await
}

async fn run_capture_with_timeout(
    program: &Path,
    args: &[String],
    cancel: &AtomicBool,
    _logs: &LogBuffer,
    label: &str,
    timeout: Duration,
) -> Result<String> {
    let result = run_media_process_with_atomic_cancel(
        MediaProcessSpec::new(program)
            .args(args.iter().cloned())
            .env("PYTHONUTF8", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .timeout(timeout),
        cancel,
    )
    .await?;

    match result.outcome {
        MediaProcessOutcome::Success => Ok(result.stdout.text),
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

fn chzzk_cookie_file(job_dir: &Path) -> Result<Option<PathBuf>> {
    let auth = ChzzkAuth::load()?;
    match auth.state() {
        ChzzkAuthState::Missing => Ok(None),
        ChzzkAuthState::Partial => {
            bail!("CHZZK 인증정보가 일부만 설정되어 있습니다. NID_AUT/NID_SES를 모두 저장하세요.")
        }
        ChzzkAuthState::Configured => {
            let path = job_dir.join(COOKIE_FILE_NAME);
            let mut text = String::from("# Netscape HTTP Cookie File\n");
            for cookie in auth.streamlink_cookies() {
                if cookie.domain.contains(['\r', '\n', '\t'])
                    || cookie.name.contains(['\r', '\n', '\t'])
                    || cookie.value.contains(['\r', '\n', '\t'])
                {
                    bail!("CHZZK cookie contains invalid control characters");
                }
                text.push_str(&format!(
                    "{}\tTRUE\t/\t{}\t{}\t{}\t{}\n",
                    cookie.domain,
                    if cookie.secure { "TRUE" } else { "FALSE" },
                    COOKIE_FILE_EXPIRES_UNIX,
                    cookie.name,
                    cookie.value
                ));
            }
            fs::write(&path, text)
                .with_context(|| format!("CHZZK 임시 cookie 파일 생성 실패: {}", path.display()))?;
            restrict_cookie_file(&path)?;
            Ok(Some(path))
        }
    }
}

fn append_streamlink_cookie_arg(args: &mut Vec<String>, cookie_file: Option<&Path>) {
    if let Some(path) = cookie_file {
        args.push("--http-cookies-file".to_string());
        args.push(path.display().to_string());
    }
}

fn resolve_chzzk_tools(backend: &Path, ffmpeg: &str) -> Result<ChzzkTools> {
    let settings = read_safe_settings(&settings_path(backend)).unwrap_or_default();
    let streamlink = resolve_streamlink_tool(backend, &settings)?;
    let ffmpeg = match resolve_tool(
        ffmpeg,
        &[
            backend.join("vod").join("ffmpeg.exe"),
            backend.join("ffmpeg.exe"),
        ],
        &["ffmpeg.exe", "ffmpeg"],
    ) {
        Some(path) => path,
        None => resolve_timestamp_rebase_ffmpeg(&streamlink)?,
    };
    Ok(ChzzkTools { streamlink, ffmpeg })
}

fn resolve_streamlink_tool(
    backend: &Path,
    settings: &std::collections::BTreeMap<String, String>,
) -> Result<PathBuf> {
    for key in ["STREAMLINK_PATH", "STREAMLINK_FALLBACK"] {
        if let Some(value) = settings.get(key) {
            let value = value.trim();
            if !value.is_empty() && !value.eq_ignore_ascii_case("AUTO") {
                let path = PathBuf::from(value);
                if path.is_file() {
                    return Ok(path);
                }
            }
        }
    }
    #[allow(unused_mut)]
    let mut candidates = vec![backend.join("streamlink.exe")];
    #[cfg(windows)]
    {
        candidates.push(PathBuf::from(
            r"C:\Program Files\Streamlink\bin\streamlink.exe",
        ));
        candidates.push(PathBuf::from(r"C:\Program Files\Streamlink\streamlink.exe"));
    }
    resolve_tool("", &candidates, &["streamlink.exe", "streamlink"])
        .ok_or_else(|| anyhow!(
            "Streamlink 실행 파일을 찾지 못했습니다. STREAMLINK_PATH/STREAMLINK_FALLBACK 설정을 확인하세요."
        ))
}

fn resolve_tool(configured: &str, candidates: &[PathBuf], names: &[&str]) -> Option<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() && !configured.eq_ignore_ascii_case("AUTO") {
        let path = PathBuf::from(configured);
        return path.is_file().then_some(path);
    }
    if let Some(path) = candidates.iter().find(|path| path.is_file()) {
        return Some(path.clone());
    }
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn vod_job_root(backend: &Path) -> PathBuf {
    backend.join(".stream-archive").join("vod")
}

fn legacy_vod_job_root(backend: &Path) -> PathBuf {
    backend.join(".rust-web").join("vod")
}

fn root_creation_lock(root: &Path) -> Result<File> {
    let lock_path = root.join(JOB_CREATION_LOCK_FILE_NAME);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| {
            format!(
                "CHZZK VOD creation lock \u{c5f4}\u{ae30} \u{c2e4}\u{d328}: {}",
                lock_path.display()
            )
        })?;
    lock.lock_exclusive()
        .context("CHZZK VOD creation lock \u{d68d}\u{b4dd} \u{c2e4}\u{d328}")?;
    Ok(lock)
}

fn cleanup_stale_job_dirs(backend: &Path) -> Result<()> {
    cleanup_stale_job_dirs_in_root(&vod_job_root(backend))?;
    cleanup_stale_job_dirs_in_root(&legacy_vod_job_root(backend))
}

fn cleanup_stale_job_dirs_in_root(root: &Path) -> Result<()> {
    if !root.is_dir() {
        return Ok(());
    }
    let creation_lock = root_creation_lock(root)?;
    for entry in fs::read_dir(root).with_context(|| {
        format!(
            "CHZZK VOD \u{c784}\u{c2dc} \u{d3f4}\u{b354} \u{c870}\u{d68c} \u{c2e4}\u{d328}: {}",
            root.display()
        )
    })? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("chzzk-") {
            continue;
        }
        let dir = entry.path();
        let lock_path = dir.join(JOB_LOCK_FILE_NAME);
        if !lock_path.is_file() {
            let _ = fs::remove_dir_all(&dir);
            continue;
        }
        let lock = match OpenOptions::new().read(true).write(true).open(&lock_path) {
            Ok(lock) => lock,
            Err(_) => continue,
        };
        match lock.try_lock_exclusive() {
            Ok(()) => {
                let _ = FileExt::unlock(&lock);
                drop(lock);
                let _ = fs::remove_dir_all(&dir);
            }
            Err(err) if is_lock_contention(&err) => {}
            Err(_) => {}
        }
    }
    let _ = FileExt::unlock(&creation_lock);
    Ok(())
}

fn job_dir(backend: &Path) -> Result<JobDirGuard> {
    let root = vod_job_root(backend);
    fs::create_dir_all(&root).with_context(|| {
        format!(
            "CHZZK VOD \u{c784}\u{c2dc} \u{b8e8}\u{d2b8} \u{c0dd}\u{c131} \u{c2e4}\u{d328}: {}",
            root.display()
        )
    })?;
    let creation_lock = root_creation_lock(&root)?;
    let id = Uuid::new_v4().simple().to_string();
    let preparing = root.join(format!(".chzzk-creating-{id}"));
    let dir = root.join(format!("chzzk-{id}"));
    fs::create_dir(&preparing).with_context(|| {
        format!(
            "CHZZK VOD \u{c784}\u{c2dc} \u{d3f4}\u{b354} \u{c0dd}\u{c131} \u{c2e4}\u{d328}: {}",
            preparing.display()
        )
    })?;
    if let Err(err) = restrict_private_dir(&preparing) {
        let _ = fs::remove_dir_all(&preparing);
        return Err(err);
    }
    let preparing_lock_path = preparing.join(JOB_LOCK_FILE_NAME);
    let preparing_lock = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&preparing_lock_path)
    {
        Ok(lock) => lock,
        Err(err) => {
            let _ = fs::remove_dir_all(&preparing);
            return Err(err).with_context(|| {
                format!(
                    "CHZZK VOD ownership lock \u{c0dd}\u{c131} \u{c2e4}\u{d328}: {}",
                    preparing_lock_path.display()
                )
            });
        }
    };
    if let Err(err) = restrict_cookie_file(&preparing_lock_path) {
        drop(preparing_lock);
        let _ = fs::remove_dir_all(&preparing);
        return Err(err);
    }
    drop(preparing_lock);
    if let Err(err) = fs::rename(&preparing, &dir) {
        let _ = fs::remove_dir_all(&preparing);
        return Err(err).with_context(|| {
            format!(
                "CHZZK VOD \u{c784}\u{c2dc} \u{d3f4}\u{b354} publish \u{c2e4}\u{d328}: {} -> {}",
                preparing.display(),
                dir.display()
            )
        });
    }
    let lock_path = dir.join(JOB_LOCK_FILE_NAME);
    let lock = match OpenOptions::new().read(true).write(true).open(&lock_path) {
        Ok(lock) => lock,
        Err(err) => {
            let _ = fs::remove_dir_all(&dir);
            return Err(err).with_context(|| {
                format!(
                    "CHZZK VOD ownership lock \u{c5f4}\u{ae30} \u{c2e4}\u{d328}: {}",
                    lock_path.display()
                )
            });
        }
    };
    if let Err(err) = lock.lock_exclusive() {
        drop(lock);
        let _ = fs::remove_dir_all(&dir);
        return Err(err).context("CHZZK VOD ownership lock \u{d68d}\u{b4dd} \u{c2e4}\u{d328}");
    }
    let _ = FileExt::unlock(&creation_lock);
    drop(creation_lock);
    Ok(JobDirGuard { path: dir, lock })
}

#[cfg(unix)]
fn restrict_cookie_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("CHZZK cookie 파일 권한 설정 실패: {}", path.display()))
}

#[cfg(not(unix))]
fn restrict_cookie_file(_path: &Path) -> Result<()> {
    Ok(())
}

fn claim_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_os_string();
    name.push(DESTINATION_CLAIM_SUFFIX);
    PathBuf::from(name)
}

#[cfg(windows)]
fn hide_destination_claim(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, GetFileAttributesW, INVALID_FILE_ATTRIBUTES, SetFileAttributesW,
    };

    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    wide.push(0);
    let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("destination claim 속성 조회 실패: {}", path.display()));
    }
    let ok = unsafe { SetFileAttributesW(wide.as_ptr(), attributes | FILE_ATTRIBUTE_HIDDEN) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("destination claim 숨김 처리 실패: {}", path.display()));
    }
    Ok(())
}

#[cfg(not(windows))]
fn hide_destination_claim(_path: &Path) -> Result<()> {
    Ok(())
}

fn finalizing_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_os_string();
    name.push(FINALIZING_SUFFIX);
    PathBuf::from(name)
}

fn is_lock_contention(err: &std::io::Error) -> bool {
    if err.kind() == std::io::ErrorKind::WouldBlock {
        return true;
    }
    #[cfg(windows)]
    {
        matches!(err.raw_os_error(), Some(32) | Some(33))
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn collision_candidate(dir: &Path, base: &str, extension: &str, number: usize) -> PathBuf {
    if number == 1 {
        dir.join(format!("{base}.{extension}"))
    } else {
        dir.join(format!("{base}_{number:02}.{extension}"))
    }
}

fn claim_collision_path(dir: &Path, base: &str, extension: &str) -> Result<DestinationClaim> {
    for number in 1..=9999 {
        let target = collision_candidate(dir, base, extension, number);
        if target.exists() {
            continue;
        }
        let claim_path = claim_path(&target);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&claim_path)
            .with_context(|| {
                format!(
                    "CHZZK VOD destination claim 열기 실패: {}",
                    claim_path.display()
                )
            })?;
        // The pathname remains a reusable lock anchor for race-free collision
        // ownership. On Windows make the internal sidecar hidden instead of
        // unlinking it after unlock, which would reintroduce the inode race.
        let _ = hide_destination_claim(&claim_path);
        match lock.try_lock_exclusive() {
            Ok(()) => {
                if target.exists() {
                    let _ = FileExt::unlock(&lock);
                    drop(lock);
                    continue;
                }
                let stale_finalizing = finalizing_path(&target);
                if stale_finalizing.is_file() {
                    fs::remove_file(&stale_finalizing).with_context(|| {
                        format!(
                            "CHZZK VOD stale destination 임시 파일 정리 실패: {}",
                            stale_finalizing.display()
                        )
                    })?;
                }
                return Ok(DestinationClaim {
                    target,
                    lock: Some(lock),
                });
            }
            Err(err) if is_lock_contention(&err) => continue,
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "CHZZK VOD destination claim 잠금 실패: {}",
                        claim_path.display()
                    )
                });
            }
        }
    }
    bail!("CHZZK VOD 파일명 충돌이 너무 많습니다.")
}

fn find_finished_output(expected: &Path) -> Result<PathBuf> {
    if expected.is_file() {
        return Ok(expected.to_path_buf());
    }
    let parent = expected.parent().unwrap_or_else(|| Path::new("."));
    let stem = expected
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let mut candidates = fs::read_dir(parent)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.starts_with(stem))
                && matches!(
                    path.extension()
                        .and_then(|value| value.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase()
                        .as_str(),
                    "ts" | "mp4" | "mkv" | "webm"
                )
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("CHZZK 완료 파일을 찾지 못했습니다: {}", expected.display()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishOutcome {
    Published,
    Cancelled,
    Collision,
}

fn publish_by_copy(
    source: &Path,
    destination: &DestinationClaim,
    cancel: &AtomicBool,
    link_error: &std::io::Error,
) -> Result<PublishOutcome> {
    let target = destination.target();
    if target.exists() {
        return Ok(PublishOutcome::Collision);
    }
    let temp = destination.finalizing_path();
    if temp.is_file() {
        fs::remove_file(&temp).with_context(|| {
            format!(
                "CHZZK VOD stale destination 임시 파일 정리 실패: {}",
                temp.display()
            )
        })?;
    }
    if cancel.load(Ordering::SeqCst) {
        return Ok(PublishOutcome::Cancelled);
    }

    let publish = (|| -> Result<PublishOutcome> {
        let mut input = File::open(source)
            .with_context(|| format!("CHZZK VOD staging 파일 열기 실패: {}", source.display()))?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .with_context(|| {
                format!(
                    "CHZZK VOD destination 임시 파일 생성 실패: {}",
                    temp.display()
                )
            })?;
        let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
        loop {
            if cancel.load(Ordering::SeqCst) {
                drop(output);
                let _ = fs::remove_file(&temp);
                return Ok(PublishOutcome::Cancelled);
            }
            let read = input.read(&mut buffer).with_context(|| {
                format!("CHZZK VOD staging 파일 읽기 실패: {}", source.display())
            })?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read]).with_context(|| {
                format!("CHZZK VOD destination 임시 복사 실패: {}", temp.display())
            })?;
        }
        output.flush().with_context(|| {
            format!(
                "CHZZK VOD destination 임시 파일 flush 실패: {}",
                temp.display()
            )
        })?;
        output.sync_all().with_context(|| {
            format!(
                "CHZZK VOD destination 임시 파일 sync 실패: {}",
                temp.display()
            )
        })?;
        drop(output);

        if cancel.load(Ordering::SeqCst) {
            let _ = fs::remove_file(&temp);
            return Ok(PublishOutcome::Cancelled);
        }

        match fs::hard_link(&temp, target) {
            Ok(()) => {
                let _ = fs::remove_file(&temp);
                Ok(PublishOutcome::Published)
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&temp);
                Ok(PublishOutcome::Collision)
            }
            Err(err) => Err(err).with_context(|| {
                format!(
                    "CHZZK VOD destination no-replace publish 실패: {} -> {}",
                    temp.display(),
                    target.display()
                )
            }),
        }
    })();

    match publish {
        Ok(PublishOutcome::Published) => {
            let _ = fs::remove_file(source);
            Ok(PublishOutcome::Published)
        }
        Ok(PublishOutcome::Cancelled) => {
            let _ = fs::remove_file(&temp);
            Ok(PublishOutcome::Cancelled)
        }
        Ok(PublishOutcome::Collision) => {
            let _ = fs::remove_file(&temp);
            Ok(PublishOutcome::Collision)
        }
        Err(err) => {
            let _ = fs::remove_file(&temp);
            Err(err).with_context(|| {
                format!(
                    "CHZZK VOD 최종 파일 이동 실패 (direct link: {link_error}): {} -> {}",
                    source.display(),
                    target.display()
                )
            })
        }
    }
}

fn finalize_output(
    source: &Path,
    destination: &DestinationClaim,
    cancel: &AtomicBool,
) -> Result<PublishOutcome> {
    let target = destination.target();
    if cancel.load(Ordering::SeqCst) {
        return Ok(PublishOutcome::Cancelled);
    }

    // hard_link is an atomic create-if-absent publication primitive for a complete
    // regular file. Unlike rename on Unix, it never replaces an existing target.
    match fs::hard_link(source, target) {
        Ok(()) => {
            let _ = fs::remove_file(source);
            Ok(PublishOutcome::Published)
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            Ok(PublishOutcome::Collision)
        }
        Err(link_error) => publish_by_copy(source, destination, cancel, &link_error),
    }
}

fn cleanup_job_media(job_dir: &Path) {
    if let Ok(entries) = fs::read_dir(job_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file()
                || path
                    .file_name()
                    .is_some_and(|name| name == COOKIE_FILE_NAME || name == JOB_LOCK_FILE_NAME)
            {
                continue;
            }
            let _ = fs::remove_file(path);
        }
    }
}

fn validate_url(raw: &str) -> Result<()> {
    video_id(raw).map(|_| ())
}

fn video_id(raw: &str) -> Result<String> {
    let url = Url::parse(raw.trim()).context("CHZZK VOD URL 형식이 올바르지 않습니다.")?;
    if url.scheme() != "https" && url.scheme() != "http" {
        bail!("CHZZK VOD URL은 http/https 주소여야 합니다.");
    }
    if !url
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("chzzk.naver.com"))
    {
        bail!("CHZZK VOD URL 호스트가 아닙니다.");
    }
    let parts = url
        .path_segments()
        .map(|segments| segments.filter(|part| !part.is_empty()).collect::<Vec<_>>())
        .unwrap_or_default();
    if parts.len() != 2 || parts[0] != "video" || !parts[1].chars().all(|c| c.is_ascii_digit()) {
        bail!("CHZZK VOD URL은 https://chzzk.naver.com/video/<숫자> 형식이어야 합니다.");
    }
    Ok(parts[1].to_string())
}

fn short_date(value: &str) -> Option<String> {
    let digits = value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>();
    (digits.len() >= 8).then(|| digits[2..8].to_string())
}

fn today_short_date() -> String {
    let today = Local::now().date_naive();
    format!(
        "{:02}{:02}{:02}",
        today.year() % 100,
        today.month(),
        today.day()
    )
}

fn safe_name(value: &str, max: usize) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() {
            continue;
        }
        if r#"\/:*?"<>|"#.contains(ch) {
            output.push('_');
        } else {
            output.push(ch);
        }
    }
    let mut output = output
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .trim_end_matches('.')
        .to_string();
    if output.is_empty() {
        output = "UNKNOWN".into();
    }
    if output.chars().count() > max {
        output = output
            .chars()
            .take(max)
            .collect::<String>()
            .trim()
            .trim_end_matches('.')
            .to_string();
    }
    output
}

fn validate_retries(retries: u32) -> Result<()> {
    if !(1..=20).contains(&retries) {
        bail!("VOD 재시도 횟수는 1~20 사이여야 합니다.");
    }
    Ok(())
}

async fn set_status(status: &Arc<RwLock<VodJobStatus>>, state: &str, message: &str) {
    let mut current = status.write().await;
    current.platform = PlatformId::Chzzk;
    current.state = state.into();
    current.message = message.into();
}

async fn sleep_retry(attempt: u32, cancel: &AtomicBool) {
    let total = Duration::from_secs((attempt as u64).min(5));
    let mut elapsed = Duration::ZERO;
    while elapsed < total && !cancel.load(Ordering::SeqCst) {
        let step = Duration::from_millis(200).min(total - elapsed);
        tokio::time::sleep(step).await;
        elapsed += step;
    }
}

fn push_tail(tail: &mut VecDeque<String>, line: &str) {
    if tail.len() >= 20 {
        tail.pop_front();
    }
    let clean = redact(line.trim());
    if !clean.is_empty() {
        tail.push_back(clean);
    }
}

fn redact(value: &str) -> String {
    Regex::new(r"(?i)(NID_(?:AUT|SES)=)[^;\s]+")
        .unwrap()
        .replace_all(value, "$1<redacted>")
        .into_owned()
}

fn exit_code(status: ExitStatus) -> String {
    status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chzzk_api_timeouts_are_bounded() {
        assert!(CHZZK_API_CONNECT_TIMEOUT <= CHZZK_API_TOTAL_TIMEOUT);
        assert!(CHZZK_API_TOTAL_TIMEOUT <= Duration::from_secs(30));
        assert!(CANCEL_POLL_INTERVAL <= Duration::from_millis(100));
    }

    #[tokio::test]
    async fn cancellation_waiter_observes_atomic_flag() {
        let cancel = Arc::new(AtomicBool::new(false));
        let setter = cancel.clone();
        let task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            setter.store(true, Ordering::SeqCst);
        });
        tokio::time::timeout(Duration::from_secs(1), wait_for_cancel(cancel.as_ref()))
            .await
            .expect("cancellation waiter timed out");
        task.await.unwrap();
    }

    #[test]
    fn recognizes_only_chzzk_video_urls() {
        assert_eq!(
            video_id("https://chzzk.naver.com/video/6325166").unwrap(),
            "6325166"
        );
        assert_eq!(
            video_id("https://chzzk.naver.com/video/6325166?foo=bar").unwrap(),
            "6325166"
        );
        assert!(video_id("https://chzzk.naver.com/live/abc").is_err());
        assert!(video_id("https://example.com/video/6325166").is_err());
    }

    #[test]
    fn streamlink_json_builds_quality_options() {
        let value = serde_json::json!({
            "streams": {
                "144p": {},
                "720p": {},
                "1080p": {},
                "worst": {},
                "best": {}
            }
        });
        let options = quality_options_from_streamlink_json(&value).unwrap();
        assert_eq!(
            options
                .iter()
                .map(|value| value.value.as_str())
                .collect::<Vec<_>>(),
            vec!["best", "1080p", "720p", "144p"]
        );
    }

    #[test]
    fn maps_legacy_height_quality_to_streamlink_selector() {
        let (args, stream) = streamlink_quality_args("best[height<=1080]");
        assert_eq!(stream, "best");
        assert_eq!(args, vec!["--stream-sorting-excludes", ">1080p"]);
        let (args, stream) = streamlink_quality_args("720p");
        assert!(args.is_empty());
        assert_eq!(stream, "720p");
    }

    #[test]
    fn validates_single_part_download_contract() {
        let mut req = VodDownloadRequest {
            vod_url: "https://chzzk.naver.com/video/6325166".into(),
            output_directory: "C:\\VOD".into(),
            parts: vec![1],
            quality: "best".into(),
            merge: true,
            cookie_mode: "SOOP_LOGIN".into(),
            cookie_file: String::new(),
            browser_name: "firefox".into(),
            yt_dlp_path: String::new(),
            ffmpeg_path: String::new(),
            max_retries: 5,
        };
        assert!(validate_download_request(&req).is_ok());
        req.parts = vec![2];
        assert!(validate_download_request(&req).is_err());
    }

    #[test]
    fn chzzk_api_metadata_preserves_seconds() {
        let content = serde_json::json!({
            "videoTitle": "테스트 VOD",
            "duration": 29856,
            "publishDate": "2026-09-13 10:00:00",
            "channel": {
                "channelName": "테스트 채널",
                "channelId": "0123456789abcdef0123456789abcdef"
            }
        });
        let metadata = metadata_from_chzzk_content(&content).unwrap();
        assert_eq!(metadata.title, "테스트 VOD");
        assert_eq!(metadata.streamer, "테스트 채널");
        assert_eq!(metadata.date, "260913");
        assert_eq!(metadata.duration_seconds, 29856);
        let view = analysis_view("https://chzzk.naver.com/video/15185683", &metadata);
        assert_eq!(view.part_count, 1);
        assert_eq!(view.parts[0].duration_seconds, 29856);
    }

    #[test]
    fn ffmpeg_media_progress_drives_chzzk_percent_and_time() {
        assert_eq!(
            ffmpeg_progress_seconds("out_time_us=3600000000"),
            Some(3600.0)
        );
        let percent = download_progress_percent(3600.0, 7200);
        assert!((percent - 50.0).abs() < 0.001);
        assert_eq!(format_media_time(29856.0), "08:17:36");
        assert_eq!(download_progress_percent(99999.0, 29856), 99.0);
    }

    #[test]
    fn redacts_naver_cookie_values() {
        let text = redact("NID_AUT=aaa; NID_SES=bbb other");
        assert!(!text.contains("aaa"));
        assert!(!text.contains("bbb"));
        assert!(text.contains("NID_AUT=<redacted>"));
        assert!(text.contains("NID_SES=<redacted>"));
    }

    #[test]
    fn job_dir_publishes_before_holding_inner_lock() {
        let temp = tempfile::tempdir().unwrap();
        let guard = job_dir(temp.path()).unwrap();
        let name = guard.path().file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("chzzk-"));
        assert!(guard.path().join(JOB_LOCK_FILE_NAME).is_file());
        let root = vod_job_root(temp.path());
        assert!(!fs::read_dir(&root).unwrap().any(|entry| {
            entry.ok().is_some_and(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".chzzk-creating-")
            })
        }));
    }

    #[test]
    fn stale_chzzk_job_dirs_are_scavenged_only() {
        let temp = tempfile::tempdir().unwrap();
        let root = vod_job_root(temp.path());
        fs::create_dir_all(root.join("chzzk-stale")).unwrap();
        fs::write(root.join("chzzk-stale").join("secret.txt"), "secret").unwrap();
        fs::create_dir_all(root.join("soop-keep")).unwrap();
        cleanup_stale_job_dirs(temp.path()).unwrap();
        assert!(!root.join("chzzk-stale").exists());
        assert!(root.join("soop-keep").exists());
    }

    #[test]
    fn job_root_uses_canonical_runtime_namespace() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            vod_job_root(temp.path()),
            temp.path().join(".stream-archive").join("vod")
        );
    }

    #[test]
    fn legacy_stale_chzzk_job_dirs_are_scavenged_without_reuse() {
        let temp = tempfile::tempdir().unwrap();
        let legacy_root = legacy_vod_job_root(temp.path());
        fs::create_dir_all(legacy_root.join("chzzk-stale")).unwrap();
        fs::write(legacy_root.join("chzzk-stale").join("secret.txt"), "secret").unwrap();

        cleanup_stale_job_dirs(temp.path()).unwrap();
        assert!(!legacy_root.join("chzzk-stale").exists());

        let guard = job_dir(temp.path()).unwrap();
        assert!(guard.path().starts_with(vod_job_root(temp.path())));
        assert!(!guard.path().starts_with(&legacy_root));
    }

    #[test]
    fn active_job_lock_survives_scavenging_until_release() {
        let temp = tempfile::tempdir().unwrap();
        let root = vod_job_root(temp.path());
        let active = root.join("chzzk-active");
        fs::create_dir_all(&active).unwrap();
        let lock_path = active.join(JOB_LOCK_FILE_NAME);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .unwrap();
        lock.lock_exclusive().unwrap();

        cleanup_stale_job_dirs(temp.path()).unwrap();
        assert!(active.exists());

        FileExt::unlock(&lock).unwrap();
        drop(lock);
        cleanup_stale_job_dirs(temp.path()).unwrap();
        assert!(!active.exists());
    }

    #[test]
    fn cleanup_is_scoped_to_unique_job_directory() {
        let temp = tempfile::tempdir().unwrap();
        let cookie = temp.path().join(COOKIE_FILE_NAME);
        let lock = temp.path().join(JOB_LOCK_FILE_NAME);
        let media = temp.path().join(MEDIA_FILE_NAME);
        fs::write(&cookie, "cookie").unwrap();
        fs::write(&lock, "lock").unwrap();
        fs::write(&media, "media").unwrap();
        fs::write(temp.path().join("media.ts.part"), "part").unwrap();
        cleanup_job_media(temp.path());
        assert!(cookie.exists());
        assert!(lock.exists());
        assert!(!media.exists());
        assert!(!temp.path().join("media.ts.part").exists());
    }

    #[test]
    fn concurrent_destination_claims_choose_distinct_collision_paths() {
        let temp = tempfile::tempdir().unwrap();
        let first = claim_collision_path(temp.path(), "same", "ts").unwrap();
        let second = claim_collision_path(temp.path(), "same", "ts").unwrap();
        assert_eq!(first.target().file_name().unwrap(), "same.ts");
        assert_eq!(second.target().file_name().unwrap(), "same_02.ts");
    }

    #[test]
    fn destination_claim_sidecar_is_reused_after_release() {
        let temp = tempfile::tempdir().unwrap();
        let target;
        let sidecar;
        {
            let first = claim_collision_path(temp.path(), "same", "ts").unwrap();
            target = first.target().to_path_buf();
            sidecar = claim_path(&target);
            assert!(sidecar.is_file());
        }
        assert!(sidecar.is_file());
        let second = claim_collision_path(temp.path(), "same", "ts").unwrap();
        assert_eq!(second.target(), target.as_path());
        assert!(sidecar.is_file());
    }

    #[cfg(windows)]
    #[test]
    fn destination_claim_sidecar_is_hidden_on_windows() {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_HIDDEN, GetFileAttributesW, INVALID_FILE_ATTRIBUTES,
        };

        let temp = tempfile::tempdir().unwrap();
        let destination = claim_collision_path(temp.path(), "hidden", "ts").unwrap();
        let sidecar = claim_path(destination.target());
        let mut wide = sidecar.as_os_str().encode_wide().collect::<Vec<_>>();
        wide.push(0);
        let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
        assert_ne!(attributes, INVALID_FILE_ATTRIBUTES);
        assert_ne!(attributes & FILE_ATTRIBUTE_HIDDEN, 0);
    }

    #[test]
    fn late_external_collision_is_not_clobbered_and_retargets() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.ts");
        fs::write(&source, b"complete-media").unwrap();
        let first = claim_collision_path(temp.path(), "final", "ts").unwrap();
        let first_target = first.target().to_path_buf();
        fs::write(&first_target, b"external-data").unwrap();
        let cancel = AtomicBool::new(false);

        assert_eq!(
            finalize_output(&source, &first, &cancel).unwrap(),
            PublishOutcome::Collision
        );
        assert_eq!(fs::read(&first_target).unwrap(), b"external-data");
        assert!(source.exists());
        drop(first);

        let second = claim_collision_path(temp.path(), "final", "ts").unwrap();
        assert_eq!(second.target().file_name().unwrap(), "final_02.ts");
        assert_eq!(
            finalize_output(&source, &second, &cancel).unwrap(),
            PublishOutcome::Published
        );
        assert_eq!(fs::read(second.target()).unwrap(), b"complete-media");
        assert!(!source.exists());
    }

    #[test]
    fn cancelled_copy_publish_keeps_final_unpublished() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.ts");
        fs::write(&source, vec![7_u8; COPY_BUFFER_SIZE + 32]).unwrap();
        let destination = claim_collision_path(temp.path(), "final", "ts").unwrap();
        let cancel = AtomicBool::new(true);
        let rename_error = std::io::Error::other("simulated cross-volume rename");

        assert_eq!(
            publish_by_copy(&source, &destination, &cancel, &rename_error).unwrap(),
            PublishOutcome::Cancelled
        );
        assert!(source.exists());
        assert!(!destination.target().exists());
        assert!(!destination.finalizing_path().exists());
    }

    #[test]
    fn completed_state_wins_over_late_cancellation() {
        let cancel = AtomicBool::new(true);
        assert!(!should_mark_cancelled(&cancel, "COMPLETED"));
        assert!(should_mark_cancelled(&cancel, "DOWNLOADING"));
    }

    #[test]
    fn stale_finalizing_is_reclaimed_under_destination_claim() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("final.ts");
        let stale = finalizing_path(&target);
        fs::write(&stale, b"stale-partial").unwrap();
        assert!(stale.exists());

        let destination = claim_collision_path(temp.path(), "final", "ts").unwrap();
        assert_eq!(destination.target(), target.as_path());
        assert!(!stale.exists());
    }

    #[test]
    fn atomic_copy_publish_keeps_partial_data_out_of_final_name() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.ts");
        fs::write(&source, b"complete-media").unwrap();
        let destination = claim_collision_path(temp.path(), "final", "ts").unwrap();
        let target = destination.target().to_path_buf();
        let cancel = AtomicBool::new(false);
        let rename_error = std::io::Error::other("simulated cross-volume rename");

        assert_eq!(
            publish_by_copy(&source, &destination, &cancel, &rename_error).unwrap(),
            PublishOutcome::Published
        );
        assert!(!source.exists());
        assert_eq!(fs::read(&target).unwrap(), b"complete-media");
        assert!(!destination.finalizing_path().exists());
    }

    #[test]
    fn streamlink_staging_name_is_short_and_title_independent() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join(MEDIA_FILE_NAME);
        assert_eq!(staging.file_name().unwrap(), MEDIA_FILE_NAME);
        assert!(!staging.to_string_lossy().contains("테스트 VOD"));
    }
}

#[cfg(test)]
mod provider_e2e {
    use super::*;
    use crate::{test_support::ProviderFixture, tool_discovery::ToolKind};

    fn request(output: &Path, ffmpeg: &Path) -> VodDownloadRequest {
        VodDownloadRequest {
            vod_url: "https://chzzk.naver.com/video/1234567".into(),
            output_directory: output.display().to_string(),
            parts: vec![1],
            quality: "best[height<=720]".into(),
            merge: true,
            cookie_mode: "SOOP_LOGIN".into(),
            cookie_file: String::new(),
            browser_name: "firefox".into(),
            yt_dlp_path: String::new(),
            ffmpeg_path: ffmpeg.display().to_string(),
            max_retries: 1,
        }
    }

    #[tokio::test]
    async fn chzzk_vod_provider_e2e_runs_prepared_download_to_completed_status() {
        let fixture = ProviderFixture::new();
        let streamlink = fixture.tool(ToolKind::Streamlink);
        let ffmpeg = fixture.tool(ToolKind::Ffmpeg);
        let settings = std::collections::BTreeMap::from([(
            "STREAMLINK_PATH".to_string(),
            streamlink.display().to_string(),
        )]);
        let resolved_streamlink = resolve_streamlink_tool(fixture.root(), &settings).unwrap();
        let resolved_ffmpeg = resolve_tool(
            &ffmpeg.display().to_string(),
            &[],
            &["ffmpeg.exe", "ffmpeg"],
        )
        .unwrap();
        assert_eq!(resolved_streamlink, streamlink);
        assert_eq!(resolved_ffmpeg, ffmpeg);
        let tools = ChzzkTools {
            streamlink: resolved_streamlink,
            ffmpeg: resolved_ffmpeg,
        };

        let output_dir = fixture.root().join("CHZZK VOD 저장 한글 🎬");
        let job_dir = fixture.root().join("CHZZK job 한글");
        fs::create_dir_all(&output_dir).unwrap();
        fs::create_dir_all(&job_dir).unwrap();
        let cookie = fixture.root().join("synthetic-cookie.txt");
        fs::write(
            &cookie,
            "# Netscape HTTP Cookie File\n.naver.com\tTRUE\t/\tTRUE\t4102444800\tNID_AUT\tsynthetic\n",
        )
        .unwrap();
        let req = request(&output_dir, &ffmpeg);
        let metadata = Metadata {
            title: "Fixture CHZZK VOD".into(),
            streamer: "Fixture Channel".into(),
            streamer_id: "fixture-channel".into(),
            date: "260923".into(),
            duration_seconds: 60,
            qualities: vec![VodQualityOption {
                value: "best".into(),
                label: "최고 화질 (자동)".into(),
            }],
        };
        let status = Arc::new(RwLock::new(VodJobStatus::default()));
        let cancel = AtomicBool::new(false);
        let logs = LogBuffer::new();

        run_download_prepared(
            &tools,
            &req,
            Some(&cookie),
            metadata,
            &job_dir,
            &output_dir,
            &logs,
            &status,
            &cancel,
        )
        .await
        .unwrap();

        let completed = status.read().await.clone();
        assert_eq!(completed.state, "COMPLETED");
        assert_eq!(completed.percent, 100.0);
        let output = PathBuf::from(completed.output_file.unwrap());
        assert!(output.is_file());
        assert!(output.starts_with(&output_dir));

        let invocation = fixture.invocations();
        assert!(invocation.contains("tool=Streamlink"));
        assert!(invocation.contains("tool=Ffmpeg"));
        assert!(invocation.contains("--ffmpeg-ffmpeg"));
        assert!(invocation.contains(&ffmpeg.display().to_string()));
        assert!(invocation.contains("--ffmpeg-fout"));
        assert!(invocation.contains("--http-cookies-file"));
        assert!(invocation.contains("synthetic-cookie.txt"));
        assert!(invocation.contains("--stream-sorting-excludes"));
        assert!(invocation.contains(">720p"));
        assert!(invocation.contains("CHZZK job 한글"));
        assert!(invocation.contains("PYTHONUTF8=1"));
        assert!(invocation.contains("PYTHONIOENCODING=utf-8"));
        assert!(!invocation.contains("\tsynthetic\n"));
    }

    #[tokio::test]
    async fn chzzk_vod_provider_e2e_maps_nonzero_spawn_failure_and_timeout() {
        let fixture = ProviderFixture::new();
        let streamlink = fixture.tool(ToolKind::Streamlink);
        let ffmpeg = fixture.tool(ToolKind::Ffmpeg);
        let output = fixture.root().join("failure.ts");
        let status = Arc::new(RwLock::new(VodJobStatus::default()));
        let cancel = AtomicBool::new(false);
        let req = request(fixture.root(), &ffmpeg);

        fixture.set_mode(&streamlink, "run-fail");
        let err = download_video(
            &ChzzkTools {
                streamlink: streamlink.clone(),
                ffmpeg: ffmpeg.clone(),
            },
            &req,
            None,
            &output,
            60,
            &status,
            &cancel,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("Streamlink CHZZK 다운로드 실패"));
        assert!(err.to_string().contains("7"));

        let err = download_video(
            &ChzzkTools {
                streamlink: fixture.root().join("missing-streamlink"),
                ffmpeg: ffmpeg.clone(),
            },
            &req,
            None,
            &output,
            60,
            &status,
            &cancel,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("Streamlink start failed"));

        fixture.set_mode(&streamlink, "run-hang");
        let logs = LogBuffer::new();
        let err = run_capture_with_timeout(
            &streamlink,
            &[],
            &cancel,
            &logs,
            "CHZZK fixture",
            Duration::from_millis(250),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("timeout"));
    }

    #[tokio::test]
    async fn chzzk_vod_provider_e2e_cancel_cleans_streamlink_descendant() {
        let fixture = ProviderFixture::new();
        let streamlink = fixture.tool(ToolKind::Streamlink);
        let ffmpeg = fixture.tool(ToolKind::Ffmpeg);
        fixture.set_mode(&streamlink, "run-spawn-child");
        let tools = ChzzkTools {
            streamlink: streamlink.clone(),
            ffmpeg: ffmpeg.clone(),
        };
        let req = request(fixture.root(), &ffmpeg);
        let output = fixture.root().join("cancelled.ts");
        let status = Arc::new(RwLock::new(VodJobStatus::default()));
        let cancel = AtomicBool::new(false);

        let run = download_video(&tools, &req, None, &output, 60, &status, &cancel);
        let cancel_when_ready = async {
            fixture
                .wait_for_path(&fixture.child_ready_path(&streamlink))
                .await;
            cancel.store(true, Ordering::SeqCst);
        };
        let (result, ()) = tokio::join!(run, cancel_when_ready);
        result.unwrap();
        fixture.assert_child_stopped(&streamlink).await;
        assert!(!output.exists());
    }
}
