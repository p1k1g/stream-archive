use crate::{
    backend::LogBuffer,
    model::{Channel, ChannelRuntimeStatus, NativeWatcherStatus as WatcherStatus},
    recorder::{RecorderConfig, RecorderManager, Recording, RecordingPoll},
    security::unprotect_secret,
    store,
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{Local, Utc};
use regex::Regex;
use reqwest::{
    Client, Response,
    header::{COOKIE, SET_COOKIE},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::{
    sync::{Mutex, RwLock, mpsc, oneshot},
    task::JoinHandle,
};

#[derive(Debug, Clone)]
struct WatcherConfig {
    check_interval: u64,
    reload_interval: u64,
    retry_interval: u64,
    worker_max_retry: usize,
    output_dir: PathBuf,
    file_name_pattern: String,
    worker_url: String,
    worker_api_key: String,
    soop_username: String,
    soop_password: String,
    recorder: RecorderConfig,
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

#[derive(Debug, Clone)]
struct StreamPassword {
    bno: String,
    value: String,
}

static STREAM_PASSWORDS: OnceLock<StdMutex<HashMap<String, StreamPassword>>> = OnceLock::new();

fn stream_passwords() -> &'static StdMutex<HashMap<String, StreamPassword>> {
    STREAM_PASSWORDS.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn is_password_protected(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_uppercase().as_str(),
        "Y" | "1" | "TRUE"
    )
}

fn stream_password_for(account: &str, bno: &str) -> Option<String> {
    let key = account.to_ascii_lowercase();
    let Ok(mut passwords) = stream_passwords().lock() else {
        return None;
    };
    match passwords.get(&key) {
        Some(item) if item.bno == bno => Some(item.value.clone()),
        Some(_) => {
            passwords.remove(&key);
            None
        }
        None => None,
    }
}

fn clear_stream_password(account: &str) {
    if let Ok(mut passwords) = stream_passwords().lock() {
        passwords.remove(&account.to_ascii_lowercase());
    }
}

fn clear_all_stream_passwords() {
    if let Ok(mut passwords) = stream_passwords().lock() {
        passwords.clear();
    }
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
        let status = if channel.enabled {
            "UNKNOWN"
        } else {
            "DISABLED"
        };
        Self {
            channel,
            status: status.into(),
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
            runtime: Mutex::new(Runtime {
                task: None,
                stop_tx: None,
                command_tx: None,
            }),
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

        let db = store::global()?;
        let settings = db.live_settings_with_secrets()?;
        let config = WatcherConfig::from_values(&self.backend_dir, &settings)?;
        let channels = db.channels()?;
        let (stop_tx, stop_rx) = oneshot::channel();
        let (command_tx, command_rx) = mpsc::channel(32);
        let backend_dir = self.backend_dir.clone();
        let logs = self.logs.clone();
        let snapshot = self.snapshot.clone();

        {
            let mut s = snapshot.write().await;
            *s = WatcherStatus {
                running: true,
                pid: None,
                last_exit_code: None,
                engine: "rust-native-v4-sqlite",
                started_at: Some(Utc::now().to_rfc3339()),
                channel_count: channels.len(),
                recording_count: 0,
                offline_count: 0,
                error_count: 0,
                channels: channels
                    .iter()
                    .map(|c| ChannelRuntimeStatus {
                        account: c.account.clone(),
                        name: c.name.clone(),
                        status: if c.enabled {
                            "UNKNOWN".into()
                        } else {
                            "DISABLED".into()
                        },
                        ..Default::default()
                    })
                    .collect(),
            };
        }

        let task = tokio::spawn(async move {
            let result = run_native_watcher(
                backend_dir,
                settings,
                config,
                channels,
                logs.clone(),
                snapshot.clone(),
                stop_rx,
                command_rx,
            )
            .await;
            if let Err(err) = result {
                logs.push(format!("[RUST:ERR] watcher stopped with error: {err:#}"))
                    .await;
            }
            snapshot.write().await.running = false;
        });

        runtime.stop_tx = Some(stop_tx);
        runtime.command_tx = Some(command_tx);
        runtime.task = Some(task);
        self.logs
            .push("[RUST] native watcher v4 started (SQLite direct)")
            .await;
        Ok(self.snapshot.read().await.clone())
    }

    pub async fn stop(&self) -> Result<WatcherStatus> {
        let task = {
            let mut runtime = self.runtime.lock().await;
            if let Some(tx) = runtime.stop_tx.take() {
                let _ = tx.send(());
            }
            runtime.command_tx = None;
            runtime.task.take()
        };
        if let Some(task) = task {
            let _ = task.await;
        }
        self.snapshot.write().await.running = false;
        self.logs.push("[RUST] native watcher v4 stopped").await;
        Ok(self.snapshot.read().await.clone())
    }

    pub async fn status(&self) -> Result<WatcherStatus> {
        let finished = {
            let runtime = self.runtime.lock().await;
            runtime.task.as_ref().is_some_and(|task| task.is_finished())
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
            runtime
                .command_tx
                .clone()
                .ok_or_else(|| anyhow!("watcher is not running"))?
        };
        let command = match action {
            "stop" => WatcherCommand::StopOnce(account),
            "resume" => WatcherCommand::Resume(account),
            "recheck" => WatcherCommand::Recheck(account),
            _ => bail!("unsupported channel action: {action}"),
        };
        tx.send(command)
            .await
            .context("watcher command channel closed")?;
        Ok(())
    }

    pub async fn channel_password(&self, account: String, password: String) -> Result<()> {
        let account = account.trim().to_string();
        if account.is_empty() {
            bail!("channel account is empty");
        }
        if password.is_empty() {
            bail!("stream password is empty");
        }
        let bno = {
            let snapshot = self.snapshot.read().await;
            let channel = snapshot
                .channels
                .iter()
                .find(|channel| channel.account.eq_ignore_ascii_case(&account))
                .ok_or_else(|| anyhow!("channel not found: {account}"))?;
            if channel.status != "PASSWORD_REQUIRED" {
                bail!("channel is not waiting for a stream password");
            }
            channel
                .bno
                .clone()
                .ok_or_else(|| anyhow!("protected broadcast number is unavailable"))?
        };
        {
            let mut passwords = stream_passwords()
                .lock()
                .map_err(|_| anyhow!("stream password memory store is unavailable"))?;
            passwords.insert(
                account.to_ascii_lowercase(),
                StreamPassword {
                    bno,
                    value: password,
                },
            );
        }
        self.logs
            .push(format!(
                "[RUST:AUTH] stream password supplied account={} (memory only)",
                account
            ))
            .await;
        self.channel_action(account, "recheck").await
    }
}

async fn run_native_watcher(
    backend_dir: PathBuf,
    mut last_settings: BTreeMap<String, String>,
    mut config: WatcherConfig,
    initial_channels: Vec<Channel>,
    logs: LogBuffer,
    snapshot: Arc<RwLock<WatcherStatus>>,
    mut stop_rx: oneshot::Receiver<()>,
    mut command_rx: mpsc::Receiver<WatcherCommand>,
) -> Result<()> {
    let client = Client::builder()
        .no_proxy()
        .http1_only()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36",
        )
        .timeout(Duration::from_secs(15))
        .build()?;
    let mut session = SoopSession::new(client);
    let recorder = RecorderManager::new(logs.clone());

    if !config.soop_username.is_empty() && !config.soop_password.is_empty() {
        match session
            .login(&config.soop_username, &config.soop_password)
            .await
        {
            Ok(login) => {
                logs.push(format!("[RUST:AUTH] SOOP login OK : {login}"))
                    .await
            }
            Err(err) => {
                logs.push(format!(
                    "[RUST:WARN] initial SOOP login failed; continuing: {err:#}"
                ))
                .await
            }
        }
    }

    clear_all_stream_passwords();
    let mut last_channel_signature = channel_signature(&initial_channels);
    let mut states = HashMap::new();
    apply_channels(&mut states, initial_channels, &recorder, &logs).await;
    let mut next_reload = Instant::now();
    let mut next_setting_check = Instant::now();

    logs.push(format!(
        "[RUST] watcher ready | source=sqlite check={}s reload={}s output={} streamlink={} secret_backend=native-dpapi",
        config.check_interval,
        config.reload_interval,
        config.output_dir.display(),
        config.recorder.streamlink.display()
    ))
    .await;

    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            Some(command) = command_rx.recv() => {
                handle_command(command, &mut states, &recorder, &logs).await;
                update_snapshot(&states, &snapshot).await;
            }
            _ = tokio::time::sleep(Duration::from_millis(400)) => {
                let now = Instant::now();

                if now >= next_setting_check {
                    next_setting_check = now + Duration::from_secs(1);
                    match store::global().and_then(|db| db.live_settings_with_secrets()) {
                        Ok(values) if values != last_settings => {
                            match WatcherConfig::from_values(&backend_dir, &values) {
                                Ok(new_config) => {
                                    let auth_changed = new_config.soop_username != config.soop_username
                                        || new_config.soop_password != config.soop_password;
                                    config = new_config;
                                    last_settings = values;
                                    logs.push("[RUST] SQLite settings hot reload applied").await;
                                    if auth_changed && !config.soop_username.is_empty() && !config.soop_password.is_empty() {
                                        match session.login(&config.soop_username, &config.soop_password).await {
                                            Ok(login) => logs.push(format!("[RUST:AUTH] SOOP login refreshed : {login}")).await,
                                            Err(err) => logs.push(format!("[RUST:WARN] SOOP login refresh failed: {err:#}")).await,
                                        }
                                    }
                                }
                                Err(err) => logs.push(format!("[RUST:WARN] SQLite settings reload rejected; previous values kept: {err:#}")).await,
                            }
                        }
                        Ok(_) => {}
                        Err(err) => logs.push(format!("[RUST:WARN] SQLite settings read failed; previous values kept: {err:#}")).await,
                    }
                }

                if now >= next_reload {
                    next_reload = now + Duration::from_secs(config.reload_interval.max(1));
                    match store::global().and_then(|db| db.channels()) {
                        Ok(channels) => {
                            let signature = channel_signature(&channels);
                            if signature != last_channel_signature {
                                apply_channels(&mut states, channels, &recorder, &logs).await;
                                last_channel_signature = signature;
                                logs.push("[RUST] SQLite channel hot reload applied").await;
                            }
                        }
                        Err(err) => logs.push(format!("[RUST:WARN] SQLite channel reload failed; previous list kept: {err:#}")).await,
                    }
                }

                monitor_recordings(&mut states, &config, &recorder, &logs).await;
                poll_channels(&mut states, &config, &mut session, &recorder, &logs).await;
                update_snapshot(&states, &snapshot).await;
            }
        }
    }

    for state in states.values_mut() {
        stop_state_recording(state, "WATCHER EXIT", &recorder, &logs).await;
    }
    clear_all_stream_passwords();
    update_snapshot(&states, &snapshot).await;
    Ok(())
}

async fn apply_channels(
    states: &mut HashMap<String, ChannelState>,
    channels: Vec<Channel>,
    recorder: &RecorderManager,
    logs: &LogBuffer,
) {
    let mut incoming = HashMap::new();
    for channel in channels {
        incoming.insert(channel.account.to_ascii_lowercase(), channel);
    }

    let existing: Vec<String> = states.keys().cloned().collect();
    for key in existing {
        if !incoming.contains_key(&key) {
            if let Some(mut state) = states.remove(&key) {
                stop_state_recording(&mut state, "CHANNEL REMOVED", recorder, logs).await;
                logs.push(format!("[RUST] channel removed: {}", state.channel.account))
                    .await;
            }
        }
    }

    for (key, channel) in incoming {
        if let Some(state) = states.get_mut(&key) {
            let was_enabled = state.channel.enabled;
            state.channel = channel;
            if was_enabled && !state.channel.enabled {
                stop_state_recording(state, "CHANNEL DISABLED", recorder, logs).await;
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

async fn handle_command(
    command: WatcherCommand,
    states: &mut HashMap<String, ChannelState>,
    recorder: &RecorderManager,
    logs: &LogBuffer,
) {
    let (account, action) = match &command {
        WatcherCommand::StopOnce(account) => (account, "stop"),
        WatcherCommand::Resume(account) => (account, "resume"),
        WatcherCommand::Recheck(account) => (account, "recheck"),
    };
    let key = account.to_ascii_lowercase();
    let Some(state) = states.get_mut(&key) else {
        logs.push(format!(
            "[RUST:WARN] channel command target not found: {account}"
        ))
        .await;
        return;
    };

    match command {
        WatcherCommand::StopOnce(_) => {
            state.suppressed_bno = state
                .last_bno
                .clone()
                .or_else(|| state.recording.as_ref().map(|rec| rec.bno.clone()));
            stop_state_recording(state, "USER CHANNEL STOP", recorder, logs).await;
            state.status = "PAUSED".into();
        }
        WatcherCommand::Resume(_) => {
            state.suppressed_bno = None;
            state.status = "UNKNOWN".into();
            state.next_check = Instant::now();
        }
        WatcherCommand::Recheck(_) => state.next_check = Instant::now(),
    }
    logs.push(format!(
        "[RUST] channel {action}: {}",
        state.channel.account
    ))
    .await;
}

async fn poll_channels(
    states: &mut HashMap<String, ChannelState>,
    config: &WatcherConfig,
    session: &mut SoopSession,
    recorder: &RecorderManager,
    logs: &LogBuffer,
) {
    let keys: Vec<String> = states.keys().cloned().collect();
    for key in keys {
        let Some(state) = states.get_mut(&key) else {
            continue;
        };
        if !state.channel.enabled || state.recording.is_some() || Instant::now() < state.next_check
        {
            continue;
        }
        state.next_check = Instant::now() + Duration::from_secs(config.check_interval.max(1));

        match session.live_info(&state.channel.account).await {
            Ok(LiveResult::Offline) => {
                clear_stream_password(&state.channel.account);
                state.status = "OFFLINE".into();
                state.last_bno = None;
                state.detail = None;
            }
            Ok(LiveResult::AuthRequired) => {
                state.status = "AUTH".into();
                match session
                    .login(&config.soop_username, &config.soop_password)
                    .await
                {
                    Ok(_) => state.next_check = Instant::now(),
                    Err(err) => state.detail = Some(format!("login failed: {err}")),
                }
            }
            Ok(LiveResult::Live(live)) => {
                if state
                    .channel
                    .name
                    .eq_ignore_ascii_case(&state.channel.account)
                    && !live.bj_nick.is_empty()
                {
                    state.channel.name = live.bj_nick.clone();
                }
                if !is_password_protected(&live.bpwd) {
                    clear_stream_password(&state.channel.account);
                } else if stream_password_for(&state.channel.account, &live.bno).is_none() {
                    state.status = "PASSWORD_REQUIRED".into();
                    state.last_bno = Some(live.bno.clone());
                    state.detail = Some("방송 비밀번호 입력이 필요합니다. 비밀번호는 현재 방송 동안 메모리에만 유지됩니다.".into());
                    continue;
                }

                if state.suppressed_bno.as_deref() == Some(live.bno.as_str()) {
                    state.status = "PAUSED".into();
                    state.last_bno = Some(live.bno);
                    continue;
                }
                if state.suppressed_bno.is_some() {
                    state.suppressed_bno = None;
                }

                match start_recording(&state.channel, &live, config, session, recorder, logs).await
                {
                    Ok(recording) => {
                        state.last_bno = Some(live.bno);
                        state.status = "RECORDING".into();
                        state.detail = None;
                        state.recording = Some(recording);
                    }
                    Err(err) => {
                        state.next_check =
                            Instant::now() + Duration::from_secs(config.retry_interval.max(1));
                        if is_password_protected(&live.bpwd) {
                            clear_stream_password(&state.channel.account);
                            state.status = "PASSWORD_REQUIRED".into();
                            state.last_bno = Some(live.bno.clone());
                            state.detail = Some("방송 비밀번호가 올바르지 않거나 보호 스트림 확인에 실패했습니다. 다시 입력하세요.".into());
                            logs.push(format!("[RUST:WARN] protected stream resolve failed {}; password cleared: {err:#}", state.channel.account)).await;
                        } else {
                            state.status = "ERROR".into();
                            state.detail = Some(err.to_string());
                            logs.push(format!(
                                "[RUST:ERR] record start failed {}: {err:#}",
                                state.channel.account
                            ))
                            .await;
                        }
                    }
                }
            }
            Err(err) => {
                state.status = "ERROR".into();
                state.detail = Some(err.to_string());
                logs.push(format!(
                    "[RUST:WARN] live check failed {}: {err:#}",
                    state.channel.account
                ))
                .await;
            }
        }
    }
}

async fn monitor_recordings(
    states: &mut HashMap<String, ChannelState>,
    config: &WatcherConfig,
    recorder: &RecorderManager,
    logs: &LogBuffer,
) {
    let keys: Vec<String> = states.keys().cloned().collect();
    for key in keys {
        let Some(state) = states.get_mut(&key) else {
            continue;
        };
        let poll = match state.recording.as_mut() {
            Some(rec) => recorder.poll(rec, &config.recorder),
            None => continue,
        };

        match poll {
            Ok(RecordingPoll::Running) => {}
            Ok(RecordingPoll::Exited(code)) => {
                if let Some(rec) = state.recording.take() {
                    let reason = if code.unwrap_or(0) == 0 {
                        "NORMAL".to_string()
                    } else {
                        format!(
                            "RECORDER EXIT CODE={}",
                            code.map(|v| v.to_string())
                                .unwrap_or_else(|| "unknown".into())
                        )
                    };
                    recorder
                        .log_finished(&state.channel.name, &state.channel.account, &rec, &reason)
                        .await;
                }
                state.status = "UNKNOWN".into();
                state.next_check = Instant::now();
            }
            Ok(RecordingPoll::LowDisk(free)) => {
                state.status = "LOW_DISK".into();
                state.detail = Some(format!(
                    "free {free:.2} GB / limit {:.2} GB",
                    config.recorder.min_free_space_gb
                ));
                stop_state_recording(state, "LOW DISK SPACE", recorder, logs).await;
                state.next_check = Instant::now() + Duration::from_secs(30);
            }
            Ok(RecordingPoll::Stalled) => {
                state.status = "STALLED".into();
                state.detail = Some(format!("{}s no growth", config.recorder.stall_timeout));
                stop_state_recording(state, "RECORD STALLED", recorder, logs).await;
                state.next_check = Instant::now();
            }
            Err(err) => {
                state.status = "ERROR".into();
                state.detail = Some(format!("recorder monitor error: {err}"));
                logs.push(format!(
                    "[RUST:WARN] recorder monitor failed {}: {err:#}",
                    state.channel.account
                ))
                .await;
            }
        }
    }
}

async fn start_recording(
    channel: &Channel,
    live: &LiveInfo,
    config: &WatcherConfig,
    session: &mut SoopSession,
    recorder: &RecorderManager,
    logs: &LogBuffer,
) -> Result<Recording> {
    let stream = session.worker_playlist(channel, live, config).await?;
    let output_dir = channel_output_dir(channel, &config.output_dir)?;
    let output_file = unique_output_file(
        &output_dir,
        &channel.name,
        &live.title,
        &config.file_name_pattern,
    )?;
    let stream_url = if stream
        .playlist_url
        .to_ascii_lowercase()
        .starts_with("hls://")
    {
        stream.playlist_url.clone()
    } else {
        format!("hls://{}", stream.playlist_url)
    };

    logs.push(format!(
        "[RUST] stream resolved account={} hls={} cdn={} host={}",
        channel.account, stream.quality, stream.cdn, stream.host
    ))
    .await;

    recorder
        .start(
            &config.recorder,
            &stream_url,
            output_file,
            live.bno.clone(),
            live.title.clone(),
            &channel.name,
            &channel.account,
        )
        .await
}

async fn stop_state_recording(
    state: &mut ChannelState,
    reason: &str,
    recorder: &RecorderManager,
    logs: &LogBuffer,
) {
    let Some(mut rec) = state.recording.take() else {
        return;
    };
    match recorder.stop(&mut rec).await {
        Ok(_) => {
            recorder
                .log_finished(&state.channel.name, &state.channel.account, &rec, reason)
                .await
        }
        Err(err) => {
            state.detail = Some(format!("stop failed: {err}"));
            logs.push(format!(
                "[RUST:ERR] recorder stop failed {} pid={}: {err:#}",
                state.channel.account, rec.pid
            ))
            .await;
        }
    }
}

async fn update_snapshot(
    states: &HashMap<String, ChannelState>,
    snapshot: &Arc<RwLock<WatcherStatus>>,
) {
    let mut ordered: Vec<&ChannelState> = states.values().collect();
    ordered.sort_by(|a, b| a.channel.name.cmp(&b.channel.name));

    let mut recording_count = 0;
    let mut offline_count = 0;
    let mut error_count = 0;
    let mut channels = Vec::with_capacity(ordered.len());

    for state in ordered {
        if state.status == "RECORDING" {
            recording_count += 1;
        }
        if state.status == "OFFLINE" {
            offline_count += 1;
        }
        if matches!(state.status.as_str(), "ERROR" | "LOW_DISK" | "STALLED") {
            error_count += 1;
        }
        let rec = state.recording.as_ref();
        channels.push(ChannelRuntimeStatus {
            account: state.channel.account.clone(),
            name: state.channel.name.clone(),
            status: if !state.channel.enabled {
                "DISABLED".into()
            } else {
                state.status.clone()
            },
            bno: rec
                .map(|r| r.bno.clone())
                .or_else(|| state.last_bno.clone()),
            title: rec.map(|r| r.title.clone()),
            file: rec.map(|r| r.file.display().to_string()),
            size_bytes: rec
                .and_then(|r| fs::metadata(&r.file).ok().map(|m| m.len()))
                .unwrap_or(0),
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
enum LiveResult {
    Offline,
    AuthRequired,
    Live(LiveInfo),
}

struct SoopSession {
    client: Client,
    cookies: BTreeMap<String, String>,
    bno_regex: Regex,
}

impl SoopSession {
    fn new(client: Client) -> Self {
        Self {
            client,
            cookies: BTreeMap::new(),
            bno_regex: Regex::new(r"window\.nBroadNo\s*=\s*(\d+);").unwrap(),
        }
    }

    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn worker_cookie_header(&self) -> String {
        const ALLOW: &[&str] = &[
            "AuthTicket",
            "BbsTicket",
            "UserTicket",
            "BbsSaveTicket",
            "RDB",
            "PdboxTicket",
            "PdboxBbs",
            "PdboxUser",
            "PdboxSaveTicket",
        ];
        self.cookies
            .iter()
            .filter(|(key, _)| ALLOW.iter().any(|item| key.eq_ignore_ascii_case(item)))
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    async fn login(&mut self, username: &str, password: &str) -> Result<String> {
        if username.trim().is_empty() || password.trim().is_empty() {
            bail!("SOOP username/password is empty");
        }
        self.cookies.clear();
        let response = self
            .client
            .post("https://login.sooplive.com/app/LoginAction.php")
            .header("Referer", "https://www.sooplive.com/")
            .form(&[
                ("szWork", "login"),
                ("szType", "json"),
                ("szUid", username),
                ("szPassword", password),
                ("isSaveId", "true"),
                ("isSavePw", "false"),
                ("isSaveJoin", "false"),
                ("isLoginRetain", "Y"),
            ])
            .send()
            .await?;
        collect_cookies(&mut self.cookies, &response);
        let value: Value = response.error_for_status()?.json().await?;
        if value.get("RESULT").and_then(Value::as_i64) != Some(1) {
            bail!("SOOP login failed RESULT={:?}", value.get("RESULT"));
        }

        let response = self
            .client
            .get("https://afevent2.sooplive.com/api/get_private_info.php")
            .header("Referer", "https://www.sooplive.com/")
            .header(COOKIE, self.cookie_header())
            .send()
            .await?;
        collect_cookies(&mut self.cookies, &response);
        let auth: Value = response.error_for_status()?.json().await?;
        let login_id = auth
            .pointer("/CHANNEL/LOGIN_ID")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if login_id.is_empty() {
            bail!("SOOP login verification failed");
        }
        Ok(login_id)
    }

    async fn live_info(&self, account: &str) -> Result<LiveResult> {
        let channel_url = format!("https://play.sooplive.com/{account}");
        let cookie = self.cookie_header();
        let mut request = self
            .client
            .get(&channel_url)
            .header("Referer", "https://play.sooplive.com/");
        if !cookie.is_empty() {
            request = request.header(COOKIE, cookie.clone());
        }
        let html = request.send().await?.error_for_status()?.text().await?;
        let Some(captures) = self.bno_regex.captures(&html) else {
            return Ok(LiveResult::Offline);
        };
        let bno = captures.get(1).unwrap().as_str().to_string();

        let mut request = self
            .client
            .post("https://live.sooplive.com/afreeca/player_live_api.php")
            .header("Referer", &channel_url)
            .form(&[
                ("from_api", "0"),
                ("mode", "landing"),
                ("player_type", "html5"),
                ("stream_type", "common"),
                ("type", "live"),
                ("bid", account),
                ("bno", bno.as_str()),
                ("pwd", ""),
            ]);
        if !cookie.is_empty() {
            request = request.header(COOKIE, cookie);
        }
        let value: Value = request.send().await?.error_for_status()?.json().await?;
        let Some(channel) = value.get("CHANNEL") else {
            return Ok(LiveResult::Offline);
        };
        if channel.get("RESULT").and_then(Value::as_i64) == Some(-6) {
            return Ok(LiveResult::AuthRequired);
        }
        if channel.get("RESULT").and_then(Value::as_i64) != Some(1) {
            return Ok(LiveResult::Offline);
        }

        let get = |key: &str| {
            channel
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let rmd = get("RMD");
        let api_bno = get("BNO");
        if rmd.is_empty() || api_bno.is_empty() {
            return Ok(LiveResult::Offline);
        }
        Ok(LiveResult::Live(LiveInfo {
            bno: api_bno,
            bj_nick: get("BJNICK"),
            title: get("TITLE"),
            rmd,
            bpwd: get("BPWD"),
        }))
    }

    async fn worker_playlist(
        &self,
        channel: &Channel,
        live: &LiveInfo,
        config: &WatcherConfig,
    ) -> Result<StreamInfo> {
        let attempts = config.worker_max_retry.max(1);
        let stream_password = stream_password_for(&channel.account, &live.bno).unwrap_or_default();
        let mut last_error = String::new();
        for attempt in 1..=attempts {
            let cookies = self.worker_cookie_header();
            let body = json!({
                "account": channel.account,
                "bno": live.bno,
                "rmd": live.rmd,
                "quality": "master",
                "cq": "sd",
                "password": stream_password.as_str(),
                "cookie": cookies,
                "bid": channel.account,
                "bpwd": stream_password.as_str(),
                "channel_url": format!("https://play.sooplive.com/{}", channel.account),
                "soop_cookie_header": self.worker_cookie_header()
            });
            match self
                .client
                .post(&config.worker_url)
                .header("X-API-Key", &config.worker_api_key)
                .json(&body)
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    let text = response.text().await.unwrap_or_default();
                    if status.is_success() {
                        if let Ok(value) = serde_json::from_str::<Value>(&text) {
                            if value.get("success").and_then(Value::as_bool) == Some(true) {
                                let playlist_url = value
                                    .get("playlist_url")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string();
                                if !playlist_url.is_empty() {
                                    return Ok(StreamInfo {
                                        quality: value
                                            .get("quality")
                                            .and_then(Value::as_str)
                                            .unwrap_or("master")
                                            .to_string(),
                                        cdn: value
                                            .get("cdn")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .to_string(),
                                        host: value
                                            .get("host")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .to_string(),
                                        playlist_url,
                                    });
                                }
                            }
                        }
                    }
                    last_error = format!("Worker HTTP {status}: {}", compact(&text, 240));
                }
                Err(err) => last_error = format!("Worker transport error: {err}"),
            }
            if attempt < attempts {
                let delay = [2, 5, 10][(attempt - 1).min(2)];
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        }
        bail!("{last_error}")
    }
}

fn collect_cookies(target: &mut BTreeMap<String, String>, response: &Response) {
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
    fn from_values(backend_dir: &Path, map: &BTreeMap<String, String>) -> Result<Self> {
        let raw = |name: &str| map.get(name).cloned().unwrap_or_default();
        let worker_api_key = unprotect_secret(&raw("CLOUDFLARE_API_KEY"), "CLOUDFLARE_API_KEY")?;
        let soop_password = unprotect_secret(&raw("SOOP_PASSWORD"), "SOOP_PASSWORD")?;
        let worker_url = raw("CLOUDFLARE_WORKER_URL");
        if !worker_url.starts_with("https://") {
            bail!("CLOUDFLARE_WORKER_URL must be https://");
        }
        if worker_api_key.is_empty() {
            bail!("CLOUDFLARE_API_KEY is empty");
        }

        let output_dir = PathBuf::from(
            map.get("OUTPUT_DIR")
                .cloned()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(default_output_dir),
        );
        fs::create_dir_all(&output_dir)?;
        let streamlink = resolve_streamlink(backend_dir, map)?;

        Ok(Self {
            check_interval: int(map, "CHECK_INTERVAL", 30),
            reload_interval: int(map, "CHANNEL_RELOAD_INTERVAL", 2),
            retry_interval: int(map, "RECORD_RETRY_INTERVAL", 5),
            worker_max_retry: int(map, "WORKER_MAX_RETRY", 3) as usize,
            output_dir,
            file_name_pattern: map
                .get("FILE_NAME_PATTERN")
                .cloned()
                .unwrap_or_else(|| "LEGACY".into())
                .to_ascii_uppercase(),
            worker_url,
            worker_api_key,
            soop_username: raw("SOOP_USERNAME"),
            soop_password,
            recorder: RecorderConfig {
                streamlink,
                quality: map
                    .get("QUALITY")
                    .cloned()
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| "best".into()),
                stall_timeout: int(map, "RECORD_STALL_TIMEOUT", 90),
                monitor_interval: int(map, "RECORD_MONITOR_INTERVAL", 5),
                min_free_space_gb: map
                    .get("MIN_FREE_SPACE_GB")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(20.0),
            },
        })
    }
}

fn resolve_streamlink(backend_dir: &Path, map: &BTreeMap<String, String>) -> Result<PathBuf> {
    for key in ["STREAMLINK_PATH", "STREAMLINK_FALLBACK"] {
        if let Some(value) = map.get(key) {
            if !value.trim().is_empty() && !value.eq_ignore_ascii_case("AUTO") {
                let path = PathBuf::from(value);
                if path.is_file() {
                    return Ok(path);
                }
            }
        }
    }

    let mut candidates = vec![backend_dir.join("streamlink.exe")];
    #[cfg(windows)]
    {
        candidates.push(PathBuf::from(
            r"C:\Program Files\Streamlink\bin\streamlink.exe",
        ));
        candidates.push(PathBuf::from(r"C:\Program Files\Streamlink\streamlink.exe"));
    }
    for path in candidates {
        if path.is_file() {
            return Ok(path);
        }
    }
    bail!("streamlink executable not found; set STREAMLINK_PATH or STREAMLINK_FALLBACK")
}

fn channel_output_dir(channel: &Channel, default: &Path) -> Result<PathBuf> {
    let base = if channel.outdir.trim().is_empty() {
        default.to_path_buf()
    } else {
        PathBuf::from(channel.outdir.trim())
    };
    let dir = base.join(safe_name(&channel.name, 80));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn unique_output_file(dir: &Path, channel: &str, title: &str, pattern: &str) -> Result<PathBuf> {
    let now = Local::now();
    let date = now.format("%y%m%d").to_string();
    let time = now.format("%H%M%S").to_string();
    let channel = safe_name(channel, 60);
    let title = safe_name(title, 90);

    let base = match pattern {
        "TIME_TITLE" => format!("{date}_{time}_{title}_{channel}"),
        "BJ_TITLE" => format!("{date}_{channel}_{title}"),
        "TITLE_NUMBER" => {
            for number in 1..=9999 {
                let path = dir.join(format!("{date}_{title}_{number:02}_{channel}.ts"));
                if !path.exists() {
                    return Ok(path);
                }
            }
            bail!("too many TITLE_NUMBER filename collisions")
        }
        _ => format!("{date}_{time}_{channel}"),
    };

    let mut path = dir.join(format!("{base}.ts"));
    for number in 2..=9999 {
        if !path.exists() {
            return Ok(path);
        }
        path = dir.join(format!("{base}_{number:02}.ts"));
    }
    bail!("too many output filename collisions")
}

fn safe_name(value: &str, max: usize) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() {
            continue;
        }
        if r#"\/:*?"<>|"#.contains(ch) {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let mut out = out
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .trim_end_matches('.')
        .to_string();
    if out.is_empty() {
        out = "UNKNOWN".into();
    }
    if out.chars().count() > max {
        out = out
            .chars()
            .take(max)
            .collect::<String>()
            .trim()
            .trim_end_matches('.')
            .to_string();
    }
    let upper = out.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        out.insert(0, '_');
    }
    out
}

fn int(map: &BTreeMap<String, String>, key: &str, default: u64) -> u64 {
    map.get(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn channel_signature(channels: &[Channel]) -> String {
    channels
        .iter()
        .map(|channel| {
            format!(
                "{}|{}|{}|{}",
                if channel.enabled { "Y" } else { "N" },
                channel.name,
                channel.account.to_ascii_lowercase(),
                channel.outdir
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn compact(text: &str, max: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() > max {
        format!("{}...", compact.chars().take(max).collect::<String>())
    } else {
        compact
    }
}

fn default_output_dir() -> String {
    if cfg!(windows) {
        r"C:\SOOP_LIVE".into()
    } else {
        "./SOOP_LIVE".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_filename_replaces_windows_reserved_chars() {
        assert_eq!(safe_name("a:b/c*?d", 80), "a_b_c__d");
        assert_eq!(safe_name("CON", 80), "_CON");
    }

    #[test]
    fn channel_signature_is_case_insensitive_for_accounts() {
        let a = vec![Channel {
            enabled: true,
            name: "A".into(),
            account: "UserA".into(),
            outdir: "".into(),
        }];
        let b = vec![Channel {
            enabled: true,
            name: "A".into(),
            account: "usera".into(),
            outdir: "".into(),
        }];
        assert_eq!(channel_signature(&a), channel_signature(&b));
    }
}
