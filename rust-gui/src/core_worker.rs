//! Dedicated background worker owning `StreamArchiveCore` and its Tokio runtime.
//! All persistence, process ownership, secret encryption, and network operations stay here.

use crate::{live_adapter, native_picker, native_shell};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use stream_archive_server::{
    app_core::StreamArchiveCore,
    backend::resolve_backend_dir,
    backup_service::{BackupPolicy, BackupSnapshot},
    diagnostics::DiagnosticsSnapshot,
    environment_settings::{EnvironmentSetting, SettingKind},
    history_service::HistoryFilter,
    model::{
        Channel, HistoryResponse, NativeWatcherStatus, VodAnalyzeRequest, VodDownloadRequest,
        VodJobStatus, VodQueueSnapshot,
    },
    storage_service::StorageSnapshot,
    support::platform::PlatformId,
};

pub enum Request {
    VodThumbnail {
        generation: u64,
        job_id: String,
        url: String,
    },
    Thumbnail {
        token: u64,
        platform: PlatformId,
        account: String,
        broadcast_id: String,
        completion_revision: u64,
    },
    Refresh,
    DesktopStatus,
    PrepareExit,
    Shutdown,
    Reload,
    Save(BTreeMap<String, String>),
    RememberClose(String),
    Pick(usize, SettingKind, String),
    ConfigReload,
    ChannelsSave(Vec<Channel>),
    ChannelPickOutput {
        index: usize,
        expected: Vec<Channel>,
        initial: String,
    },
    ChannelResolve {
        index: usize,
        platform: PlatformId,
        account: String,
    },
    ProviderSave {
        username: String,
        worker_url: String,
        secrets: BTreeMap<String, String>,
    },
    ProviderTestSoop,
    ClearKickToken,
    ClearProviderCredentials(String),
    LiveStatus {
        poll: bool,
        generation: u64,
    },
    LiveRefresh,
    LiveStart,
    LiveStop,
    LiveAction {
        target: String,
        action: String,
    },
    LivePassword {
        target: String,
        password: String,
    },
    StorageLoad {
        generation: u64,
        poll: bool,
    },
    VodStatus {
        poll: bool,
    },
    VodAnalyze {
        url: String,
    },
    VodPickOutput {
        initial: String,
    },
    VodDownload(VodDownloadRequest),
    VodCancel,
    QueueStatus {
        poll: bool,
    },
    QueueEnqueue(VodDownloadRequest),
    QueueAction {
        id: String,
        action: String,
    },
    HistoryLoad {
        filter: HistoryFilter,
        view: String,
    },
    MaintenanceLoad {
        active_tools: bool,
    },
    BackupPickDirectory {
        initial: String,
    },
    BackupSave {
        policy: BackupPolicy,
        directory: Option<String>,
    },
    BackupCreate,
    BackupRestore {
        file_name: String,
    },
    LogsLoad {
        poll: bool,
    },
    LogsClear,
    OpenLiveFolder(String),
    OpenLiveStream {
        platform: String,
        account: String,
    },
    OpenLogsFolder,
    SaveWindowBounds(String),
}

pub enum Response {
    ResourceTrace(stream_archive_server::resource_trace::ResourceTrace),
    VodThumbnail {
        generation: u64,
        url: String,
        image: Option<stream_archive_server::thumbnail_service::ThumbnailImage>,
    },
    Thumbnail {
        token: u64,
        image: Option<stream_archive_server::thumbnail_service::ThumbnailImage>,
    },
    RememberedClose(Result<String, String>),
    Snapshot {
        fields: Option<Vec<EnvironmentSetting>>,
        diagnostics: DiagnosticsSnapshot,
        window_bounds: Option<String>,
        first_run: bool,
        startup_failed: bool,
        message: String,
    },
    Configuration {
        channels: Vec<Channel>,
        username: String,
        worker_url: String,
        secrets: BTreeMap<String, bool>,
        reload_channels: bool,
        reload_provider: bool,
        message: String,
    },
    ChannelPicked {
        index: usize,
        expected: Vec<Channel>,
        path: Option<String>,
    },
    ChannelResolved {
        index: usize,
        account: String,
        name: String,
    },
    ConfigMessage(String),
    ConfigError(String),
    Picked(usize, Option<String>),
    Live {
        generation: Option<u64>,
        status: NativeWatcherStatus,
        refresh_thumbnails: bool,
        message: Option<String>,
        poll: bool,
    },
    LiveError {
        message: String,
        poll: bool,
    },
    Storage {
        generation: u64,
        snapshot: StorageSnapshot,
        message: Option<String>,
        poll: bool,
    },
    StorageError {
        generation: u64,
        message: String,
        poll: bool,
    },
    Vod {
        status: VodJobStatus,
        message: Option<String>,
        poll: bool,
    },
    VodPicked(Option<String>),
    VodError {
        message: String,
        poll: bool,
    },
    Queue {
        snapshot: VodQueueSnapshot,
        message: Option<String>,
        poll: bool,
    },
    QueueError {
        message: String,
        poll: bool,
    },
    History {
        history: HistoryResponse,
        view: String,
        message: Option<String>,
    },
    HistoryError(String),
    Maintenance {
        snapshot: BackupSnapshot,
        diagnostics: DiagnosticsSnapshot,
        logs: Vec<String>,
        message: String,
    },
    MaintenancePicked(Option<String>),
    Logs {
        lines: Vec<String>,
        poll: bool,
    },
    LogsCleared,
    LiveFolderOpened(Result<(), String>),
    LiveStreamOpened(Result<(), String>),
    LogsFolderOpened(Result<(), String>),
    MaintenanceError {
        message: String,
        poll: bool,
    },
    Error(String),
    DesktopStatus(String),
    ExitCheck(Result<bool, String>),
    ExitComplete,
    Downloads(Vec<stream_archive_server::download_events::DownloadEvent>),
    ClearNotices,
}

fn read_active_snapshot(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    include_settings: bool,
    message: &str,
) -> Response {
    let fields = if include_settings {
        match core.environment_settings() {
            Ok(fields) => Some(fields),
            Err(error) => return Response::Error(format!("설정 불러오기 실패: {error:#}")),
        }
    } else {
        None
    };
    Response::Snapshot {
        fields,
        diagnostics: runtime.block_on(core.active_local_diagnostics()),
        window_bounds: core.window_bounds().ok().flatten(),
        first_run: core.is_first_run_unconfigured().unwrap_or(false),
        startup_failed: false,
        message: message.into(),
    }
}

fn read_snapshot(core: &StreamArchiveCore, include_settings: bool, message: &str) -> Response {
    let fields = if include_settings {
        match core.environment_settings() {
            Ok(fields) => Some(fields),
            Err(error) => return Response::Error(format!("설정 불러오기 실패: {error:#}")),
        }
    } else {
        None
    };
    Response::Snapshot {
        fields,
        diagnostics: core.diagnostics(),
        window_bounds: core.window_bounds().ok().flatten(),
        first_run: core.is_first_run_unconfigured().unwrap_or(false),
        startup_failed: false,
        message: message.into(),
    }
}

fn configuration_snapshot(
    core: &StreamArchiveCore,
    reload_channels: bool,
    reload_provider: bool,
    message: &str,
) -> Response {
    let settings = match core.settings() {
        Ok(settings) => settings,
        Err(error) => {
            return Response::ConfigError(format!("공급자 설정 불러오기 실패: {error:#}"));
        }
    };
    let channels = match core.channels() {
        Ok(channels) => channels,
        Err(error) => {
            return Response::ConfigError(format!("채널 목록 불러오기 실패: {error:#}"));
        }
    };
    let secrets = match core.configured_secrets() {
        Ok(secrets) => secrets,
        Err(error) => {
            return Response::ConfigError(format!("인증 정보 상태 불러오기 실패: {error:#}"));
        }
    };
    Response::Configuration {
        channels,
        username: settings.get("SOOP_USERNAME").cloned().unwrap_or_default(),
        worker_url: settings
            .get("CLOUDFLARE_WORKER_URL")
            .cloned()
            .unwrap_or_default(),
        secrets,
        reload_channels,
        reload_provider,
        message: message.into(),
    }
}

async fn live_status_async(
    core: &StreamArchiveCore,
    poll: bool,
    generation: Option<u64>,
    refresh_thumbnails: bool,
) -> Response {
    match core.watcher_status().await {
        Ok(status) => Response::Live {
            generation,
            status,
            refresh_thumbnails,
            message: None,
            poll,
        },
        Err(error) => Response::LiveError {
            message: format!("LIVE 상태 새로고침 실패: {error:#}"),
            poll,
        },
    }
}

fn live_status(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    poll: bool,
    generation: Option<u64>,
) -> Response {
    runtime.block_on(live_status_async(core, poll, generation, false))
}

fn storage_status(core: &StreamArchiveCore, poll: bool, generation: u64) -> Response {
    match core.storage_snapshot() {
        Ok(snapshot) => Response::Storage {
            generation,
            snapshot,
            message: None,
            poll,
        },
        Err(error) => Response::StorageError {
            generation,
            message: format!("저장 공간 새로고침 실패: {error:#}"),
            poll,
        },
    }
}

fn vod_status(core: &StreamArchiveCore, runtime: &tokio::runtime::Runtime, poll: bool) -> Response {
    if poll {
        return Response::Vod {
            status: runtime.block_on(core.local_vod_status()),
            message: None,
            poll: true,
        };
    }
    match runtime.block_on(core.vod_status()) {
        Ok(status) => Response::Vod {
            status,
            message: None,
            poll: false,
        },
        Err(error) => Response::VodError {
            message: format!("VOD 상태 새로고침 실패: {error:#}"),
            poll: false,
        },
    }
}

fn queue_status(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    poll: bool,
) -> Response {
    match runtime.block_on(core.queue_snapshot()) {
        Ok(snapshot) => Response::Queue {
            snapshot,
            message: None,
            poll,
        },
        Err(error) => Response::QueueError {
            message: format!("대기열 새로고침 실패: {error:#}"),
            poll,
        },
    }
}

fn maintenance_snapshot(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    active_tools: bool,
    message: impl Into<String>,
) -> Response {
    match runtime.block_on(core.backup_snapshot()) {
        Ok(snapshot) => Response::Maintenance {
            snapshot,
            diagnostics: if active_tools {
                runtime.block_on(core.active_local_diagnostics())
            } else {
                core.diagnostics()
            },
            logs: runtime.block_on(core.runtime_logs(200)),
            message: message.into(),
        },
        Err(error) => Response::MaintenanceError {
            message: format!("관리 상태 새로고침 실패: {error:#}"),
            poll: false,
        },
    }
}

fn selected_parts_label(parts: &[usize]) -> String {
    if parts.is_empty() {
        return "전체".into();
    }
    parts
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn vod_analyze_request(url: String) -> VodAnalyzeRequest {
    VodAnalyzeRequest {
        vod_url: url,
        cookie_mode: "SOOP_LOGIN".into(),
        cookie_file: String::new(),
        browser_name: "firefox".into(),
        yt_dlp_path: String::new(),
        ffmpeg_path: String::new(),
        max_retries: 5,
    }
}

fn save_channels(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    mut channels: Vec<Channel>,
) -> Response {
    for channel in &mut channels {
        if channel.name.trim().is_empty() && !channel.account.trim().is_empty() {
            match runtime.block_on(core.resolve_channel_name(channel.platform, &channel.account)) {
                Ok(name) => channel.name = name,
                Err(error) => {
                    return Response::ConfigError(format!(
                        "채널 이름 조회 실패 {}/{}: {error:#}",
                        channel.platform, channel.account
                    ));
                }
            }
        }
    }

    match runtime.block_on(core.update_channels(&channels)) {
        Ok(_) => configuration_snapshot(
            core,
            true,
            false,
            "채널 목록을 저장했습니다. 실행 중인 Watcher는 기존 재로딩 정책에 따라 canonical 목록을 반영합니다.",
        ),
        Err(error) => Response::ConfigError(format!("채널 목록을 저장하지 못했습니다: {error:#}")),
    }
}

fn spawn_detached_task(name: &str, f: impl FnOnce() + Send + 'static) -> Result<(), String> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(f)
        .map(|_| ())
        .map_err(|error| format!("백그라운드 작업 스레드를 시작할 수 없습니다: {error}"))
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub fn spawn_worker(
    requests: mpsc::Receiver<Request>,
    responses: mpsc::Sender<Response>,
    stopping: Arc<AtomicBool>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("stream-archive-core-worker".into())
        .spawn(move || worker(requests, responses, stopping))
}

fn worker(
    requests: mpsc::Receiver<Request>,
    responses: mpsc::Sender<Response>,
    stopping: Arc<AtomicBool>,
) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = responses.send(Response::Error(format!("Worker 초기화 실패: {error}")));
            return;
        }
    };
    let backend = resolve_backend_dir();
    let backend_path = backend.as_ref().ok().cloned();
    let core = match backend.and_then(StreamArchiveCore::open) {
        Ok(opened) => opened.core,
        Err(error) => {
            let message = format!("런타임 초기화 실패: {error:#}");
            let _ = responses.send(Response::Snapshot {
                fields: None,
                diagnostics: DiagnosticsSnapshot::startup_failure(
                    backend_path.as_deref(),
                    &message,
                ),
                window_bounds: None,
                first_run: false,
                startup_failed: true,
                message,
            });
            return;
        }
    };
    worker_loop(&core, &runtime, requests, &responses, &stopping);
    runtime.block_on(core.shutdown());
    let _ = responses.send(Response::ExitComplete);
}

pub fn worker_loop(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    requests: mpsc::Receiver<Request>,
    responses: &mpsc::Sender<Response>,
    stopping: &AtomicBool,
) {
    let _ = responses.send(Response::ResourceTrace(core.resource_trace()));
    let mut download_events = core.subscribe_download_events();
    let event_responses = responses.clone();
    let _download_event_guard = AbortOnDrop(runtime.spawn(async move {
        loop {
            match download_events.recv().await {
                Ok(first) => {
                    let mut batch = vec![first];
                    batch.extend(crate::notifications::drain_download_events(|| {
                        download_events.try_recv()
                    }));
                    if event_responses.send(Response::Downloads(batch)).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                    eprintln!("Windows download notifications skipped {count} older events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }));

    runtime.block_on(async {
        core.spawn_vod_history_sync();
        core.spawn_queue_worker();
        core.spawn_auto_backup();
    });
    if responses
        .send(read_snapshot(
            core,
            true,
            "canonical SQLite에서 설정을 불러왔습니다. 저장한 변경사항은 이후 런타임 작업부터 적용됩니다.",
        ))
        .is_err()
    {
        return;
    }
    if responses
        .send(configuration_snapshot(
            core,
            true,
            true,
            "canonical SQLite에서 채널 및 서비스 연결 정보를 불러왔습니다.",
        ))
        .is_err()
    {
        return;
    }
    if responses
        .send(live_status(core, runtime, false, None))
        .is_err()
    {
        return;
    }
    if responses.send(storage_status(core, false, 0)).is_err() {
        return;
    }
    if responses.send(vod_status(core, runtime, false)).is_err() {
        return;
    }
    if responses.send(queue_status(core, runtime, false)).is_err() {
        return;
    }
    match core.history(&HistoryFilter::default()) {
        Ok(history) => {
            if responses
                .send(Response::History {
                    history,
                    view: "ALL".into(),
                    message: Some("canonical SQLite에서 기록을 불러왔습니다".into()),
                })
                .is_err()
            {
                return;
            }
        }
        Err(error) => {
            if responses
                .send(Response::HistoryError(format!(
                    "기록 불러오기 실패: {error:#}"
                )))
                .is_err()
            {
                return;
            }
        }
    }

    if responses
        .send(maintenance_snapshot(
            core,
            runtime,
            false,
            "공유 런타임 서비스에서 관리 상태를 불러왔습니다.",
        ))
        .is_err()
    {
        return;
    }

    for request in requests {
        if stopping.load(Ordering::Acquire) {
            break;
        }
        let response = match request {
            Request::VodThumbnail {
                generation,
                job_id,
                url,
            } => {
                let core = core.clone();
                let responses = responses.clone();
                runtime.spawn(async move {
                    let image = core.vod_thumbnail(&job_id, &url).await.ok();
                    let _ = responses.send(Response::VodThumbnail {
                        generation,
                        url,
                        image,
                    });
                });
                continue;
            }
            Request::Thumbnail {
                token,
                platform,
                account,
                broadcast_id,
                completion_revision,
            } => {
                let core = core.clone();
                let responses = responses.clone();
                runtime.spawn(async move {
                    let image = core
                        .live_thumbnail(platform, &account, &broadcast_id, completion_revision)
                        .await
                        .ok();
                    let _ = responses.send(Response::Thumbnail { token, image });
                });
                continue;
            }
            Request::Shutdown => break,
            Request::DesktopStatus => {
                let core = core.clone();
                let responses = responses.clone();
                runtime.spawn(async move {
                    let status = desktop_snapshot(&core).await;
                    let _ = responses.send(Response::DesktopStatus(match status {
                        Ok((summary, _)) => summary,
                        Err(error) => format!("상태 확인 실패: {error}"),
                    }));
                });
                continue;
            }
            Request::PrepareExit => Response::ExitCheck(
                runtime
                    .block_on(desktop_snapshot(core))
                    .map(|(_, active)| active),
            ),
            Request::Refresh => read_active_snapshot(
                core,
                runtime,
                false,
                "진단 정보를 다시 탐색하고 로컬 도구 버전을 확인했습니다. 저장하지 않은 편집 내용은 유지됩니다.",
            ),
            Request::Reload => read_snapshot(
                core,
                true,
                "저장된 설정을 다시 불러왔습니다. 편집 중이던 내용은 취소되었습니다.",
            ),
            Request::RememberClose(action) => Response::RememberedClose(
                runtime
                    .block_on(core.update_environment_settings(&BTreeMap::from([(
                        "STREAM_ARCHIVE_CLOSE_ACTION".into(),
                        action.clone(),
                    )])))
                    .map(|_| action)
                    .map_err(|error| format!("닫기 동작 저장 실패: {error:#}")),
            ),
            Request::Save(patch) => {
                match runtime.block_on(core.update_environment_settings(&patch)) {
                    Ok(_) => {
                        if patch.contains_key("STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS") {
                            let _ = responses.send(Response::ClearNotices);
                        }
                        read_snapshot(
                            core,
                            true,
                            "canonical SQLite에 저장했습니다. 진행 중인 녹화는 현재 설정을 그대로 유지합니다.",
                        )
                    }
                    Err(error) => Response::Error(format!("저장 실패: {error:#}")),
                }
            }
            Request::Pick(index, kind, initial) => {
                let responses = responses.clone();
                if let Err(error) = spawn_detached_task("native-picker", move || {
                    let response = match native_picker::pick(kind, &initial) {
                        Ok(path) => Response::Picked(index, path),
                        Err(error) => Response::Error(format!("폴더 선택기 오류: {error}")),
                    };
                    let _ = responses.send(response);
                }) {
                    Response::Error(error)
                } else {
                    continue;
                }
            }
            Request::ConfigReload => configuration_snapshot(
                core,
                true,
                true,
                "저장된 채널 및 공급자 인증 정보를 다시 불러왔습니다. 편집 중이던 내용은 취소되었습니다.",
            ),
            Request::ChannelsSave(channels) => save_channels(core, runtime, channels),
            Request::ChannelPickOutput {
                index,
                expected,
                initial,
            } => {
                let responses = responses.clone();
                if let Err(error) = spawn_detached_task("native-picker", move || {
                    let response = match native_picker::pick_directory(&initial) {
                        Ok(path) => Response::ChannelPicked {
                            index,
                            expected,
                            path,
                        },
                        Err(message) => Response::ConfigError(message),
                    };
                    let _ = responses.send(response);
                }) {
                    Response::ConfigError(error)
                } else {
                    continue;
                }
            }
            Request::ChannelResolve {
                index,
                platform,
                account,
            } => {
                let core = core.clone();
                let responses = responses.clone();
                runtime.spawn(async move {
                    let response = match core.resolve_channel_name(platform, &account).await {
                        Ok(name) => Response::ChannelResolved {
                            index,
                            account,
                            name,
                        },
                        Err(error) => Response::ConfigError(format!(
                            "채널 이름 조회 실패 {platform}/{account}: {error:#}"
                        )),
                    };
                    let _ = responses.send(response);
                });
                continue;
            }
            Request::ProviderSave {
                username,
                worker_url,
                secrets,
            } => {
                let settings = BTreeMap::from([
                    ("SOOP_USERNAME".into(), username),
                    ("CLOUDFLARE_WORKER_URL".into(), worker_url),
                ]);
                match runtime.block_on(core.update_provider_configuration(&settings, &secrets)) {
                    Ok(_) => configuration_snapshot(
                        core,
                        false,
                        true,
                        "서비스 연결 정보를 저장했습니다. 비밀 값은 암호화 상태로 유지되며 UI로 다시 읽어오지 않습니다.",
                    ),
                    Err(error) => Response::ConfigError(format!(
                        "서비스 연결 정보를 저장하지 못했습니다: {error:#}"
                    )),
                }
            }
            Request::ClearProviderCredentials(provider) => {
                match runtime.block_on(core.clear_provider_credentials(&provider)) {
                    Ok(()) => configuration_snapshot(
                        core,
                        false,
                        true,
                        &format!("{provider} 인증정보를 삭제했습니다."),
                    ),
                    Err(error) => Response::ConfigError(format!(
                        "{provider} 인증정보 삭제가 완료되지 않았습니다. 삭제를 다시 시도하세요: {error:#}"
                    )),
                }
            }
            Request::ClearKickToken => match runtime.block_on(core.clear_kick_token()) {
                Ok(()) => {
                    configuration_snapshot(core, false, true, "KICK 인증정보를 삭제했습니다.")
                }
                Err(error) => Response::ConfigError(format!("KICK 인증정보 삭제 실패: {error:#}")),
            },
            Request::ProviderTestSoop => {
                let core = core.clone();
                let responses = responses.clone();
                runtime.spawn(async move {
                    let response = match core.test_soop_auth().await {
                        Ok(message) => Response::ConfigMessage(message),
                        Err(error) => {
                            Response::ConfigError(format!("SOOP / Worker 테스트 실패: {error:#}"))
                        }
                    };
                    let _ = responses.send(response);
                });
                continue;
            }
            Request::LiveStatus { poll, generation } => {
                let core = core.clone();
                let responses = responses.clone();
                runtime.spawn(async move {
                    let _ = responses
                        .send(live_status_async(&core, poll, Some(generation), false).await);
                });
                continue;
            }
            Request::LiveRefresh => {
                let core = core.clone();
                let responses = responses.clone();
                runtime.spawn(async move {
                    let _ = responses.send(live_status_async(&core, false, None, true).await);
                });
                continue;
            }
            Request::StorageLoad { poll, generation } => storage_status(core, poll, generation),
            Request::LiveStart => match runtime.block_on(core.start_watcher()) {
                Ok(status) => Response::Live {
                    generation: None,
                    status,
                    refresh_thumbnails: false,
                    message: Some("LIVE Watcher를 시작했습니다".into()),
                    poll: false,
                },
                Err(error) => Response::LiveError {
                    message: format!("Watcher 시작 실패: {error:#}"),
                    poll: false,
                },
            },
            Request::LiveStop => match runtime.block_on(core.stop_watcher()) {
                Ok(status) => Response::Live {
                    generation: None,
                    status,
                    refresh_thumbnails: false,
                    message: Some("LIVE Watcher를 중지했습니다".into()),
                    poll: false,
                },
                Err(error) => Response::LiveError {
                    message: format!("Watcher 중지 실패: {error:#}"),
                    poll: false,
                },
            },
            Request::LiveAction { target, action } => {
                let Some(action) = live_adapter::validated_action(&action) else {
                    let _ = responses.send(Response::LiveError {
                        message: "지원하지 않는 LIVE 채널 작업입니다".into(),
                        poll: false,
                    });
                    continue;
                };
                match runtime.block_on(core.channel_action(target, action)) {
                    Ok(()) => match runtime.block_on(core.watcher_status()) {
                        Ok(status) => Response::Live {
                            generation: None,
                            status,
                            refresh_thumbnails: false,
                            message: Some(match action {
                                "stop" => "현재 방송은 방송이 바뀔 때까지 제외됩니다".into(),
                                "resume" => "채널 모니터링을 재개했습니다".into(),
                                _ => "채널 다시 확인을 요청했습니다".into(),
                            }),
                            poll: false,
                        },
                        Err(error) => Response::LiveError {
                            message: format!(
                                "작업은 성공했지만 상태 새로고침에 실패했습니다: {error:#}"
                            ),
                            poll: false,
                        },
                    },
                    Err(error) => Response::LiveError {
                        message: format!("채널 작업 실패: {error:#}"),
                        poll: false,
                    },
                }
            }
            Request::LivePassword { target, password } => {
                match runtime.block_on(core.channel_password(target, password)) {
                    Ok(()) => match runtime.block_on(core.watcher_status()) {
                        Ok(status) => Response::Live {
                            generation: None,
                            status,
                            refresh_thumbnails: false,
                            message: Some(
                                "비밀번호를 메모리에 전달하고 채널 다시 확인을 요청했습니다".into(),
                            ),
                            poll: false,
                        },
                        Err(error) => Response::LiveError {
                            message: format!(
                                "비밀번호는 적용됐지만 상태 새로고침에 실패했습니다: {error:#}"
                            ),
                            poll: false,
                        },
                    },
                    Err(error) => Response::LiveError {
                        message: format!("비밀번호를 적용하지 못했습니다: {error:#}"),
                        poll: false,
                    },
                }
            }
            Request::VodStatus { poll } => vod_status(core, runtime, poll),
            Request::VodAnalyze { url } => {
                match runtime.block_on(core.analyze_vod(vod_analyze_request(url))) {
                    Ok(status) => Response::Vod {
                        status,
                        message: Some("VOD 분석을 시작했습니다".into()),
                        poll: false,
                    },
                    Err(error) => Response::VodError {
                        message: format!("VOD 분석을 시작하지 못했습니다: {error:#}"),
                        poll: false,
                    },
                }
            }
            Request::VodPickOutput { initial } => {
                let responses = responses.clone();
                if let Err(error) = spawn_detached_task("native-picker", move || {
                    let response = match native_picker::pick_directory(&initial) {
                        Ok(path) => Response::VodPicked(path),
                        Err(error) => Response::VodError {
                            message: format!("VOD 출력 폴더 선택기 오류: {error}"),
                            poll: false,
                        },
                    };
                    let _ = responses.send(response);
                }) {
                    Response::VodError {
                        message: error,
                        poll: false,
                    }
                } else {
                    continue;
                }
            }
            Request::VodDownload(req) => {
                let selected_parts = selected_parts_label(&req.parts);
                match runtime.block_on(core.download_vod(req)) {
                    Ok(status) => Response::Vod {
                        status,
                        message: Some(format!(
                            "VOD 다운로드를 시작했습니다 · 선택한 PART: {selected_parts}"
                        )),
                        poll: false,
                    },
                    Err(error) => Response::VodError {
                        message: format!("VOD 다운로드를 시작하지 못했습니다: {error:#}"),
                        poll: false,
                    },
                }
            }
            Request::VodCancel => match runtime.block_on(core.cancel_vod()) {
                Ok(status) => Response::Vod {
                    status,
                    message: Some("VOD 작업 취소를 완료했습니다".into()),
                    poll: false,
                },
                Err(error) => Response::VodError {
                    message: format!("VOD 작업 취소 실패: {error:#}"),
                    poll: false,
                },
            },
            Request::QueueStatus { poll } => queue_status(core, runtime, poll),
            Request::QueueEnqueue(req) => {
                let selected_parts = selected_parts_label(&req.parts);
                match runtime.block_on(core.enqueue_vod(req)) {
                    Ok(item) => match runtime.block_on(core.queue_snapshot()) {
                        Ok(snapshot) => Response::Queue {
                            snapshot,
                            message: Some(format!(
                                "VOD 작업을 대기열에 추가했습니다 {} · 선택한 PART: {selected_parts}",
                                item.id
                            )),
                            poll: false,
                        },
                        Err(error) => Response::QueueError {
                            message: format!(
                                "작업은 대기열에 추가됐지만 새로고침에 실패했습니다: {error:#}"
                            ),
                            poll: false,
                        },
                    },
                    Err(error) => Response::QueueError {
                        message: format!("대기열 추가 실패: {error:#}"),
                        poll: false,
                    },
                }
            }
            Request::QueueAction { id, action } => {
                let result = match action.as_str() {
                    "cancel" => runtime.block_on(core.cancel_queue_item(&id)),
                    "retry" => runtime.block_on(core.retry_queue_item(&id)),
                    "remove" => runtime.block_on(core.remove_queue_item(&id)),
                    _ => Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("지원하지 않는 대기열 작업: {action}"),
                    )
                    .into()),
                };
                match result {
                    Ok(snapshot) => Response::Queue {
                        snapshot,
                        message: Some(format!(
                            "대기열 작업 완료: {}",
                            match action.as_str() {
                                "cancel" => "취소",
                                "retry" => "재시도",
                                "remove" => "삭제",
                                _ => action.as_str(),
                            }
                        )),
                        poll: false,
                    },
                    Err(error) => Response::QueueError {
                        message: format!("대기열 작업 실패: {error:#}"),
                        poll: false,
                    },
                }
            }
            Request::HistoryLoad { filter, view } => match core.history(&filter) {
                Ok(history) => Response::History {
                    history,
                    view,
                    message: Some("기록을 새로고침했습니다".into()),
                },
                Err(error) => Response::HistoryError(format!("기록 새로고침 실패: {error:#}")),
            },
            Request::MaintenanceLoad { active_tools } => maintenance_snapshot(
                core,
                runtime,
                active_tools,
                if active_tools {
                    "관리 상태를 다시 탐색하고 로컬 도구 버전을 확인했습니다"
                } else {
                    "백업 목록과 관리 상태를 새로고침했습니다"
                },
            ),
            Request::BackupPickDirectory { initial } => {
                let responses = responses.clone();
                if let Err(error) = spawn_detached_task("native-picker", move || {
                    let response = match native_picker::pick_directory(&initial) {
                        Ok(path) => Response::MaintenancePicked(path),
                        Err(error) => Response::MaintenanceError {
                            message: format!("백업 폴더 선택기 오류: {error}"),
                            poll: false,
                        },
                    };
                    let _ = responses.send(response);
                }) {
                    Response::MaintenanceError {
                        message: error,
                        poll: false,
                    }
                } else {
                    continue;
                }
            }
            Request::BackupSave { policy, directory } => {
                match runtime.block_on(core.update_backup_policy(&policy, directory.as_deref())) {
                    Ok(snapshot) => Response::Maintenance {
                        snapshot,
                        diagnostics: core.diagnostics(),
                        logs: runtime.block_on(core.runtime_logs(200)),
                        message: "백업 정책을 canonical SQLite에 저장했습니다.".into(),
                    },
                    Err(error) => Response::MaintenanceError {
                        message: format!("백업 정책을 저장하지 못했습니다: {error:#}"),
                        poll: false,
                    },
                }
            }
            Request::BackupCreate => match runtime.block_on(core.create_manual_backup()) {
                Ok(snapshot) => Response::Maintenance {
                    snapshot,
                    diagnostics: core.diagnostics(),
                    logs: runtime.block_on(core.runtime_logs(200)),
                    message: "수동 백업을 생성하고 검증했습니다.".into(),
                },
                Err(error) => Response::MaintenanceError {
                    message: format!("수동 백업 실패: {error:#}"),
                    poll: false,
                },
            },
            Request::BackupRestore { file_name } => {
                match runtime.block_on(core.restore_backup(&file_name)) {
                    Ok(outcome) => {
                        let _ = responses.send(Response::ClearNotices);
                        let _ = responses.send(read_snapshot(
                            core,
                            true,
                            "DB를 복원하고 canonical SQLite에서 설정을 다시 불러왔습니다.",
                        ));
                        let _ = responses.send(configuration_snapshot(
                            core,
                            true,
                            true,
                            "DB를 복원하고 채널 및 서비스 연결 정보를 다시 불러왔습니다.",
                        ));
                        let _ = responses.send(queue_status(core, runtime, false));
                        if let Ok(history) = core.history(&HistoryFilter::default()) {
                            let _ = responses.send(Response::History {
                                history,
                                view: "ALL".into(),
                                message: Some("복원 후 기록을 다시 불러왔습니다".into()),
                            });
                        }
                        maintenance_snapshot(
                            core,
                            runtime,
                            false,
                            format!(
                                "복원 완료: {}. 안전 백업: {}. Watcher는 중지 상태를 유지합니다.",
                                outcome.restored.file_name, outcome.safety_backup.file_name
                            ),
                        )
                    }
                    Err(error) => Response::MaintenanceError {
                        message: format!("복원이 차단되었거나 실패했습니다: {error:#}"),
                        poll: false,
                    },
                }
            }
            Request::LogsLoad { poll } => Response::Logs {
                lines: runtime.block_on(core.runtime_logs(200)),
                poll,
            },
            Request::LogsClear => {
                runtime.block_on(core.clear_runtime_logs());
                Response::LogsCleared
            }
            Request::OpenLiveFolder(file_path) => {
                let responses = responses.clone();
                if let Err(error) = spawn_detached_task("native-shell", move || {
                    let result = native_shell::open_containing_directory(&file_path);
                    let _ = responses.send(Response::LiveFolderOpened(result));
                }) {
                    Response::LiveFolderOpened(Err(error))
                } else {
                    continue;
                }
            }
            Request::OpenLiveStream { platform, account } => {
                let responses = responses.clone();
                if let Err(error) = spawn_detached_task("native-shell", move || {
                    let result = native_shell::open_provider_url(&platform, &account);
                    let _ = responses.send(Response::LiveStreamOpened(result));
                }) {
                    Response::LiveStreamOpened(Err(error))
                } else {
                    continue;
                }
            }
            Request::SaveWindowBounds(spec) => {
                let _ = core.save_window_bounds(&spec);
                continue;
            }
            Request::OpenLogsFolder => {
                let target = core.logs_dir().or_else(|| {
                    std::env::current_exe()
                        .ok()
                        .and_then(|exe| exe.parent().map(|p| p.join("log")))
                });
                let result = match target {
                    Some(dir) => match std::fs::create_dir_all(&dir) {
                        Ok(()) => Ok(dir.display().to_string()),
                        Err(err) => Err(format!("로그 폴더를 생성할 수 없습니다: {err}")),
                    },
                    None => Err("로그 폴더 경로를 확인할 수 없습니다.".to_string()),
                };
                match result {
                    Ok(dir) => {
                        let responses = responses.clone();
                        if let Err(error) = spawn_detached_task("native-shell", move || {
                            let result = native_shell::open_containing_directory(&dir);
                            let _ = responses.send(Response::LogsFolderOpened(result));
                        }) {
                            Response::LogsFolderOpened(Err(error))
                        } else {
                            continue;
                        }
                    }
                    Err(err) => Response::LogsFolderOpened(Err(err)),
                }
            }
        };
        if responses.send(response).is_err() {
            break;
        }
    }
}

async fn desktop_snapshot(core: &StreamArchiveCore) -> Result<(String, bool), String> {
    let live = core.watcher_status().await.map_err(|e| e.to_string())?;
    let vod = core.local_vod_status().await;
    let queue = core.queue_snapshot().await.map_err(|e| e.to_string())?;
    let summary = format!(
        "녹화 {}개 / 다운로드 {}개 / 대기 {}개 / 감시 {}",
        live.recording_count,
        usize::from(vod.running),
        queue.queued_count,
        if live.running { "실행 중" } else { "중지" }
    );
    let active = live.running
        || live.recording_count > 0
        || vod.running
        || queue.active_id.is_some()
        || queue.queued_count > 0;
    Ok((summary, active))
}

pub fn localized_setting_description<'a>(key: &str, fallback: &'a str) -> &'a str {
    match key {
        "STREAMLINK_PATH" => {
            "Streamlink 실행 파일 경로입니다. 비워두거나 AUTO를 사용하면 자동 탐색합니다."
        }
        "STREAMLINK_FALLBACK" => {
            "대체 Streamlink 실행 파일 경로입니다. 비워두거나 AUTO를 사용하면 자동 탐색합니다."
        }
        "YT_DLP_PATH" => "yt-dlp 실행 파일 경로입니다. 비워두면 런타임에서 자동 탐색합니다.",
        "FFMPEG_PATH" => "FFmpeg 실행 파일 경로입니다. 비워두면 런타임에서 자동 탐색합니다.",
        "OUTPUT_DIR" => {
            "LIVE 및 VOD 기본 저장 폴더입니다. VOD에서는 개별 경로를 지정할 수 있습니다."
        }
        "CHECK_INTERVAL" => "LIVE 상태 확인 간격(초)입니다. 허용 범위: 1~86400.",
        "MIN_FREE_SPACE_GB" => "최소 여유 디스크 공간(GB)입니다. 허용 범위: 0~1000000.",
        "QUALITY" => "LIVE 화질입니다. 예: best",
        "STREAM_ARCHIVE_CLOSE_ACTION" => {
            "Windows 닫기 버튼 동작입니다. 트레이에서는 녹화·다운로드·감시가 계속됩니다. 변경사항을 저장하면 적용됩니다."
        }
        "GUI_NOTIFY_RECORD_START" => {
            "새 LIVE 방송 녹화가 시작될 때 Windows 트레이 알림을 표시합니다. 변경사항 저장 후 적용됩니다."
        }
        "GUI_NOTIFY_WARNING" => {
            "저장 공간 부족(CRITICAL) 또는 채널 오류·인증 필요 상태 전환 시 Windows 트레이 경고 알림을 표시합니다."
        }
        _ => fallback,
    }
}

#[cfg(test)]
#[path = "../tests/support/desktop_process.rs"]
mod desktop_process;

#[cfg(test)]
mod desktop_lifecycle_tests {
    use super::*;

    #[test]
    fn confirmed_shutdown_does_not_execute_queued_mutations() {
        desktop_process::run(
            "core_worker::desktop_lifecycle_tests::confirmed_shutdown_does_not_execute_queued_mutations",
            |directory| {
                let backend = directory.join("backend");
                std::fs::create_dir_all(&backend).unwrap();
                let core = StreamArchiveCore::open(&backend).unwrap().core;
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .enable_all()
                    .build()
                    .unwrap();
                let (sender, requests) = mpsc::channel();
                let (responses, _receiver) = mpsc::channel();
                sender
                    .send(Request::Save(BTreeMap::from([(
                        "CHECK_INTERVAL".into(),
                        "99".into(),
                    )])))
                    .unwrap();
                sender.send(Request::Shutdown).unwrap();
                drop(sender);
                let stopping = AtomicBool::new(true);
                worker_loop(&core, &runtime, requests, &responses, &stopping);
                assert_eq!(core.settings().unwrap()["CHECK_INTERVAL"], "30");
                runtime.block_on(core.shutdown());
                drop(runtime);
                drop(core);
            },
        );
    }
}
