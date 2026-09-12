use crate::{
    backend::LogBuffer,
    model::{Channel, ChannelRuntimeStatus, NativeWatcherStatus as WatcherStatus},
    recorder::{RecorderConfig, RecorderManager, Recording, RecordingPoll},
    security::unprotect_secret,
    store,
    support::platform::{
        PlatformId,
        live::{
            LiveBroadcast, LiveProbe, LiveSession, StreamResolveConfig, ensure_supported, session_for,
        },
    },
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{Local, Utc};
use reqwest::Client;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
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
struct StreamPassword {
    broadcast_id: String,
    value: String,
}

static STREAM_PASSWORDS: OnceLock<StdMutex<HashMap<String, StreamPassword>>> = OnceLock::new();

fn stream_passwords() -> &'static StdMutex<HashMap<String, StreamPassword>> {
    STREAM_PASSWORDS.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn channel_key(platform: PlatformId, account: &str) -> String {
    format!("{}:{}", platform.as_str(), account.trim().to_ascii_lowercase())
}

fn stream_password_for(platform: PlatformId, account: &str, broadcast_id: &str) -> Option<String> {
    let key = channel_key(platform, account);
    let Ok(mut passwords) = stream_passwords().lock() else {
        return None;
    };
    match passwords.get(&key) {
        Some(item) if item.broadcast_id == broadcast_id => Some(item.value.clone()),
        Some(_) => {
            passwords.remove(&key);
            None
        }
        None => None,
    }
}

fn clear_stream_password(platform: PlatformId, account: &str) {
    if let Ok(mut passwords) = stream_passwords().lock() {
        passwords.remove(&channel_key(platform, account));
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
    last_broadcast_id: Option<String>,
    suppressed_broadcast_id: Option<String>,
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
            last_broadcast_id: None,
            suppressed_broadcast_id: None,
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
        if let Some(task) = runtime.task.take() {
            finish_watcher_task(task, &self.logs, &self.snapshot, "before restart").await;
        }
        runtime.stop_tx = None;
        runtime.command_tx = None;

        let db = store::global()?;
        let settings = db.live_settings_with_secrets()?;
        let channels = db.channels()?;
        for channel in &channels {
            ensure_supported(channel.platform)?;
        }
        let config = WatcherConfig::from_values(
            &self.backend_dir,
            &settings,
            channels_require_soop(&channels),
        )?;
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
                engine: "rust-native-v5-multiplatform",
                started_at: Some(Utc::now().to_rfc3339()),
                channel_count: channels.len(),
                recording_count: 0,
                offline_count: 0,
                error_count: 0,
                channels: channels
                    .iter()
                    .map(|c| ChannelRuntimeStatus {
                        platform: c.platform,
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
            .push("[RUST] native watcher v5 started (platform LIVE adapters)")
            .await;
        Ok(self.snapshot.read().await.clone())
    }

    pub async fn stop(&self) -> Result<WatcherStatus> {
        let mut runtime = self.runtime.lock().await;
        if let Some(tx) = runtime.stop_tx.take() {
            let _ = tx.send(());
        }
        runtime.command_tx = None;
        if let Some(task) = runtime.task.take() {
            finish_watcher_task(task, &self.logs, &self.snapshot, "stop").await;
        }
        self.snapshot.write().await.running = false;
        self.logs.push("[RUST] native watcher v5 stopped").await;
        Ok(self.snapshot.read().await.clone())
    }

    pub async fn status(&self) -> Result<WatcherStatus> {
        let mut runtime = self.runtime.lock().await;
        if runtime
            .task
            .as_ref()
            .is_some_and(|task| task.is_finished())
        {
            if let Some(task) = runtime.task.take() {
                finish_watcher_task(task, &self.logs, &self.snapshot, "status reap").await;
            }
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
        let (platform, broadcast_id) = {
            let snapshot = self.snapshot.read().await;
            let mut matches = snapshot
                .channels
                .iter()
                .filter(|channel| channel.account.eq_ignore_ascii_case(&account));
            let channel = matches
                .next()
                .ok_or_else(|| anyhow!("channel not found: {account}"))?;
            if matches.next().is_some() {
                bail!("channel account is ambiguous across platforms: {account}");
            }
            if channel.status != "PASSWORD_REQUIRED" {
                bail!("channel is not waiting for a stream password");
            }
            (
                channel.platform,
                channel
                    .bno
                    .clone()
                    .ok_or_else(|| anyhow!("protected broadcast number is unavailable"))?,
            )
        };
        {
            let mut passwords = stream_passwords()
                .lock()
                .map_err(|_| anyhow!("stream password memory store is unavailable"))?;
            passwords.insert(
                channel_key(platform, &account),
                StreamPassword {
                    broadcast_id,
                    value: password,
                },
            );
        }
        self.logs
            .push(format!(
                "[RUST:AUTH] stream password supplied platform={platform} account={account} (memory only)"
            ))
            .await;
        self.channel_action(account, "recheck").await
    }
}

async fn finish_watcher_task(
    task: JoinHandle<()>,
    logs: &LogBuffer,
    snapshot: &Arc<RwLock<WatcherStatus>>,
    context: &str,
) {
    if let Err(err) = task.await {
        logs.push(format!(
            "[RUST:ERR] watcher task terminated unexpectedly ({context}): {err}"
        ))
        .await;
        let mut state = snapshot.write().await;
        state.running = false;
        state.recording_count = 0;
        state.error_count = state.error_count.saturating_add(1);
        for channel in &mut state.channels {
            if channel.status == "RECORDING" {
                channel.status = "ERROR".into();
                channel.detail = Some(
                    "watcher task terminated unexpectedly; recorder child was terminated".into(),
                );
            }
        }
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
    let mut sessions = create_sessions(&initial_channels, &client)?;
    refresh_soop_login(&mut sessions, &config, &logs, "initial").await;
    let recorder = RecorderManager::new(logs.clone());

    clear_all_stream_passwords();
    let mut last_channel_signature = channel_signature(&initial_channels);
    let mut states = HashMap::new();
    apply_channels(&mut states, initial_channels, &recorder, &logs).await;
    let mut next_reload = Instant::now();
    let mut next_setting_check = Instant::now();

    logs.push(format!(
        "[RUST] watcher ready | source=sqlite check={}s reload={}s output={} streamlink={} provider_sessions={}",
        config.check_interval,
        config.reload_interval,
        config.output_dir.display(),
        config.recorder.streamlink.display(),
        sessions.len()
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
                            let require_soop = states.values().any(|state| {
                                state.channel.enabled && state.channel.platform == PlatformId::Soop
                            });
                            match WatcherConfig::from_values(&backend_dir, &values, require_soop) {
                                Ok(new_config) => {
                                    let auth_changed = new_config.soop_username != config.soop_username
                                        || new_config.soop_password != config.soop_password;
                                    config = new_config;
                                    last_settings = values;
                                    logs.push("[RUST] SQLite settings hot reload applied").await;
                                    if auth_changed && require_soop {
                                        refresh_soop_login(&mut sessions, &config, &logs, "refresh").await;
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
                                let require_soop = channels_require_soop(&channels);
                                match WatcherConfig::from_values(&backend_dir, &last_settings, require_soop) {
                                    Err(err) => logs.push(format!("[RUST:WARN] channel reload rejected by platform configuration: {err:#}")).await,
                                    Ok(new_config) => {
                                        if let Err(err) = ensure_sessions(&mut sessions, &channels, &client) {
                                            logs.push(format!("[RUST:WARN] channel reload contains unsupported platform: {err:#}")).await;
                                        } else {
                                            config = new_config;
                                            if require_soop {
                                                refresh_soop_login(&mut sessions, &config, &logs, "channel reload").await;
                                            }
                                            apply_channels(&mut states, channels, &recorder, &logs).await;
                                            last_channel_signature = signature;
                                            logs.push("[RUST] SQLite channel hot reload applied").await;
                                        }
                                    }
                                }
                            }
                        }
                        Err(err) => logs.push(format!("[RUST:WARN] SQLite channel reload failed; previous list kept: {err:#}")).await,
                    }
                }

                check_recording_broadcasts(&mut states, &config, &mut sessions, &recorder, &logs).await;
                monitor_recordings(&mut states, &config, &recorder, &logs).await;
                poll_channels(&mut states, &config, &mut sessions, &recorder, &logs).await;
                update_snapshot(&states, &snapshot).await;
            }
        }
    }

    for state in states.values_mut() {
        stop_state_recording(state, "WATCHER EXIT", &recorder, &logs).await;
    }
    mark_watcher_stopped(&mut states);
    clear_all_stream_passwords();
    update_snapshot(&states, &snapshot).await;
    Ok(())
}

fn create_sessions(channels: &[Channel], client: &Client) -> Result<HashMap<PlatformId, LiveSession>> {
    let mut sessions = HashMap::new();
    ensure_sessions(&mut sessions, channels, client)?;
    Ok(sessions)
}

fn ensure_sessions(
    sessions: &mut HashMap<PlatformId, LiveSession>,
    channels: &[Channel],
    client: &Client,
) -> Result<()> {
    let platforms = channels.iter().map(|c| c.platform).collect::<HashSet<_>>();
    for platform in platforms {
        ensure_supported(platform)?;
        if let std::collections::hash_map::Entry::Vacant(entry) = sessions.entry(platform) {
            entry.insert(LiveSession::new(platform, client.clone())?);
        }
    }
    Ok(())
}

fn channels_require_soop(channels: &[Channel]) -> bool {
    channels
        .iter()
        .any(|channel| channel.enabled && channel.platform == PlatformId::Soop)
}

async fn refresh_soop_login(
    sessions: &mut HashMap<PlatformId, LiveSession>,
    config: &WatcherConfig,
    logs: &LogBuffer,
    context: &str,
) {
    if config.soop_username.is_empty() || config.soop_password.is_empty() {
        return;
    }
    let Ok(session) = session_for(sessions, PlatformId::Soop) else {
        return;
    };
    match session
        .login(&config.soop_username, &config.soop_password)
        .await
    {
        Ok(login) => {
            logs.push(format!(
                "[RUST:AUTH] SOOP login {context} OK : {login}"
            ))
            .await
        }
        Err(err) => {
            logs.push(format!(
                "[RUST:WARN] SOOP login {context} failed; continuing: {err:#}"
            ))
            .await
        }
    }
}

fn mark_watcher_stopped(states: &mut HashMap<String, ChannelState>) {
    for state in states.values_mut() {
        if state.channel.enabled {
            state.status = "WATCHER_STOPPED".into();
            state.last_broadcast_id = None;
            state.suppressed_broadcast_id = None;
            state.detail = None;
        } else {
            state.status = "DISABLED".into();
        }
    }
}

async fn apply_channels(
    states: &mut HashMap<String, ChannelState>,
    channels: Vec<Channel>,
    recorder: &RecorderManager,
    logs: &LogBuffer,
) {
    let mut incoming = HashMap::new();
    for channel in channels {
        incoming.insert(channel_key(channel.platform, &channel.account), channel);
    }

    let existing: Vec<String> = states.keys().cloned().collect();
    for key in existing {
        if !incoming.contains_key(&key) {
            if let Some(mut state) = states.remove(&key) {
                stop_state_recording(&mut state, "CHANNEL REMOVED", recorder, logs).await;
                logs.push(format!(
                    "[RUST] channel removed: {}/{}",
                    state.channel.platform, state.channel.account
                ))
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

fn command_state_key(states: &HashMap<String, ChannelState>, account: &str) -> Result<String> {
    let matches = states
        .iter()
        .filter(|(_, state)| state.channel.account.eq_ignore_ascii_case(account))
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [key] => Ok(key.clone()),
        [] => bail!("channel command target not found: {account}"),
        _ => bail!("channel account is ambiguous across platforms: {account}"),
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
    let key = match command_state_key(states, account) {
        Ok(key) => key,
        Err(err) => {
            logs.push(format!("[RUST:WARN] {err}")).await;
            return;
        }
    };
    let Some(state) = states.get_mut(&key) else {
        return;
    };

    match command {
        WatcherCommand::StopOnce(_) => {
            state.suppressed_broadcast_id = state
                .last_broadcast_id
                .clone()
                .or_else(|| state.recording.as_ref().map(|rec| rec.bno.clone()));
            stop_state_recording(state, "USER CHANNEL STOP", recorder, logs).await;
            state.status = "PAUSED".into();
        }
        WatcherCommand::Resume(_) => {
            state.suppressed_broadcast_id = None;
            state.status = "UNKNOWN".into();
            state.next_check = Instant::now();
        }
        WatcherCommand::Recheck(_) => state.next_check = Instant::now(),
    }
    logs.push(format!(
        "[RUST] channel {action}: {}/{}",
        state.channel.platform, state.channel.account
    ))
    .await;
}

async fn login_for_auth_required(
    platform: PlatformId,
    session: &mut LiveSession,
    config: &WatcherConfig,
) -> Result<String> {
    match platform {
        PlatformId::Soop => session
            .login(&config.soop_username, &config.soop_password)
            .await,
        PlatformId::Chzzk => bail!(
            "CHZZK 제한 방송 인증은 NID_AUT/NID_SES 설정이 필요합니다."
        ),
    }
}

async fn poll_channels(
    states: &mut HashMap<String, ChannelState>,
    config: &WatcherConfig,
    sessions: &mut HashMap<PlatformId, LiveSession>,
    recorder: &RecorderManager,
    logs: &LogBuffer,
) {
    let keys: Vec<String> = states.keys().cloned().collect();
    for key in keys {
        let Some(state) = states.get_mut(&key) else {
            continue;
        };
        if !state.channel.enabled
            || state.recording.is_some()
            || Instant::now() < state.next_check
        {
            continue;
        }
        state.next_check = Instant::now() + Duration::from_secs(config.check_interval.max(1));

        let platform = state.channel.platform;
        let session = match session_for(sessions, platform) {
            Ok(session) => session,
            Err(err) => {
                state.status = "ERROR".into();
                state.detail = Some(err.to_string());
                continue;
            }
        };
        match session.probe(&state.channel.account).await {
            Ok(LiveProbe::Offline) => {
                clear_stream_password(platform, &state.channel.account);
                state.status = "OFFLINE".into();
                state.last_broadcast_id = None;
                state.detail = None;
            }
            Ok(LiveProbe::AuthRequired) => {
                state.status = "AUTH".into();
                match login_for_auth_required(platform, session, config).await {
                    Ok(_) => state.next_check = Instant::now(),
                    Err(err) => state.detail = Some(format!("login failed: {err}")),
                }
            }
            Ok(LiveProbe::Live(live)) => {
                if state
                    .channel
                    .name
                    .eq_ignore_ascii_case(&state.channel.account)
                    && !live.channel_name.is_empty()
                {
                    state.channel.name = live.channel_name.clone();
                }
                if !live.password_required {
                    clear_stream_password(platform, &state.channel.account);
                } else if stream_password_for(platform, &state.channel.account, &live.id).is_none() {
                    state.status = "PASSWORD_REQUIRED".into();
                    state.last_broadcast_id = Some(live.id.clone());
                    state.detail = Some("방송 비밀번호 입력이 필요합니다. 비밀번호는 현재 방송 동안 메모리에만 유지됩니다.".into());
                    continue;
                }

                if state.suppressed_broadcast_id.as_deref() == Some(live.id.as_str()) {
                    state.status = "PAUSED".into();
                    state.last_broadcast_id = Some(live.id);
                    continue;
                }
                if state.suppressed_broadcast_id.is_some() {
                    state.suppressed_broadcast_id = None;
                }

                match start_recording(&state.channel, &live, config, session, recorder, logs).await {
                    Ok(recording) => {
                        state.last_broadcast_id = Some(live.id);
                        state.status = "RECORDING".into();
                        state.detail = None;
                        state.recording = Some(recording);
                    }
                    Err(err) => {
                        state.next_check =
                            Instant::now() + Duration::from_secs(config.retry_interval.max(1));
                        if live.password_required {
                            clear_stream_password(platform, &state.channel.account);
                            state.status = "PASSWORD_REQUIRED".into();
                            state.last_broadcast_id = Some(live.id.clone());
                            state.detail = Some("방송 비밀번호가 올바르지 않거나 보호 스트림 확인에 실패했습니다. 다시 입력하세요.".into());
                            logs.push(format!("[RUST:WARN] protected stream resolve failed {}/{}; password cleared: {err:#}", platform, state.channel.account)).await;
                        } else {
                            state.status = "ERROR".into();
                            state.detail = Some(err.to_string());
                            logs.push(format!(
                                "[RUST:ERR] record start failed {}/{}: {err:#}",
                                platform, state.channel.account
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
                    "[RUST:WARN] live check failed {}/{}: {err:#}",
                    platform, state.channel.account
                ))
                .await;
            }
        }
    }
}

async fn check_recording_broadcasts(
    states: &mut HashMap<String, ChannelState>,
    config: &WatcherConfig,
    sessions: &mut HashMap<PlatformId, LiveSession>,
    recorder: &RecorderManager,
    logs: &LogBuffer,
) {
    let keys: Vec<String> = states.keys().cloned().collect();
    for key in keys {
        let Some(state) = states.get_mut(&key) else {
            continue;
        };
        let Some(recording_id) = state.recording.as_ref().map(|rec| rec.bno.clone()) else {
            continue;
        };
        if Instant::now() < state.next_check {
            continue;
        }
        state.next_check = Instant::now() + Duration::from_secs(config.check_interval.max(1));

        let platform = state.channel.platform;
        let session = match session_for(sessions, platform) {
            Ok(session) => session,
            Err(err) => {
                logs.push(format!(
                    "[RUST:WARN] LIVE session missing while recording {}/{}: {err:#}",
                    platform, state.channel.account
                ))
                .await;
                continue;
            }
        };
        match session.probe(&state.channel.account).await {
            Ok(LiveProbe::Offline) => {
                clear_stream_password(platform, &state.channel.account);
                stop_state_recording(state, "BROADCAST ENDED", recorder, logs).await;
                state.status = "OFFLINE".into();
                state.last_broadcast_id = None;
                state.detail = None;
                logs.push(format!(
                    "[RUST] broadcast ended platform={platform} account={}",
                    state.channel.account
                ))
                .await;
            }
            Ok(LiveProbe::Live(live)) if live.id != recording_id => {
                clear_stream_password(platform, &state.channel.account);
                stop_state_recording(state, "BROADCAST CHANGED", recorder, logs).await;
                state.status = "UNKNOWN".into();
                state.last_broadcast_id = Some(live.id);
                state.detail = None;
                state.next_check = Instant::now();
                logs.push(format!(
                    "[RUST] broadcast id changed platform={platform} account={}; restarting discovery",
                    state.channel.account
                ))
                .await;
            }
            Ok(LiveProbe::Live(_)) => {}
            Ok(LiveProbe::AuthRequired) => {
                logs.push(format!(
                    "[RUST:WARN] live recheck requires {platform} auth while recording {}; keeping recorder running",
                    state.channel.account
                ))
                .await;
            }
            Err(err) => {
                logs.push(format!(
                    "[RUST:WARN] live recheck failed while recording {}/{}; keeping recorder running: {err:#}",
                    platform, state.channel.account
                ))
                .await;
            }
        }
    }
}

fn recording_exit_outcome(code: Option<i32>) -> (bool, String) {
    match code {
        Some(0) => (true, "NORMAL".to_string()),
        Some(code) => (false, format!("RECORDER EXIT CODE={code}")),
        None => (false, "RECORDER EXIT CODE=unknown".to_string()),
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
                let (normal_exit, reason) = recording_exit_outcome(code);
                if let Some(rec) = state.recording.take() {
                    recorder
                        .log_finished(&state.channel.name, &state.channel.account, &rec, &reason)
                        .await;
                }
                if normal_exit {
                    state.status = "UNKNOWN".into();
                    state.detail = None;
                    state.next_check = Instant::now();
                } else {
                    state.status = "ERROR".into();
                    state.detail = Some(reason.clone());
                    state.next_check =
                        Instant::now() + Duration::from_secs(config.retry_interval.max(1));
                    logs.push(format!(
                        "[RUST:ERR] recorder exited unexpectedly {}/{}: {reason}",
                        state.channel.platform, state.channel.account
                    ))
                    .await;
                }
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
                    "[RUST:WARN] recorder monitor failed {}/{}: {err:#}",
                    state.channel.platform, state.channel.account
                ))
                .await;
            }
        }
    }
}

async fn start_recording(
    channel: &Channel,
    live: &LiveBroadcast,
    config: &WatcherConfig,
    session: &LiveSession,
    recorder: &RecorderManager,
    logs: &LogBuffer,
) -> Result<Recording> {
    let password = stream_password_for(channel.platform, &channel.account, &live.id).unwrap_or_default();
    let resolve_config = StreamResolveConfig {
        worker_url: &config.worker_url,
        worker_api_key: &config.worker_api_key,
        max_retries: config.worker_max_retry,
    };
    let stream = session
        .resolve_stream(&channel.account, live, &resolve_config, &password)
        .await?;
    let output_dir = channel_output_dir(channel, &config.output_dir)?;
    let output_file = unique_output_file(
        &output_dir,
        &channel.name,
        &live.title,
        &config.file_name_pattern,
    )?;

    logs.push(format!(
        "[RUST] stream resolved platform={} account={} quality={} cdn={} host={}",
        channel.platform, channel.account, stream.quality, stream.cdn, stream.host
    ))
    .await;

    recorder
        .start(
            &config.recorder,
            channel.platform,
            &stream.input,
            output_file,
            live.id.clone(),
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
                "[RUST:ERR] recorder stop failed {}/{} pid={}: {err:#}",
                state.channel.platform, state.channel.account, rec.pid
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
    ordered.sort_by(|a, b| {
        a.channel
            .platform
            .as_str()
            .cmp(b.channel.platform.as_str())
            .then_with(|| a.channel.name.cmp(&b.channel.name))
    });

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
            platform: state.channel.platform,
            account: state.channel.account.clone(),
            name: state.channel.name.clone(),
            status: if !state.channel.enabled {
                "DISABLED".into()
            } else {
                state.status.clone()
            },
            bno: rec
                .map(|r| r.bno.clone())
                .or_else(|| state.last_broadcast_id.clone()),
            title: rec.map(|r| r.title.clone()),
            file: rec.map(|r| r.file.display().to_string()),
            size_bytes: rec
                .and_then(|r| fs::metadata(&r.file).ok().map(|m| m.len()))
                .unwrap_or(0),
            started_at: rec.map(|r| r.started_at.to_rfc3339()),
            suppressed: state.suppressed_broadcast_id.is_some(),
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

impl WatcherConfig {
    fn from_values(
        backend_dir: &Path,
        map: &BTreeMap<String, String>,
        require_soop: bool,
    ) -> Result<Self> {
        let raw = |name: &str| map.get(name).cloned().unwrap_or_default();
        let worker_url = raw("CLOUDFLARE_WORKER_URL");
        let soop_username = raw("SOOP_USERNAME");
        let (worker_api_key, soop_password) = if require_soop {
            let worker_api_key =
                unprotect_secret(&raw("CLOUDFLARE_API_KEY"), "CLOUDFLARE_API_KEY")?;
            let soop_password = unprotect_secret(&raw("SOOP_PASSWORD"), "SOOP_PASSWORD")?;
            if !worker_url.starts_with("https://") {
                bail!("CLOUDFLARE_WORKER_URL must be https:// when an enabled SOOP channel exists");
            }
            if worker_api_key.is_empty() {
                bail!("CLOUDFLARE_API_KEY is empty while an enabled SOOP channel exists");
            }
            (worker_api_key, soop_password)
        } else {
            (String::new(), String::new())
        };

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
            soop_username,
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
                "{}|{}|{}|{}|{}",
                channel.platform.as_str(),
                if channel.enabled { "Y" } else { "N" },
                channel.name,
                channel.account.to_ascii_lowercase(),
                channel.outdir
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
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

    fn channel(name: &str, account: &str) -> Channel {
        Channel {
            platform: PlatformId::Soop,
            enabled: true,
            name: name.into(),
            account: account.into(),
            outdir: String::new(),
        }
    }

    fn chzzk_channel(enabled: bool) -> Channel {
        Channel {
            platform: PlatformId::Chzzk,
            enabled,
            name: "CHZZK".into(),
            account: "0123456789abcdef0123456789abcdef".into(),
            outdir: String::new(),
        }
    }

    #[test]
    fn safe_filename_replaces_windows_reserved_chars() {
        assert_eq!(safe_name("a:b/c*?d", 80), "a_b_c__d");
        assert_eq!(safe_name("CON", 80), "_CON");
    }

    #[test]
    fn channel_signature_is_case_insensitive_for_accounts() {
        let a = vec![channel("A", "UserA")];
        let b = vec![channel("A", "usera")];
        assert_eq!(channel_signature(&a), channel_signature(&b));
    }

    #[test]
    fn soop_worker_credentials_are_needed_only_for_enabled_soop_channels() {
        assert!(!channels_require_soop(&[chzzk_channel(true)]));

        let mut disabled_soop = channel("SOOP", "disabled");
        disabled_soop.enabled = false;
        assert!(!channels_require_soop(&[disabled_soop, chzzk_channel(true)]));

        assert!(channels_require_soop(&[
            channel("SOOP", "enabled"),
            chzzk_channel(true),
        ]));
    }

    #[test]
    fn recorder_exit_outcome_distinguishes_crashes() {
        assert_eq!(
            recording_exit_outcome(Some(0)),
            (true, "NORMAL".to_string())
        );
        assert_eq!(
            recording_exit_outcome(Some(23)),
            (false, "RECORDER EXIT CODE=23".to_string())
        );
        assert_eq!(
            recording_exit_outcome(None),
            (false, "RECORDER EXIT CODE=unknown".to_string())
        );
    }

    #[test]
    fn watcher_exit_replaces_paused_state_and_clears_runtime_markers() {
        let mut disabled = channel("Disabled", "disabled");
        disabled.enabled = false;
        let mut states = HashMap::new();
        let mut live = ChannelState::new(channel("Live", "live"));
        live.status = "PAUSED".into();
        live.last_broadcast_id = Some("123".into());
        live.suppressed_broadcast_id = Some("123".into());
        live.detail = Some("old detail".into());
        states.insert(channel_key(PlatformId::Soop, "live"), live);
        states.insert(
            channel_key(PlatformId::Soop, "disabled"),
            ChannelState::new(disabled),
        );

        mark_watcher_stopped(&mut states);

        let live = states
            .get(&channel_key(PlatformId::Soop, "live"))
            .unwrap();
        assert_eq!(live.status, "WATCHER_STOPPED");
        assert!(live.last_broadcast_id.is_none());
        assert!(live.suppressed_broadcast_id.is_none());
        assert!(live.detail.is_none());
        assert_eq!(
            states
                .get(&channel_key(PlatformId::Soop, "disabled"))
                .unwrap()
                .status,
            "DISABLED"
        );
    }

    #[test]
    fn channel_key_namespaces_accounts_by_platform() {
        assert_eq!(channel_key(PlatformId::Soop, "User"), "SOOP:user");
        assert_eq!(
            channel_key(PlatformId::Chzzk, "ABCDEF0123456789ABCDEF0123456789"),
            "CHZZK:abcdef0123456789abcdef0123456789"
        );
    }
}
