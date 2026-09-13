use super::auth::{ChzzkAuth, ChzzkAuthState};
use crate::{
    backend::LogBuffer,
    model::{
        VodAnalysisView, VodAnalyzeRequest, VodDownloadRequest, VodJobStatus, VodPartInfo,
        VodQualityOption,
    },
    support::platform::PlatformId,
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{Datelike, Local, Utc};
use fs2::FileExt;
use regex::Regex;
use serde_json::Value;
use std::{
    collections::{BTreeSet, VecDeque},
    env,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::{Command as StdCommand, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
    sync::{Mutex, RwLock, mpsc},
    task::JoinHandle,
};
use url::Url;
use uuid::Uuid;

const TERMINAL_CACHE_LIMIT: usize = 32;
const COOKIE_FILE_EXPIRES_UNIX: i64 = 4_102_444_800; // 2100-01-01 UTC
const PROGRESS_PREFIX: &str = "__CHZZK_PROGRESS__";
const COOKIE_FILE_NAME: &str = "chzzk-cookies.txt";
const JOB_LOCK_FILE_NAME: &str = "owner.lock";
const MEDIA_FILE_NAME: &str = "media.mp4";

#[derive(Debug, Clone)]
struct Tools {
    yt_dlp: PathBuf,
    ffmpeg: Option<PathBuf>,
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
                if cancel.load(Ordering::SeqCst) {
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

    let tools = resolve_tools(backend, &req.yt_dlp_path, &req.ffmpeg_path)?;
    let job_guard = job_dir(backend)?;
    let job_dir = job_guard.path().to_path_buf();
    let _job_guard = job_guard;
    let cookie_file = chzzk_cookie_file(&job_dir)?;
    let metadata =
        load_metadata(&tools, &req.vod_url, cookie_file.as_deref(), cancel, logs).await?;
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
        && !Regex::new(r"^best(?:\[height<=\d+\])?$")
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
    let tools = resolve_tools(backend, &req.yt_dlp_path, &req.ffmpeg_path)?;
    let output_dir = PathBuf::from(req.output_directory.trim());
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("VOD 출력 폴더 생성 실패: {}", output_dir.display()))?;
    let job_guard = job_dir(backend)?;
    let job_dir = job_guard.path().to_path_buf();
    let _job_guard = job_guard;
    let cookie_file = chzzk_cookie_file(&job_dir)?;

    set_status(status, "ANALYZING", "CHZZK VOD 메타데이터 확인 중…").await;
    let metadata =
        load_metadata(&tools, &req.vod_url, cookie_file.as_deref(), cancel, logs).await?;
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
    let final_output = collision_path(&output_dir, &base, "mp4")?;
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
            status,
            cancel,
            logs,
        )
        .await
        {
            Ok(()) if !cancel.load(Ordering::SeqCst) => {
                let staged_file = find_finished_output(&staging_output)?;
                finalize_output(&staged_file, &final_output)?;
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
            Ok(()) => {
                cleanup_job_media(&job_dir);
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

async fn load_metadata(
    tools: &Tools,
    vod_url: &str,
    cookie_file: Option<&Path>,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<Metadata> {
    let mut args = vec![
        "--dump-single-json".to_string(),
        "--skip-download".to_string(),
        "--no-playlist".to_string(),
        "--no-warnings".to_string(),
    ];
    append_cookie_arg(&mut args, cookie_file);
    args.push(vod_url.to_string());

    let output = run_capture(&tools.yt_dlp, &args, cancel, logs, "CHZZK metadata").await;
    let stdout = match output {
        Ok(stdout) => stdout,
        Err(err) => {
            let state = ChzzkAuth::load()
                .map(|auth| auth.state())
                .unwrap_or(ChzzkAuthState::Missing);
            return match state {
                ChzzkAuthState::Missing => Err(err).context(
                    "연령 제한/로그인 전용 VOD라면 설정 > CHZZK 인증에서 NID_AUT/NID_SES를 저장하세요.",
                ),
                ChzzkAuthState::Partial => Err(err).context(
                    "CHZZK 인증정보가 일부만 설정되어 있습니다. NID_AUT/NID_SES를 모두 저장하세요.",
                ),
                ChzzkAuthState::Configured => Err(err).context(
                    "CHZZK 인증 쿠키가 만료되었거나 이 VOD를 볼 권한이 없을 수 있습니다.",
                ),
            };
        }
    };
    let value: Value =
        serde_json::from_str(stdout.trim()).context("yt-dlp CHZZK 메타데이터 JSON 해석 실패")?;
    metadata_from_json(&value)
}

fn metadata_from_json(value: &Value) -> Result<Metadata> {
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        bail!("CHZZK VOD 제목을 찾지 못했습니다.");
    }
    let streamer = value
        .get("channel")
        .or_else(|| value.get("uploader"))
        .and_then(Value::as_str)
        .unwrap_or("CHZZK")
        .trim()
        .to_string();
    let streamer_id = value
        .get("channel_id")
        .or_else(|| value.get("uploader_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let duration_seconds = value
        .get("duration")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .max(0.0) as u64;
    let date = value
        .get("upload_date")
        .and_then(Value::as_str)
        .and_then(short_date)
        .unwrap_or_else(today_short_date);
    let qualities = quality_options(value);
    Ok(Metadata {
        title,
        streamer,
        streamer_id,
        date,
        duration_seconds,
        qualities,
    })
}

fn quality_options(value: &Value) -> Vec<VodQualityOption> {
    let mut heights = BTreeSet::new();
    if let Some(formats) = value.get("formats").and_then(Value::as_array) {
        for format in formats {
            let vcodec = format.get("vcodec").and_then(Value::as_str).unwrap_or("");
            if vcodec.eq_ignore_ascii_case("none") {
                continue;
            }
            if let Some(height) = format.get("height").and_then(Value::as_u64) {
                if height > 0 {
                    heights.insert(height);
                }
            }
        }
    }
    let mut options = vec![VodQualityOption {
        value: "best".into(),
        label: "최고 화질 (자동)".into(),
    }];
    for height in heights.into_iter().rev().take(8) {
        options.push(VodQualityOption {
            value: format!("best[height<={height}]"),
            label: format!("{height}p 이하"),
        });
    }
    options
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
    tools: &Tools,
    req: &VodDownloadRequest,
    cookie_file: Option<&Path>,
    output: &Path,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<()> {
    let selector = format_selector(req.quality.trim());
    let mut args = vec![
        "--no-playlist".to_string(),
        "--newline".to_string(),
        "--progress".to_string(),
        "--progress-template".to_string(),
        format!("download:{PROGRESS_PREFIX}%(progress._percent_str)s"),
        "-f".to_string(),
        selector,
        "--merge-output-format".to_string(),
        "mp4".to_string(),
        "-o".to_string(),
        output.display().to_string(),
    ];
    append_cookie_arg(&mut args, cookie_file);
    if let Some(ffmpeg) = tools.ffmpeg.as_ref() {
        args.push("--ffmpeg-location".to_string());
        args.push(ffmpeg.display().to_string());
    }
    args.push(req.vod_url.clone());
    run_download_process(&tools.yt_dlp, &args, status, cancel, logs).await
}

fn format_selector(quality: &str) -> String {
    if let Some(captures) = Regex::new(r"^best\[height<=(\d+)\]$")
        .unwrap()
        .captures(quality)
    {
        let height = captures.get(1).unwrap().as_str();
        return format!("bestvideo*[height<={height}]+bestaudio/best[height<={height}]");
    }
    "bestvideo*+bestaudio/best".into()
}

fn configure_ytdlp_command(command: &mut Command) {
    command.env("PYTHONUTF8", "1");
    command.env("PYTHONIOENCODING", "utf-8");
}

async fn run_download_process(
    program: &Path,
    args: &[String],
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
    _logs: &LogBuffer,
) -> Result<()> {
    let mut command = Command::new(program);
    configure_ytdlp_command(&mut command);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .with_context(|| format!("yt-dlp 실행 실패: {}", program.display()))?;

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    if let Some(stdout) = child.stdout.take() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx.send(line);
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx.send(line);
            }
        });
    }
    drop(tx);

    let mut tail = VecDeque::with_capacity(20);
    let exit_status = loop {
        if cancel.load(Ordering::SeqCst) {
            terminate_owned(&mut child).await;
            return Ok(());
        }
        while let Ok(line) = rx.try_recv() {
            handle_download_line(&line, status).await;
            push_tail(&mut tail, &line);
        }
        if let Some(exit) = child.try_wait().context("yt-dlp 상태 확인 실패")? {
            break exit;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    };
    while let Ok(line) = rx.try_recv() {
        handle_download_line(&line, status).await;
        push_tail(&mut tail, &line);
    }
    if !exit_status.success() {
        bail!(
            "yt-dlp CHZZK 다운로드 실패 (exit={}): {}",
            exit_code(exit_status),
            redact(&tail.into_iter().collect::<Vec<_>>().join(" | "))
        );
    }
    Ok(())
}

async fn handle_download_line(line: &str, status: &Arc<RwLock<VodJobStatus>>) {
    let Some(pos) = line.find(PROGRESS_PREFIX) else {
        return;
    };
    let raw = line[pos + PROGRESS_PREFIX.len()..].trim();
    let number = raw.trim_end_matches('%').trim().parse::<f64>().ok();
    if let Some(percent) = number {
        let mut current = status.write().await;
        current.percent = percent.clamp(0.0, 100.0);
        current.current_part = 1;
        current.part_count = 1;
    }
}

async fn run_capture(
    program: &Path,
    args: &[String],
    cancel: &AtomicBool,
    _logs: &LogBuffer,
    label: &str,
) -> Result<String> {
    let mut command = Command::new(program);
    configure_ytdlp_command(&mut command);
    let mut child = command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("{label} 프로세스 실행 실패: {}", program.display()))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("stdout unavailable"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("stderr unavailable"))?;
    let stdout_task = tokio::spawn(async move {
        let mut data = Vec::new();
        stdout.read_to_end(&mut data).await.map(|_| data)
    });
    let stderr_task = tokio::spawn(async move {
        let mut data = Vec::new();
        stderr.read_to_end(&mut data).await.map(|_| data)
    });

    let exit = loop {
        if cancel.load(Ordering::SeqCst) {
            terminate_owned(&mut child).await;
            bail!("{label} 취소됨");
        }
        if let Some(exit) = child
            .try_wait()
            .with_context(|| format!("{label} 상태 확인 실패"))?
        {
            break exit;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let stdout = stdout_task.await.context("stdout task join failed")??;
    let stderr = stderr_task.await.context("stderr task join failed")??;
    if !exit.success() {
        bail!(
            "{label} 실패 (exit={}): {}",
            exit_code(exit),
            redact(&String::from_utf8_lossy(&stderr))
        );
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

async fn terminate_owned(child: &mut Child) {
    #[cfg(windows)]
    {
        if let Some(pid) = child.id() {
            let _ = Command::new("taskkill.exe")
                .arg("/PID")
                .arg(pid.to_string())
                .arg("/T")
                .arg("/F")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        let _ = child.wait().await;
    }
    #[cfg(not(windows))]
    {
        let _ = child.kill().await;
        let _ = child.wait().await;
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

fn append_cookie_arg(args: &mut Vec<String>, cookie_file: Option<&Path>) {
    if let Some(path) = cookie_file {
        args.push("--cookies".to_string());
        args.push(path.display().to_string());
    }
}

fn resolve_tools(backend: &Path, yt_dlp: &str, ffmpeg: &str) -> Result<Tools> {
    let yt_dlp = resolve_tool(
        yt_dlp,
        &[
            backend.join("vod").join("yt-dlp.exe"),
            backend.join("yt-dlp.exe"),
        ],
        &["yt-dlp.exe", "yt-dlp"],
    )
    .ok_or_else(|| {
        anyhow!(
            "yt-dlp 실행 파일을 찾지 못했습니다. 설정 > 외부 프로그램에서 YT_DLP_PATH를 확인하세요."
        )
    })?;
    let ffmpeg = resolve_tool(
        ffmpeg,
        &[
            backend.join("vod").join("ffmpeg.exe"),
            backend.join("ffmpeg.exe"),
        ],
        &["ffmpeg.exe", "ffmpeg"],
    );
    Ok(Tools { yt_dlp, ffmpeg })
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
    backend.join(".rust-web").join("vod")
}

fn cleanup_stale_job_dirs(backend: &Path) -> Result<()> {
    let root = vod_job_root(backend);
    if !root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&root)
        .with_context(|| format!("CHZZK VOD 임시 폴더 조회 실패: {}", root.display()))?
    {
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
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => {}
        }
    }
    Ok(())
}

fn job_dir(backend: &Path) -> Result<JobDirGuard> {
    let root = vod_job_root(backend);
    fs::create_dir_all(&root)
        .with_context(|| format!("CHZZK VOD 임시 루트 생성 실패: {}", root.display()))?;
    let id = Uuid::new_v4().simple().to_string();
    let preparing = root.join(format!(".chzzk-creating-{id}"));
    let dir = root.join(format!("chzzk-{id}"));
    fs::create_dir(&preparing)
        .with_context(|| format!("CHZZK VOD 임시 폴더 생성 실패: {}", preparing.display()))?;
    if let Err(err) = restrict_job_dir(&preparing) {
        let _ = fs::remove_dir_all(&preparing);
        return Err(err);
    }
    let lock_path = preparing.join(JOB_LOCK_FILE_NAME);
    let lock = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        Ok(lock) => lock,
        Err(err) => {
            let _ = fs::remove_dir_all(&preparing);
            return Err(err).with_context(|| {
                format!(
                    "CHZZK VOD ownership lock 생성 실패: {}",
                    lock_path.display()
                )
            });
        }
    };
    if let Err(err) = restrict_cookie_file(&lock_path) {
        drop(lock);
        let _ = fs::remove_dir_all(&preparing);
        return Err(err);
    }
    if let Err(err) = lock.lock_exclusive() {
        drop(lock);
        let _ = fs::remove_dir_all(&preparing);
        return Err(err).context("CHZZK VOD ownership lock 획득 실패");
    }
    if let Err(err) = fs::rename(&preparing, &dir) {
        let _ = FileExt::unlock(&lock);
        drop(lock);
        let _ = fs::remove_dir_all(&preparing);
        return Err(err).with_context(|| {
            format!(
                "CHZZK VOD 임시 폴더 publish 실패: {} -> {}",
                preparing.display(),
                dir.display()
            )
        });
    }
    Ok(JobDirGuard { path: dir, lock })
}

#[cfg(windows)]
fn restrict_job_dir(dir: &Path) -> Result<()> {
    let username = env::var("USERNAME").context("Windows USERNAME 환경 변수가 없습니다.")?;
    let domain = env::var("USERDOMAIN").unwrap_or_default();
    let identity = if domain.trim().is_empty() || domain == "." {
        username
    } else {
        format!("{domain}\\{username}")
    };
    let status = StdCommand::new("icacls.exe")
        .arg(dir)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{identity}:(OI)(CI)F"))
        .arg("/Q")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("CHZZK 임시 폴더 ACL 설정 실패: {}", dir.display()))?;
    if !status.success() {
        let _ = fs::remove_dir_all(dir);
        bail!("CHZZK 임시 폴더를 현재 사용자 전용으로 제한하지 못했습니다.");
    }
    Ok(())
}

#[cfg(unix)]
fn restrict_job_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("CHZZK 임시 폴더 권한 설정 실패: {}", dir.display()))
}

#[cfg(not(any(windows, unix)))]
fn restrict_job_dir(_dir: &Path) -> Result<()> {
    Ok(())
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

fn collision_path(dir: &Path, base: &str, extension: &str) -> Result<PathBuf> {
    let first = dir.join(format!("{base}.{extension}"));
    if !first.exists() {
        return Ok(first);
    }
    for number in 2..=9999 {
        let candidate = dir.join(format!("{base}_{number:02}.{extension}"));
        if !candidate.exists() {
            return Ok(candidate);
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
                    "mp4" | "mkv" | "webm"
                )
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("yt-dlp 완료 파일을 찾지 못했습니다: {}", expected.display()))
}

fn finalizing_path(target: &Path) -> PathBuf {
    let mut name = target.as_os_str().to_os_string();
    name.push(format!(".{}.finalizing", Uuid::new_v4().simple()));
    PathBuf::from(name)
}

fn publish_by_copy(source: &Path, target: &Path, rename_error: &std::io::Error) -> Result<()> {
    if target.exists() {
        bail!(
            "CHZZK VOD 최종 파일이 이미 존재합니다: {}",
            target.display()
        );
    }
    let temp = finalizing_path(target);
    let publish = (|| -> Result<()> {
        fs::copy(source, &temp).with_context(|| {
            format!(
                "CHZZK VOD destination 임시 복사 실패: {} -> {}",
                source.display(),
                temp.display()
            )
        })?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&temp)
            .and_then(|file| file.sync_all())
            .with_context(|| {
                format!(
                    "CHZZK VOD destination 임시 파일 sync 실패: {}",
                    temp.display()
                )
            })?;
        if target.exists() {
            bail!(
                "CHZZK VOD 최종 파일이 복사 중 생성되었습니다: {}",
                target.display()
            );
        }
        fs::rename(&temp, target).with_context(|| {
            format!(
                "CHZZK VOD destination 임시 파일 publish 실패: {} -> {}",
                temp.display(),
                target.display()
            )
        })?;
        Ok(())
    })();
    if let Err(err) = publish {
        let _ = fs::remove_file(&temp);
        return Err(err).with_context(|| {
            format!(
                "CHZZK VOD 최종 파일 이동 실패 (rename: {rename_error}): {} -> {}",
                source.display(),
                target.display()
            )
        });
    }
    let _ = fs::remove_file(source);
    Ok(())
}

fn finalize_output(source: &Path, target: &Path) -> Result<()> {
    if target.exists() {
        bail!(
            "CHZZK VOD 최종 파일이 이미 존재합니다: {}",
            target.display()
        );
    }
    if let Err(rename_error) = fs::rename(source, target) {
        publish_by_copy(source, target, &rename_error)?;
    }
    if !target.is_file() {
        bail!("CHZZK VOD 최종 파일 생성 확인 실패: {}", target.display());
    }
    Ok(())
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
    fn builds_quality_options_from_video_heights() {
        let value = serde_json::json!({
            "formats": [
                {"height": 1080, "vcodec": "avc1"},
                {"height": 720, "vcodec": "avc1"},
                {"height": 1080, "vcodec": "avc1"},
                {"height": null, "vcodec": "none"}
            ]
        });
        let options = quality_options(&value);
        assert_eq!(options[0].value, "best");
        assert_eq!(options[1].value, "best[height<=1080]");
        assert_eq!(options[2].value, "best[height<=720]");
    }

    #[test]
    fn maps_ui_quality_to_ytdlp_selector() {
        assert_eq!(format_selector("best"), "bestvideo*+bestaudio/best");
        assert_eq!(
            format_selector("best[height<=1080]"),
            "bestvideo*[height<=1080]+bestaudio/best[height<=1080]"
        );
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
    fn metadata_json_maps_to_common_analysis_shape() {
        let value = serde_json::json!({
            "title": "테스트 VOD",
            "channel": "테스트 채널",
            "channel_id": "0123456789abcdef0123456789abcdef",
            "duration": 1234.5,
            "upload_date": "20260913",
            "formats": [{"height": 1080, "vcodec": "avc1"}]
        });
        let metadata = metadata_from_json(&value).unwrap();
        assert_eq!(metadata.title, "테스트 VOD");
        assert_eq!(metadata.streamer, "테스트 채널");
        assert_eq!(metadata.date, "260913");
        assert_eq!(metadata.duration_seconds, 1234);
        let view = analysis_view("https://chzzk.naver.com/video/1", &metadata);
        assert_eq!(view.part_count, 1);
        assert_eq!(view.parts[0].part, 1);
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
        fs::write(temp.path().join("media.mp4.part-Frag1.part"), "part").unwrap();
        cleanup_job_media(temp.path());
        assert!(cookie.exists());
        assert!(lock.exists());
        assert!(!media.exists());
        assert!(!temp.path().join("media.mp4.part-Frag1.part").exists());
    }

    #[test]
    fn atomic_copy_publish_keeps_partial_data_out_of_final_name() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.mp4");
        let target = temp.path().join("final.mp4");
        fs::write(&source, b"complete-media").unwrap();
        let rename_error = std::io::Error::other("simulated cross-volume rename");

        publish_by_copy(&source, &target, &rename_error).unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&target).unwrap(), b"complete-media");
        assert!(
            fs::read_dir(temp.path())
                .unwrap()
                .flatten()
                .all(|entry| !entry.file_name().to_string_lossy().ends_with(".finalizing"))
        );
    }

    #[test]
    fn atomic_copy_publish_failure_preserves_source_and_existing_target() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.mp4");
        let target = temp.path().join("final.mp4");
        fs::write(&source, b"source").unwrap();
        fs::write(&target, b"existing").unwrap();
        let rename_error = std::io::Error::other("simulated cross-volume rename");

        assert!(publish_by_copy(&source, &target, &rename_error).is_err());
        assert!(source.exists());
        assert_eq!(fs::read(&target).unwrap(), b"existing");
        assert!(
            fs::read_dir(temp.path())
                .unwrap()
                .flatten()
                .all(|entry| !entry.file_name().to_string_lossy().ends_with(".finalizing"))
        );
    }

    #[test]
    fn yt_dlp_staging_name_is_short_and_title_independent() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join(MEDIA_FILE_NAME);
        assert_eq!(staging.file_name().unwrap(), MEDIA_FILE_NAME);
        assert!(!staging.to_string_lossy().contains("테스트 VOD"));
    }
}
