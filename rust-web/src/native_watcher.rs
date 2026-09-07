use crate::{backend::{read_channels, channels_path, LogBuffer}, model::{Channel, ChannelRuntimeStatus, NativeWatcherStatus as WatcherStatus}};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::{DateTime, Local, Utc};
use regex::Regex;
use reqwest::{header::{COOKIE, SET_COOKIE}, Client, Response};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
use tokio::{
    process::{Child, Command},
    sync::{mpsc, oneshot, Mutex, RwLock},
    task::JoinHandle,
};

const GB: u64 = 1024 * 1024 * 1024;
const DPAPI_PREFIX: &str = "dpapi:v1:";
const DPAPI_ENTROPY: &str = "SOOPLiveDownloader:v1";

#[derive(Debug, Clone)]
struct WatcherConfig {
    check_interval: u64,
    reload_interval: u64,
    retry_interval: u64,
    stall_timeout: u64,
    monitor_interval: u64,
    worker_max_retry: usize,
    min_free_space_gb: f64,
    output_dir: PathBuf,
    quality: String,
    file_name_pattern: String,
    streamlink: PathBuf,
    worker_url: String,
    worker_api_key: String,
    soop_username: String,
    soop_password: String,
}

#[derive(Debug, Clone)]
struct LiveInfo {
    bno: String,
    bj_nick: String,
    title: String,
    rmd: String,
    bpwd: String,
}

#[derive(Debug, Clone)]
struct StreamInfo {
    quality: String,
    cdn: String,
    host: String,
    playlist_url: String,
}

struct Recording {
    child: Child,
    pid: u32,
    bno: String,
    title: String,
    file: PathBuf,
    output_dir: PathBuf,
    started_at: DateTime<Utc>,
    last_size: u64,
    last_growth: Instant,
    last_monitor: Instant,
    stdout_file: PathBuf,
    stderr_file: PathBuf,
}

struct ChannelState {
    channel: Channel,
    status: String,
    recording: Option<Recording>,
    last_bno: Option<String>,
    suppressed_bno: Option<String>,
    next_check: Instant,
    detail: Option<String>,
}

impl ChannelState {
    fn new(channel: Channel) -> Self {
        Self {
            channel,
            status: "UNKNOWN".into(),
            recording: None,
            last_bno: None,
            suppressed_bno: None,
            next_check: Instant::now(),
            detail: None,
        }
    }
}

#[derive(Debug)]
enum WatcherCommand {
    StopOnce(String),
    Resume(String),
    Recheck(String),
}

struct Runtime {
    task: Option<JoinHandle<()>>,
    stop_tx: Option<oneshot::Sender<()>>,
    command_tx: Option<mpsc::Sender<WatcherCommand>>,
}

pub struct NativeWatcherManager {
    backend_dir: PathBuf,
    logs: LogBuffer,
    runtime: Mutex<Runtime>,
    snapshot: Arc<RwLock<WatcherStatus>>,
}

impl NativeWatcherManager {
    pub fn new(backend_dir: PathBuf, logs: LogBuffer) -> Self {
        Self {
            backend_dir,
            logs,
            runtime: Mutex::new(Runtime { task: None, stop_tx: None, command_tx: None }),
            snapshot: Arc::new(RwLock::new(WatcherStatus::default())),
        }
    }

    pub async fn start(&self) -> Result<WatcherStatus> {
        let mut runtime = self.runtime.lock().await;
        if let Some(task) = runtime.task.as_ref() {
            if !task.is_finished() {
                return Ok(self.snapshot.read().await.clone());
            }
        }
        runtime.task.take();
        runtime.stop_tx = None;
        runtime.command_tx = None;

        let config = WatcherConfig::load(&self.backend_dir).await?;
        let channels = read_channels(&channels_path(&self.backend_dir))?;
        if channels.is_empty() {
            self.logs.push("[RUST] watcher starting with an empty channel list").await;
        }

        let (stop_tx, stop_rx) = oneshot::channel();
        let (command_tx, command_rx) = mpsc::channel(32);
        let backend_dir = self.backend_dir.clone();
        let logs = self.logs.clone();
        let snapshot = self.snapshot.clone();
        let started = Utc::now();

        {
            let mut s = snapshot.write().await;
            *s = WatcherStatus {
                running: true,
                pid: None,
                last_exit_code: None,
                engine: "rust-native",
                started_at: Some(started.to_rfc3339()),
                channel_count: channels.len(),
                recording_count: 0,
                offline_count: 0,
                error_count: 0,
                channels: channels.iter().map(|c| ChannelRuntimeStatus {
                    account: c.account.clone(), name: c.name.clone(), status: if c.enabled { "UNKNOWN".into() } else { "DISABLED".into() }, ..Default::default()
                }).collect(),
            };
        }

        let task = tokio::spawn(async move {
            let result = run_native_watcher(backend_dir, config, channels, logs.clone(), snapshot.clone(), stop_rx, command_rx).await;
            if let Err(err) = result {
                logs.push(format!("[RUST:ERR] watcher stopped with error: {err:#}")).await;
            }
            let mut s = snapshot.write().await;
            s.running = false;
        });

        runtime.stop_tx = Some(stop_tx);
        runtime.command_tx = Some(command_tx);
        runtime.task = Some(task);
        self.logs.push("[RUST] native watcher started").await;
        Ok(self.snapshot.read().await.clone())
    }

    pub async fn stop(&self) -> Result<WatcherStatus> {
        let task = {
            let mut runtime = self.runtime.lock().await;
            if let Some(tx) = runtime.stop_tx.take() { let _ = tx.send(()); }
            runtime.command_tx = None;
            runtime.task.take()
        };
        if let Some(task) = task {
            let _ = task.await;
        }
        {
            let mut s = self.snapshot.write().await;
            s.running = false;
        }
        self.logs.push("[RUST] native watcher stopped").await;
        Ok(self.snapshot.read().await.clone())
    }

    pub async fn status(&self) -> Result<WatcherStatus> {
        let finished = {
            let runtime = self.runtime.lock().await;
            runtime.task.as_ref().is_some_and(|t| t.is_finished())
        };
        if finished {
            let mut runtime = self.runtime.lock().await;
            runtime.task.take();
            runtime.stop_tx = None;
            runtime.command_tx = None;
            self.snapshot.write().await.running = false;
        }
        Ok(self.snapshot.read().await.clone())
    }

    pub async fn channel_action(&self, account: String, action: &str) -> Result<()> {
        let tx = {
            let runtime = self.runtime.lock().await;
            runtime.command_tx.clone().ok_or_else(|| anyhow!("watcher is not running"))?
        };
        let cmd = match action {
            "stop" => WatcherCommand::StopOnce(account),
            "resume" => WatcherCommand::Resume(account),
            "recheck" => WatcherCommand::Recheck(account),
            _ => bail!("unsupported channel action: {action}"),
        };
        tx.send(cmd).await.context("watcher command channel closed")?;
        Ok(())
    }
}

async fn run_native_watcher(
    backend_dir: PathBuf,
    mut config: WatcherConfig,
    initial_channels: Vec<Channel>,
    logs: LogBuffer,
    snapshot: Arc<RwLock<WatcherStatus>>,
    mut stop_rx: oneshot::Receiver<()>,
    mut command_rx: mpsc::Receiver<WatcherCommand>,
) -> Result<()> {
    // Match the legacy PowerShell direct-client behavior as closely as possible:
    // - bypass environment/system proxies
    // - use the Windows native TLS stack (Schannel via reqwest native-tls)
    // - keep SOOP requests on HTTP/1.1 instead of negotiating HTTP/2
    let client = Client::builder()
        .no_proxy()
        .http1_only()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36")
        .timeout(Duration::from_secs(15))
        .build()?;
    let mut session = SoopSession::new(client);
    if !config.soop_username.is_empty() && !config.soop_password.is_empty() {
        match session.login(&config.soop_username, &config.soop_password).await {
            Ok(login) => logs.push(format!("[RUST:AUTH] SOOP login OK : {login}")).await,
            Err(err) => logs.push(format!("[RUST:WARN] initial SOOP login failed; continuing: {err:#}")).await,
        }
    }

    let mut states: HashMap<String, ChannelState> = HashMap::new();
    apply_channels(&mut states, initial_channels, &logs).await;
    let mut next_reload = Instant::now();
    let mut last_channel_mtime = modified(&channels_path(&backend_dir));
    let settings_file = backend_dir.join("SOOP_LIVE_SETTING.ini");
    let mut last_setting_mtime = modified(&settings_file);
    let mut next_setting_check = Instant::now();

    logs.push(format!(
        "[RUST] watcher ready | check={}s reload={}s output={} streamlink={}",
        config.check_interval, config.reload_interval, config.output_dir.display(), config.streamlink.display()
    )).await;

    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            Some(command) = command_rx.recv() => {
                handle_command(command, &mut states, &logs).await;
                update_snapshot(&states, &snapshot).await;
            }
            _ = tokio::time::sleep(Duration::from_millis(400)) => {
                let now = Instant::now();

                if now >= next_setting_check {
                    next_setting_check = now + Duration::from_secs(1);
                    let mtime = modified(&settings_file);
                    if mtime.is_some() && mtime != last_setting_mtime {
                        match WatcherConfig::load(&backend_dir).await {
                            Ok(new_config) => {
                                let auth_changed = new_config.soop_username != config.soop_username || new_config.soop_password != config.soop_password;
                                config = new_config;
                                last_setting_mtime = mtime;
                                logs.push("[RUST] settings hot reload applied").await;
                                if auth_changed && !config.soop_username.is_empty() && !config.soop_password.is_empty() {
                                    match session.login(&config.soop_username, &config.soop_password).await {
                                        Ok(login) => logs.push(format!("[RUST:AUTH] SOOP login refreshed : {login}")).await,
                                        Err(err) => logs.push(format!("[RUST:WARN] SOOP login refresh failed: {err:#}")).await,
                                    }
                                }
                            }
                            Err(err) => logs.push(format!("[RUST:WARN] settings reload rejected; previous values kept: {err:#}")).await,
                        }
                    }
                }

                if now >= next_reload {
                    next_reload = now + Duration::from_secs(config.reload_interval.max(1));
                    let path = channels_path(&backend_dir);
                    let mtime = modified(&path);
                    if mtime.is_some() && mtime != last_channel_mtime {
                        match read_channels(&path) {
                            Ok(channels) => {
                                apply_channels(&mut states, channels, &logs).await;
                                last_channel_mtime = mtime;
                            }
                            Err(err) => logs.push(format!("[RUST:WARN] channel reload failed; previous list kept: {err:#}")).await,
                        }
                    }
                }

                monitor_recordings(&mut states, &config, &logs).await;
                poll_channels(&mut states, &config, &mut session, &logs).await;
                update_snapshot(&states, &snapshot).await;
            }
        }
    }

    for state in states.values_mut() {
        if state.recording.is_some() {
            let _ = stop_recording(state, "WATCHER EXIT", &logs).await;
        }
    }
    update_snapshot(&states, &snapshot).await;
    Ok(())
}

async fn apply_channels(states: &mut HashMap<String, ChannelState>, channels: Vec<Channel>, logs: &LogBuffer) {
    let mut incoming = HashMap::new();
    for channel in channels {
        incoming.insert(channel.account.to_ascii_lowercase(), channel);
    }

    let existing: Vec<String> = states.keys().cloned().collect();
    for key in existing {
        if !incoming.contains_key(&key) {
            if let Some(mut state) = states.remove(&key) {
                let _ = stop_recording(&mut state, "CHANNEL REMOVED", logs).await;
                logs.push(format!("[RUST] channel removed: {}", state.channel.account)).await;
            }
        }
    }

    for (key, channel) in incoming {
        if let Some(state) = states.get_mut(&key) {
            let was_enabled = state.channel.enabled;
            state.channel = channel;
            if was_enabled && !state.channel.enabled {
                let _ = stop_recording(state, "CHANNEL DISABLED", logs).await;
                state.status = "DISABLED".into();
            } else if !was_enabled && state.channel.enabled {
                state.status = "UNKNOWN".into();
                state.next_check = Instant::now();
            }
        } else {
            states.insert(key, ChannelState::new(channel));
        }
    }
}

async fn handle_command(cmd: WatcherCommand, states: &mut HashMap<String, ChannelState>, logs: &LogBuffer) {
    let (account, action) = match &cmd {
        WatcherCommand::StopOnce(a) => (a, "stop"),
        WatcherCommand::Resume(a) => (a, "resume"),
        WatcherCommand::Recheck(a) => (a, "recheck"),
    };
    let key = account.to_ascii_lowercase();
    let Some(state) = states.get_mut(&key) else {
        logs.push(format!("[RUST:WARN] channel command target not found: {account}")).await;
        return;
    };
    match cmd {
        WatcherCommand::StopOnce(_) => {
            state.suppressed_bno = state.last_bno.clone().or_else(|| state.recording.as_ref().map(|r| r.bno.clone()));
            let _ = stop_recording(state, "USER CHANNEL STOP", logs).await;
            state.status = "PAUSED".into();
        }
        WatcherCommand::Resume(_) => {
            state.suppressed_bno = None;
            state.status = "UNKNOWN".into();
            state.next_check = Instant::now();
        }
        WatcherCommand::Recheck(_) => state.next_check = Instant::now(),
    }
    logs.push(format!("[RUST] channel {action}: {}", state.channel.account)).await;
}

async fn poll_channels(states: &mut HashMap<String, ChannelState>, config: &WatcherConfig, session: &mut SoopSession, logs: &LogBuffer) {
    let keys: Vec<String> = states.keys().cloned().collect();
    for key in keys {
        let Some(state) = states.get_mut(&key) else { continue; };
        if !state.channel.enabled || state.recording.is_some() || Instant::now() < state.next_check { continue; }
        state.next_check = Instant::now() + Duration::from_secs(config.check_interval.max(1));

        match session.live_info(&state.channel.account).await {
            Ok(LiveResult::Offline) => {
                state.status = "OFFLINE".into();
                state.last_bno = None;
                state.detail = None;
            }
            Ok(LiveResult::AuthRequired) => {
                state.status = "AUTH".into();
                match session.login(&config.soop_username, &config.soop_password).await {
                    Ok(_) => state.next_check = Instant::now(),
                    Err(err) => state.detail = Some(format!("login failed: {err}")),
                }
            }
            Ok(LiveResult::Live(live)) => {
                if state.channel.name.eq_ignore_ascii_case(&state.channel.account) && !live.bj_nick.is_empty() {
                    state.channel.name = live.bj_nick.clone();
                }
                if state.suppressed_bno.as_deref() == Some(live.bno.as_str()) {
                    state.status = "PAUSED".into();
                    state.last_bno = Some(live.bno);
                    continue;
                }
                if state.suppressed_bno.is_some() { state.suppressed_bno = None; }

                match start_recording(&state.channel, &live, config, session, logs).await {
                    Ok(recording) => {
                        state.last_bno = Some(live.bno.clone());
                        state.status = "RECORDING".into();
                        state.detail = None;
                        state.recording = Some(recording);
                    }
                    Err(err) => {
                        state.status = "ERROR".into();
                        state.detail = Some(err.to_string());
                        state.next_check = Instant::now() + Duration::from_secs(config.retry_interval.max(1));
                        logs.push(format!("[RUST:ERR] record start failed {}: {err:#}", state.channel.account)).await;
                    }
                }
            }
            Err(err) => {
                state.status = "ERROR".into();
                state.detail = Some(err.to_string());
                logs.push(format!("[RUST:WARN] live check failed {}: {err:#}", state.channel.account)).await;
            }
        }
    }
}

async fn monitor_recordings(states: &mut HashMap<String, ChannelState>, config: &WatcherConfig, logs: &LogBuffer) {
    let keys: Vec<String> = states.keys().cloned().collect();
    for key in keys {
        let Some(state) = states.get_mut(&key) else { continue; };
        let Some(rec) = state.recording.as_mut() else { continue; };

        match rec.child.try_wait() {
            Ok(Some(status)) => {
                let code = status.code();
                let reason = if code.unwrap_or(0) == 0 { "NORMAL".to_string() } else { format!("RECORDER EXIT CODE={}", code.map(|x| x.to_string()).unwrap_or_else(|| "unknown".into())) };
                finish_recording(state, &reason, logs).await;
                state.next_check = Instant::now();
                continue;
            }
            Ok(None) => {}
            Err(err) => {
                state.detail = Some(format!("recorder status error: {err}"));
            }
        }

        if rec.last_monitor.elapsed() < Duration::from_secs(config.monitor_interval.max(1)) { continue; }
        rec.last_monitor = Instant::now();

        match free_gb(&rec.output_dir) {
            Ok(free) if free < config.min_free_space_gb => {
                state.status = "LOW_DISK".into();
                state.detail = Some(format!("free {free:.2} GB / limit {:.2} GB", config.min_free_space_gb));
                let _ = stop_recording(state, "LOW DISK SPACE", logs).await;
                state.next_check = Instant::now() + Duration::from_secs(30);
                continue;
            }
            Err(err) => logs.push(format!("[RUST:WARN] disk check failed {}: {err:#}", rec.output_dir.display())).await,
            _ => {}
        }

        let size = fs::metadata(&rec.file).map(|m| m.len()).unwrap_or(0);
        if size > rec.last_size {
            rec.last_size = size;
            rec.last_growth = Instant::now();
        } else if rec.last_growth.elapsed() >= Duration::from_secs(config.stall_timeout.max(10)) {
            state.status = "STALLED".into();
            state.detail = Some(format!("{}s no growth", config.stall_timeout));
            let _ = stop_recording(state, "RECORD STALLED", logs).await;
            state.next_check = Instant::now();
        }
    }
}

async fn start_recording(channel: &Channel, live: &LiveInfo, config: &WatcherConfig, session: &mut SoopSession, logs: &LogBuffer) -> Result<Recording> {
    let stream = session.worker_playlist(channel, live, config).await?;
    let output_dir = channel_output_dir(channel, &config.output_dir)?;
    let free = free_gb(&output_dir)?;
    if free < config.min_free_space_gb {
        bail!("LOW DISK SPACE - free={free:.2}GB limit={:.2}GB", config.min_free_space_gb);
    }
    let output_file = unique_output_file(&output_dir, &channel.name, &live.title, &config.file_name_pattern)?;
    let stream_url = if stream.playlist_url.to_ascii_lowercase().starts_with("hls://") { stream.playlist_url.clone() } else { format!("hls://{}", stream.playlist_url) };
    let console_base = env::temp_dir().join(format!("soop_streamlink_rust_{}_{}", std::process::id(), uuid::Uuid::new_v4().simple()));
    let stdout_file = console_base.with_extension("stdout.log");
    let stderr_file = console_base.with_extension("stderr.log");
    let stdout = fs::File::create(&stdout_file)?;
    let stderr = fs::File::create(&stderr_file)?;

    let mut command = Command::new(&config.streamlink);
    command
        .arg(stream_url)
        .arg(&config.quality)
        .arg("--output").arg(&output_file)
        .arg("--force")
        .arg("--hls-live-edge").arg("3")
        .arg("--stream-segment-threads").arg("3")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .kill_on_drop(false);
    if let Some(parent) = config.streamlink.parent() {
        if parent.is_dir() { command.current_dir(parent); }
    }
    let child = command.spawn().with_context(|| format!("failed to start Streamlink: {}", config.streamlink.display()))?;
    let pid = child.id().ok_or_else(|| anyhow!("Streamlink PID unavailable"))?;
    let now = Utc::now();
    logs.push(format!(
        "[RUST] RECORD START channel={} account={} bno={} title={} hls={} cdn={} host={} pid={} file={}",
        channel.name, channel.account, live.bno, live.title, stream.quality, stream.cdn, stream.host, pid, output_file.display()
    )).await;

    Ok(Recording {
        child, pid, bno: live.bno.clone(), title: live.title.clone(), file: output_file, output_dir,
        started_at: now, last_size: 0, last_growth: Instant::now(), last_monitor: Instant::now(), stdout_file, stderr_file,
    })
}

async fn stop_recording(state: &mut ChannelState, reason: &str, logs: &LogBuffer) -> Result<()> {
    let Some(mut rec) = state.recording.take() else { return Ok(()); };
    if rec.child.try_wait()?.is_none() {
        #[cfg(windows)]
        {
            let status = Command::new("taskkill.exe")
                .arg("/PID").arg(rec.pid.to_string()).arg("/T").arg("/F")
                .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().await?;
            if !status.success() && rec.child.try_wait()?.is_none() {
                state.recording = Some(rec);
                bail!("taskkill failed for recorder pid={}", state.recording.as_ref().unwrap().pid);
            }
        }
        #[cfg(not(windows))]
        {
            let _ = rec.child.kill().await;
        }
    }
    let _ = rec.child.wait().await;
    log_recording_finish(state, &rec, reason, logs).await;
    cleanup_recorder_files(&rec);
    Ok(())
}

async fn finish_recording(state: &mut ChannelState, reason: &str, logs: &LogBuffer) {
    if let Some(rec) = state.recording.take() {
        log_recording_finish(state, &rec, reason, logs).await;
        cleanup_recorder_files(&rec);
    }
}

async fn log_recording_finish(state: &ChannelState, rec: &Recording, reason: &str, logs: &LogBuffer) {
    let size = fs::metadata(&rec.file).map(|m| m.len()).unwrap_or(rec.last_size);
    let secs = (Utc::now() - rec.started_at).num_seconds().max(0);
    logs.push(format!(
        "[RUST] RECORD FINISHED channel={} account={} duration={}s size={} reason={} file={}",
        state.channel.name, state.channel.account, secs, size, reason, rec.file.display()
    )).await;
}

fn cleanup_recorder_files(rec: &Recording) {
    let _ = fs::remove_file(&rec.stdout_file);
    let _ = fs::remove_file(&rec.stderr_file);
}

async fn update_snapshot(states: &HashMap<String, ChannelState>, snapshot: &Arc<RwLock<WatcherStatus>>) {
    let mut channels = Vec::with_capacity(states.len());
    let mut recording_count = 0;
    let mut offline_count = 0;
    let mut error_count = 0;
    let mut ordered: Vec<&ChannelState> = states.values().collect();
    ordered.sort_by(|a,b| a.channel.name.cmp(&b.channel.name));
    for state in ordered {
        if state.status == "RECORDING" { recording_count += 1; }
        if state.status == "OFFLINE" { offline_count += 1; }
        if matches!(state.status.as_str(), "ERROR" | "LOW_DISK" | "STALLED") { error_count += 1; }
        let rec = state.recording.as_ref();
        channels.push(ChannelRuntimeStatus {
            account: state.channel.account.clone(),
            name: state.channel.name.clone(),
            status: if !state.channel.enabled { "DISABLED".into() } else { state.status.clone() },
            bno: rec.map(|r| r.bno.clone()).or_else(|| state.last_bno.clone()),
            title: rec.map(|r| r.title.clone()),
            file: rec.map(|r| r.file.display().to_string()),
            size_bytes: rec.and_then(|r| fs::metadata(&r.file).ok().map(|m| m.len())).unwrap_or(0),
            started_at: rec.map(|r| r.started_at.to_rfc3339()),
            suppressed: state.suppressed_bno.is_some(),
            detail: state.detail.clone(),
        });
    }
    let mut s = snapshot.write().await;
    s.channel_count = channels.len();
    s.recording_count = recording_count;
    s.offline_count = offline_count;
    s.error_count = error_count;
    s.channels = channels;
}

#[derive(Debug)]
enum LiveResult { Offline, AuthRequired, Live(LiveInfo) }

struct SoopSession {
    client: Client,
    cookies: BTreeMap<String, String>,
    bno_regex: Regex,
}

impl SoopSession {
    fn new(client: Client) -> Self {
        Self { client, cookies: BTreeMap::new(), bno_regex: Regex::new(r"window\.nBroadNo\s*=\s*(\d+);").unwrap() }
    }

    fn cookie_header(&self) -> String {
        self.cookies.iter().map(|(k,v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
    }

    fn worker_cookie_header(&self) -> String {
        const ALLOW: &[&str] = &["AuthTicket","BbsTicket","UserTicket","BbsSaveTicket","RDB","PdboxTicket","PdboxBbs","PdboxUser","PdboxSaveTicket"];
        self.cookies.iter().filter(|(k,_)| ALLOW.iter().any(|x| k.eq_ignore_ascii_case(x))).map(|(k,v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
    }

    async fn login(&mut self, username: &str, password: &str) -> Result<String> {
        if username.trim().is_empty() || password.trim().is_empty() { bail!("SOOP username/password is empty"); }
        self.cookies.clear();
        let response = self.client.post("https://login.sooplive.com/app/LoginAction.php")
            .header("Referer", "https://www.sooplive.com/")
            .form(&[
                ("szWork", "login"), ("szType", "json"), ("szUid", username), ("szPassword", password),
                ("isSaveId", "true"), ("isSavePw", "false"), ("isSaveJoin", "false"), ("isLoginRetain", "Y")
            ]).send().await?;
        collect_cookies(&mut self.cookies, &response);
        let value: Value = response.error_for_status()?.json().await?;
        if value.get("RESULT").and_then(Value::as_i64) != Some(1) { bail!("SOOP login failed RESULT={:?}", value.get("RESULT")); }

        let response = self.client.get("https://afevent2.sooplive.com/api/get_private_info.php")
            .header("Referer", "https://www.sooplive.com/")
            .header(COOKIE, self.cookie_header())
            .send().await?;
        collect_cookies(&mut self.cookies, &response);
        let auth: Value = response.error_for_status()?.json().await?;
        let login_id = auth.pointer("/CHANNEL/LOGIN_ID").and_then(Value::as_str).unwrap_or("").to_string();
        if login_id.is_empty() { bail!("SOOP login verification failed"); }
        Ok(login_id)
    }

    async fn live_info(&self, account: &str) -> Result<LiveResult> {
        let channel_url = format!("https://play.sooplive.com/{account}");
        let mut req = self.client.get(&channel_url).header("Referer", "https://play.sooplive.com/");
        let cookie = self.cookie_header();
        if !cookie.is_empty() { req = req.header(COOKIE, cookie.clone()); }
        let html = req.send().await?.error_for_status()?.text().await?;
        let Some(caps) = self.bno_regex.captures(&html) else { return Ok(LiveResult::Offline); };
        let bno = caps.get(1).unwrap().as_str().to_string();
        let mut req = self.client.post("https://live.sooplive.com/afreeca/player_live_api.php")
            .header("Referer", &channel_url)
            .form(&[
                ("from_api", "0"), ("mode", "landing"), ("player_type", "html5"), ("stream_type", "common"),
                ("type", "live"), ("bid", account), ("bno", bno.as_str()), ("pwd", "")
            ]);
        if !cookie.is_empty() { req = req.header(COOKIE, cookie); }
        let v: Value = req.send().await?.error_for_status()?.json().await?;
        let Some(ch) = v.get("CHANNEL") else { return Ok(LiveResult::Offline); };
        if ch.get("RESULT").and_then(Value::as_i64) == Some(-6) { return Ok(LiveResult::AuthRequired); }
        if ch.get("RESULT").and_then(Value::as_i64) != Some(1) { return Ok(LiveResult::Offline); }
        let get = |key: &str| ch.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        let rmd = get("RMD");
        let api_bno = get("BNO");
        if rmd.is_empty() || api_bno.is_empty() { return Ok(LiveResult::Offline); }
        Ok(LiveResult::Live(LiveInfo { bno: api_bno, bj_nick: get("BJNICK"), title: get("TITLE"), rmd, bpwd: get("BPWD") }))
    }

    async fn worker_playlist(&mut self, channel: &Channel, live: &LiveInfo, config: &WatcherConfig) -> Result<StreamInfo> {
        let mut last = String::new();
        for attempt in 1..=config.worker_max_retry.max(1) {
            let cookies = self.worker_cookie_header();
            let body = json!({
                "account": channel.account, "bno": live.bno, "rmd": live.rmd, "quality": "master", "cq": "sd",
                "password": live.bpwd, "cookie": cookies,
                "bid": channel.account, "bpwd": live.bpwd, "channel_url": format!("https://play.sooplive.com/{}", channel.account),
                "soop_cookie_header": self.worker_cookie_header()
            });
            match self.client.post(&config.worker_url).header("X-API-Key", &config.worker_api_key).json(&body).send().await {
                Ok(response) => {
                    let status = response.status();
                    let text = response.text().await.unwrap_or_default();
                    if status.is_success() {
                        if let Ok(v) = serde_json::from_str::<Value>(&text) {
                            if v.get("success").and_then(Value::as_bool) == Some(true) {
                                let playlist = v.get("playlist_url").and_then(Value::as_str).unwrap_or("").to_string();
                                if !playlist.is_empty() {
                                    return Ok(StreamInfo {
                                        quality: v.get("quality").and_then(Value::as_str).unwrap_or("master").to_string(),
                                        cdn: v.get("cdn").and_then(Value::as_str).unwrap_or("").to_string(),
                                        host: v.get("host").and_then(Value::as_str).unwrap_or("").to_string(),
                                        playlist_url: playlist,
                                    });
                                }
                            }
                        }
                    }
                    last = format!("Worker HTTP {status}: {}", compact(&text, 240));
                }
                Err(err) => last = format!("Worker transport error: {err}"),
            }
            if attempt < config.worker_max_retry.max(1) {
                let delay = [2,5,10][(attempt-1).min(2)];
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        }
        bail!("{last}")
    }
}

fn collect_cookies(target: &mut BTreeMap<String,String>, response: &Response) {
    for value in response.headers().get_all(SET_COOKIE).iter() {
        if let Ok(text) = value.to_str() {
            if let Some(pair) = text.split(';').next() {
                if let Some((name, value)) = pair.split_once('=') {
                    target.insert(name.trim().to_string(), value.trim().to_string());
                }
            }
        }
    }
}

impl WatcherConfig {
    async fn load(backend_dir: &Path) -> Result<Self> {
        let path = backend_dir.join("SOOP_LIVE_SETTING.ini");
        let map = read_ini(&path)?;
        let secret = |name: &str| map.get(name).cloned().unwrap_or_default();
        let worker_api_key = resolve_secret(secret("CLOUDFLARE_API_KEY"), "CLOUDFLARE_API_KEY").await?;
        let soop_password = resolve_secret(secret("SOOP_PASSWORD"), "SOOP_PASSWORD").await?;
        let worker_url = secret("CLOUDFLARE_WORKER_URL");
        if !worker_url.starts_with("https://") { bail!("CLOUDFLARE_WORKER_URL must be https://"); }
        if worker_api_key.is_empty() { bail!("CLOUDFLARE_API_KEY is empty"); }
        let output_dir = PathBuf::from(map.get("OUTPUT_DIR").cloned().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| default_output_dir()));
        fs::create_dir_all(&output_dir)?;
        let streamlink = resolve_streamlink(backend_dir, &map)?;
        Ok(Self {
            check_interval: int(&map, "CHECK_INTERVAL", 30),
            reload_interval: int(&map, "CHANNEL_RELOAD_INTERVAL", 2),
            retry_interval: int(&map, "RECORD_RETRY_INTERVAL", 5),
            stall_timeout: int(&map, "RECORD_STALL_TIMEOUT", 90),
            monitor_interval: int(&map, "RECORD_MONITOR_INTERVAL", 5),
            worker_max_retry: int(&map, "WORKER_MAX_RETRY", 3) as usize,
            min_free_space_gb: map.get("MIN_FREE_SPACE_GB").and_then(|v| v.parse().ok()).unwrap_or(20.0),
            output_dir,
            quality: map.get("QUALITY").cloned().filter(|v| !v.is_empty()).unwrap_or_else(|| "best".into()),
            file_name_pattern: map.get("FILE_NAME_PATTERN").cloned().unwrap_or_else(|| "LEGACY".into()).to_ascii_uppercase(),
            streamlink,
            worker_url,
            worker_api_key,
            soop_username: secret("SOOP_USERNAME"),
            soop_password,
        })
    }
}

fn read_ini(path: &Path) -> Result<BTreeMap<String,String>> {
    let text = fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut map = BTreeMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some((k,v)) = line.split_once('=') { map.insert(k.trim().to_string(), v.trim().to_string()); }
    }
    Ok(map)
}

async fn resolve_secret(value: String, name: &str) -> Result<String> {
    if value.trim().is_empty() || !value.to_ascii_lowercase().starts_with(DPAPI_PREFIX) { return Ok(value); }
    #[cfg(windows)]
    {
        let script = format!(
            "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Security; $e=[Text.Encoding]::UTF8.GetBytes('{}'); $x=[Convert]::FromBase64String($env:SOOP_DPAPI_VALUE.Substring(9)); $p=[Security.Cryptography.ProtectedData]::Unprotect($x,$e,[Security.Cryptography.DataProtectionScope]::CurrentUser); [Console]::Out.Write([Convert]::ToBase64String($p))",
            DPAPI_ENTROPY
        );
        let out = Command::new("powershell.exe").arg("-NoLogo").arg("-NoProfile").arg("-NonInteractive").arg("-Command").arg(script)
            .env("SOOP_DPAPI_VALUE", &value).stdin(Stdio::null()).output().await.context("failed to invoke DPAPI compatibility bootstrap")?;
        if !out.status.success() { bail!("{name} DPAPI decrypt failed: {}", String::from_utf8_lossy(&out.stderr).trim()); }
        let plain = BASE64.decode(String::from_utf8_lossy(&out.stdout).trim()).context("invalid DPAPI bootstrap output")?;
        return String::from_utf8(plain).context("DPAPI secret is not UTF-8");
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        bail!("DPAPI-protected secrets require Windows in Phase 2; use plaintext/env migration for non-Windows testing")
    }
}

fn resolve_streamlink(backend_dir: &Path, map: &BTreeMap<String,String>) -> Result<PathBuf> {
    for key in ["STREAMLINK_PATH", "STREAMLINK_FALLBACK"] {
        if let Some(value) = map.get(key) {
            if !value.trim().is_empty() && !value.eq_ignore_ascii_case("AUTO") {
                let path = PathBuf::from(value);
                if path.is_file() { return Ok(path); }
            }
        }
    }
    let mut candidates = vec![backend_dir.join("streamlink.exe")];
    #[cfg(windows)]
    {
        candidates.push(PathBuf::from(r"C:\Program Files\Streamlink\bin\streamlink.exe"));
        candidates.push(PathBuf::from(r"C:\Program Files\Streamlink\streamlink.exe"));
    }
    for p in candidates { if p.is_file() { return Ok(p); } }
    bail!("streamlink executable not found; set STREAMLINK_PATH or STREAMLINK_FALLBACK")
}

fn channel_output_dir(channel: &Channel, default: &Path) -> Result<PathBuf> {
    let base = if channel.outdir.trim().is_empty() { default.to_path_buf() } else { PathBuf::from(channel.outdir.trim()) };
    let dir = base.join(safe_name(&channel.name, 80));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn unique_output_file(dir: &Path, channel: &str, title: &str, pattern: &str) -> Result<PathBuf> {
    let now = Local::now();
    let date = now.format("%y%m%d").to_string();
    let time = now.format("%H%M%S").to_string();
    let ch = safe_name(channel, 60);
    let title = safe_name(title, 90);
    let base = match pattern {
        "TIME_TITLE" => format!("{date}_{time}_{title}_{ch}"),
        "BJ_TITLE" => format!("{date}_{ch}_{title}"),
        "TITLE_NUMBER" => {
            for n in 1..=9999 {
                let p = dir.join(format!("{date}_{title}_{n:02}_{ch}.ts"));
                if !p.exists() { return Ok(p); }
            }
            bail!("too many TITLE_NUMBER filename collisions")
        }
        _ => format!("{date}_{time}_{ch}"),
    };
    let mut p = dir.join(format!("{base}.ts"));
    for n in 2..=9999 {
        if !p.exists() { return Ok(p); }
        p = dir.join(format!("{base}_{n:02}.ts"));
    }
    bail!("too many output filename collisions")
}

fn safe_name(value: &str, max: usize) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if c.is_control() { continue; }
        if r#"\/:*?"<>|"#.contains(c) { out.push('_'); } else { out.push(c); }
    }
    let mut out = out.split_whitespace().collect::<Vec<_>>().join(" ").trim().trim_end_matches('.').to_string();
    if out.is_empty() { out = "UNKNOWN".into(); }
    if out.chars().count() > max { out = out.chars().take(max).collect::<String>().trim().trim_end_matches('.').to_string(); }
    let upper = out.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON"|"PRN"|"AUX"|"NUL"|"COM1"|"COM2"|"COM3"|"COM4"|"COM5"|"COM6"|"COM7"|"COM8"|"COM9"|"LPT1"|"LPT2"|"LPT3"|"LPT4"|"LPT5"|"LPT6"|"LPT7"|"LPT8"|"LPT9") { out.insert(0, '_'); }
    out
}

fn free_gb(path: &Path) -> Result<f64> { Ok(fs2::available_space(path)? as f64 / GB as f64) }
fn int(map: &BTreeMap<String,String>, key: &str, default: u64) -> u64 { map.get(key).and_then(|v| v.parse().ok()).unwrap_or(default) }
fn modified(path: &Path) -> Option<SystemTime> { fs::metadata(path).and_then(|m| m.modified()).ok() }
fn compact(text: &str, max: usize) -> String { let s = text.split_whitespace().collect::<Vec<_>>().join(" "); if s.chars().count() > max { format!("{}...", s.chars().take(max).collect::<String>()) } else { s } }
fn default_output_dir() -> String { if cfg!(windows) { r"C:\SOOP_LIVE".into() } else { "./SOOP_LIVE".into() } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn safe_filename_replaces_windows_reserved_chars() {
        assert_eq!(safe_name("a:b/c*?d", 80), "a_b_c__d");
        assert_eq!(safe_name("CON", 80), "_CON");
    }
    #[test]
    fn ini_parser_preserves_dpapi_text() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.ini");
        fs::write(&p, "A=1\r\nSOOP_PASSWORD=dpapi:v1:abc\r\n").unwrap();
        let m = read_ini(&p).unwrap();
        assert_eq!(m["SOOP_PASSWORD"], "dpapi:v1:abc");
    }
}
