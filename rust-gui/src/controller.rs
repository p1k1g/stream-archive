use crate::{
    AppState, ChannelConfigRow, DiagnosticRow, HistoryCalendarDay, HistoryDisplayRow,
    LiveChannelRow, MainWindow, MaintenanceBackupRow, MaintenanceDiagnosticRow, MaintenanceLogRow,
    MaintenanceState, QueueDisplayRow, QueueHistoryState, SettingRow, VodPartRow, VodQualityRow,
    channels_adapter::ChannelsDraft, history_adapter, live_adapter, maintenance_adapter,
    native_picker, queue_adapter, settings_adapter::SettingsDraft, vod_adapter,
};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::mpsc,
    time::Duration,
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
    support::platform::PlatformId,
};

use vod_adapter::{AnalysisSync, VodDraft};

enum Request {
    Refresh,
    Reload,
    Save(BTreeMap<String, String>),
    Pick(usize, SettingKind, String),
    ConfigReload,
    ChannelsSave(Vec<Channel>),
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
    LiveStatus {
        poll: bool,
    },
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
    MaintenanceLoad,
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
}

enum Response {
    Snapshot {
        fields: Option<Vec<EnvironmentSetting>>,
        diagnostics: DiagnosticsSnapshot,
        first_run: bool,
        message: String,
    },
    Configuration {
        channels: Vec<Channel>,
        username: String,
        worker_url: String,
        secrets: BTreeMap<String, bool>,
        message: String,
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
        status: NativeWatcherStatus,
        message: Option<String>,
        poll: bool,
    },
    LiveError {
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
    MaintenanceError {
        message: String,
        poll: bool,
    },
    Error(String),
}

pub struct Controller {
    _response_timer: Timer,
    _live_poll_timer: Timer,
    _vod_poll_timer: Timer,
    _queue_poll_timer: Timer,
    _maintenance_log_poll_timer: Timer,
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
        first_run: core.is_first_run_unconfigured().unwrap_or(false),
        message: message.into(),
    }
}

fn configuration_snapshot(core: &StreamArchiveCore, message: &str) -> Response {
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
        message: message.into(),
    }
}

fn live_status(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    poll: bool,
) -> Response {
    match runtime.block_on(core.watcher_status()) {
        Ok(status) => Response::Live {
            status,
            message: None,
            poll,
        },
        Err(error) => Response::LiveError {
            message: format!("LIVE 상태 새로고침 실패: {error:#}"),
            poll,
        },
    }
}

fn vod_status(core: &StreamArchiveCore, runtime: &tokio::runtime::Runtime, poll: bool) -> Response {
    match runtime.block_on(core.vod_status()) {
        Ok(status) => Response::Vod {
            status,
            message: None,
            poll,
        },
        Err(error) => Response::VodError {
            message: format!("VOD 상태 새로고침 실패: {error:#}"),
            poll,
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
    message: impl Into<String>,
) -> Response {
    match runtime.block_on(core.backup_snapshot()) {
        Ok(snapshot) => Response::Maintenance {
            snapshot,
            diagnostics: core.diagnostics(),
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
            "채널 목록을 저장했습니다. 실행 중인 Watcher는 기존 재로딩 정책에 따라 canonical 목록을 반영합니다.",
        ),
        Err(error) => Response::ConfigError(format!("채널 목록을 저장하지 못했습니다: {error:#}")),
    }
}

fn worker(requests: mpsc::Receiver<Request>, responses: mpsc::Sender<Response>) {
    // Watcher/VOD operations spawn long-lived Tokio tasks. A single-worker
    // multi-thread runtime keeps those tasks moving between GUI requests while
    // retaining one dedicated native-runtime worker for controller requests.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = responses.send(Response::Error(format!(
                "Worker 초기화 실패: {error}"
            )));
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
                first_run: false,
                message,
            });
            return;
        }
    };
    // spawn_vod_history_sync uses tokio::spawn internally, so enter the runtime
    // while creating that long-lived task. The multi-thread runtime then keeps
    // driving it between GUI requests.
    runtime.block_on(async {
        core.spawn_vod_history_sync();
        core.spawn_queue_worker();
        core.spawn_auto_backup();
    });
    if responses
        .send(read_snapshot(
            &core,
            true,
            "canonical SQLite에서 설정을 불러왔습니다. 저장한 변경사항은 이후 런타임 작업부터 적용됩니다.",
        ))
        .is_err()
    {
        return;
    }
    if responses
        .send(configuration_snapshot(
            &core,
            "canonical SQLite에서 채널 및 서비스 연결 정보를 불러왔습니다.",
        ))
        .is_err()
    {
        return;
    }
    if responses.send(live_status(&core, &runtime, false)).is_err() {
        return;
    }
    if responses.send(vod_status(&core, &runtime, false)).is_err() {
        return;
    }
    if responses
        .send(queue_status(&core, &runtime, false))
        .is_err()
    {
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
            &core,
            &runtime,
            "공유 런타임 서비스에서 관리 상태를 불러왔습니다.",
        ))
        .is_err()
    {
        return;
    }

    for request in requests {
        let response = match request {
            Request::Refresh => read_snapshot(
                &core,
                false,
                "진단 정보를 새로고침했습니다. 저장하지 않은 편집 내용은 유지됩니다.",
            ),
            Request::Reload => {
                read_snapshot(&core, true, "저장된 설정을 다시 불러왔습니다. 편집 중이던 내용은 취소되었습니다.")
            }
            Request::Save(patch) => {
                match runtime.block_on(core.update_environment_settings(&patch)) {
                    Ok(_) => read_snapshot(
                        &core,
                        true,
                        "canonical SQLite에 저장했습니다. 진행 중인 녹화는 현재 설정을 그대로 유지합니다.",
                    ),
                    Err(error) => Response::Error(format!("저장 실패: {error:#}")),
                }
            }
            Request::Pick(index, kind, initial) => match native_picker::pick(kind, &initial) {
                Ok(path) => Response::Picked(index, path),
                Err(error) => Response::Error(format!("폴더 선택기 오류: {error}")),
            },
            Request::ConfigReload => configuration_snapshot(
                &core,
                "저장된 채널 및 공급자 인증 정보를 다시 불러왔습니다. 편집 중이던 내용은 취소되었습니다.",
            ),
            Request::ChannelsSave(channels) => save_channels(&core, &runtime, channels),
            Request::ChannelResolve {
                index,
                platform,
                account,
            } => match runtime.block_on(core.resolve_channel_name(platform, &account)) {
                Ok(name) => Response::ChannelResolved {
                    index,
                    account,
                    name,
                },
                Err(error) => Response::ConfigError(format!(
                    "채널 이름 조회 실패 {platform}/{account}: {error:#}"
                )),
            },
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
                        &core,
                        "서비스 연결 정보를 저장했습니다. 비밀 값은 암호화 상태로 유지되며 UI로 다시 읽어오지 않습니다.",
                    ),
                    Err(error) => Response::ConfigError(format!(
                        "서비스 연결 정보를 저장하지 못했습니다: {error:#}"
                    )),
                }
            }
            Request::ProviderTestSoop => match runtime.block_on(core.test_soop_auth()) {
                Ok(message) => Response::ConfigMessage(message),
                Err(error) => {
                    Response::ConfigError(format!("SOOP / Worker 테스트 실패: {error:#}"))
                }
            },
            Request::LiveStatus { poll } => live_status(&core, &runtime, poll),
            Request::LiveStart => match runtime.block_on(core.start_watcher()) {
                Ok(status) => Response::Live {
                    status,
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
                    status,
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
                            status,
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
                            status,
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
            Request::VodStatus { poll } => vod_status(&core, &runtime, poll),
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
            Request::VodPickOutput { initial } => match native_picker::pick_directory(&initial) {
                Ok(path) => Response::VodPicked(path),
                Err(error) => Response::VodError {
                    message: format!("VOD 출력 폴더 선택기 오류: {error}"),
                    poll: false,
                },
            },
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
            Request::QueueStatus { poll } => queue_status(&core, &runtime, poll),
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
                            message: format!("작업은 대기열에 추가됐지만 새로고침에 실패했습니다: {error:#}"),
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
                        message: Some(format!("대기열 작업 완료: {}", match action.as_str() { "cancel" => "취소", "retry" => "재시도", "remove" => "삭제", _ => action.as_str() })),
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
            Request::MaintenanceLoad => {
                maintenance_snapshot(&core, &runtime, "관리 상태를 새로고침했습니다")
            }
            Request::BackupPickDirectory { initial } => {
                match native_picker::pick_directory(&initial) {
                    Ok(path) => Response::MaintenancePicked(path),
                    Err(error) => Response::MaintenanceError {
                        message: format!("백업 폴더 선택기 오류: {error}"),
                        poll: false,
                    },
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
                        let _ = responses.send(read_snapshot(
                            &core,
                            true,
                            "DB를 복원하고 canonical SQLite에서 설정을 다시 불러왔습니다.",
                        ));
                        let _ = responses.send(configuration_snapshot(
                            &core,
                            "DB를 복원하고 채널 및 서비스 연결 정보를 다시 불러왔습니다.",
                        ));
                        let _ = responses.send(queue_status(&core, &runtime, false));
                        if let Ok(history) = core.history(&HistoryFilter::default()) {
                            let _ = responses.send(Response::History {
                                history,
                                view: "ALL".into(),
                                message: Some("복원 후 기록을 다시 불러왔습니다".into()),
                            });
                        }
                        maintenance_snapshot(
                            &core,
                            &runtime,
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
        };
        if responses.send(response).is_err() {
            break;
        }
    }

    runtime.block_on(core.shutdown());
}

fn localized_setting_description<'a>(key: &str, fallback: &'a str) -> &'a str {
    match key {
        "STREAMLINK_PATH" => "Streamlink 실행 파일 경로입니다. 비워두거나 AUTO를 사용하면 자동 탐색합니다.",
        "STREAMLINK_FALLBACK" => "대체 Streamlink 실행 파일 경로입니다. 비워두거나 AUTO를 사용하면 자동 탐색합니다.",
        "YT_DLP_PATH" => "yt-dlp 실행 파일 경로입니다. 비워두면 런타임에서 자동 탐색합니다.",
        "FFMPEG_PATH" => "FFmpeg 실행 파일 경로입니다. 비워두면 런타임에서 자동 탐색합니다.",
        "OUTPUT_DIR" => "LIVE 기본 저장 폴더입니다. 비워두면 런타임 기본값을 사용합니다.",
        "CHECK_INTERVAL" => "LIVE 상태 확인 간격(초)입니다. 허용 범위: 1~86400.",
        "MIN_FREE_SPACE_GB" => "최소 여유 디스크 공간(GB)입니다. 허용 범위: 0~1000000.",
        "QUALITY" => "LIVE 화질입니다. 예: best",
        _ => fallback,
    }
}

fn render_draft(ui: &MainWindow, draft: &SettingsDraft) {
    let rows: Vec<_> = draft
        .fields
        .iter()
        .map(|field| SettingRow {
            key: field.key.clone().into(),
            value: field.value.clone().into(),
            description: localized_setting_description(&field.key, &field.description).into(),
            pickable: field.kind != SettingKind::Text,
        })
        .collect();
    let state = ui.global::<AppState>();
    state.set_settings_rows(ModelRc::new(VecModel::from(rows)));
    state.set_settings_dirty(!draft.patch().is_empty());
}

fn render_channels(ui: &MainWindow, draft: &ChannelsDraft) {
    let rows: Vec<_> = draft
        .rows
        .iter()
        .map(|channel| ChannelConfigRow {
            platform: channel.platform.to_string().into(),
            enabled: channel.enabled,
            name: channel.name.clone().into(),
            account: channel.account.clone().into(),
            outdir: channel.outdir.clone().into(),
        })
        .collect();
    let state = ui.global::<AppState>();
    state.set_channel_config_rows(ModelRc::new(VecModel::from(rows)));
    state.set_channels_dirty(draft.dirty());
}

fn render_live(ui: &MainWindow, status: NativeWatcherStatus) {
    let view = live_adapter::view(status);
    let rows: Vec<_> = view
        .channels
        .into_iter()
        .map(|row| LiveChannelRow {
            target: row.target.into(),
            platform: row.platform.into(),
            name: row.name.into(),
            account: row.account.into(),
            status: row.status.into(),
            status_label: row.status_label.into(),
            status_tone: row.status_tone.into(),
            title: row.title.into(),
            bno: row.bno.into(),
            file: row.file.into(),
            size: row.size.into(),
            started_at: row.started_at.into(),
            suppressed: row.suppressed,
            detail: row.detail.into(),
            can_stop_once: row.can_stop_once,
            can_resume: row.can_resume,
            can_recheck: row.can_recheck,
            password_required: row.password_required,
        })
        .collect();
    let state = ui.global::<AppState>();
    state.set_live_running(view.running);
    state.set_live_state(view.state_label.into());
    state.set_live_engine(view.engine.into());
    state.set_live_started_at(view.started_at.into());
    state.set_live_channel_count(view.channel_count.into());
    state.set_live_recording_count(view.recording_count.into());
    state.set_live_offline_count(view.offline_count.into());
    state.set_live_error_count(view.error_count.into());
    state.set_live_rows(ModelRc::new(VecModel::from(rows)));
    state.set_live_loaded(true);
}

fn render_vod_draft(ui: &MainWindow, draft: &VodDraft) {
    let qualities = draft
        .qualities
        .iter()
        .map(|quality| VodQualityRow {
            value: quality.value.clone().into(),
            label: quality.label.clone().into(),
            selected: quality.selected,
        })
        .collect::<Vec<_>>();
    let parts = draft
        .parts
        .iter()
        .map(|part| VodPartRow {
            part: part.part.to_string().into(),
            duration: vod_adapter::format_duration(part.duration_seconds).into(),
            selected: part.selected,
        })
        .collect::<Vec<_>>();
    let state = ui.global::<AppState>();
    state.set_vod_url(draft.url.clone().into());
    state.set_vod_output_directory(draft.output_directory.clone().into());
    state.set_vod_merge(draft.merge);
    state.set_vod_quality_rows(ModelRc::new(VecModel::from(qualities)));
    state.set_vod_part_rows(ModelRc::new(VecModel::from(parts)));
    state.set_vod_can_analyze(draft.can_analyze());
    state.set_vod_can_download(draft.can_download());
}

fn render_vod_status(ui: &MainWindow, draft: &mut VodDraft, status: VodJobStatus) -> AnalysisSync {
    let sync = status
        .analysis
        .as_ref()
        .map(|analysis| draft.sync_analysis(analysis))
        .unwrap_or(AnalysisSync::AlreadyCurrent);
    if sync == AnalysisSync::Applied {
        render_vod_draft(ui, draft);
    }

    let view = vod_adapter::view(&status);
    let state = ui.global::<AppState>();
    state.set_vod_platform(view.platform.into());
    state.set_vod_state(view.state.into());
    state.set_vod_state_tone(view.state_tone.into());
    state.set_vod_running(view.running);
    state.set_vod_current_part(view.current_part.into());
    state.set_vod_part_count(view.part_count.into());
    state.set_vod_percent(view.percent);
    state.set_vod_percent_label(view.percent_label.into());
    state.set_vod_output_file(view.output_file.into());
    state.set_vod_started_at(view.started_at.into());
    state.set_vod_finished_at(view.finished_at.into());
    if sync == AnalysisSync::Stale {
        state.set_vod_title("".into());
        state.set_vod_streamer("".into());
        state.set_vod_streamer_id("".into());
    } else {
        state.set_vod_title(view.title.into());
        state.set_vod_streamer(view.streamer.into());
        state.set_vod_streamer_id(view.streamer_id.into());
    }
    state.set_vod_loaded(true);
    if !view.message.is_empty() {
        state.set_vod_runtime_message(view.message.into());
    }
    sync
}

fn render_queue(ui: &MainWindow, snapshot: VodQueueSnapshot) {
    let view = queue_adapter::view(snapshot);
    let rows = view
        .rows
        .into_iter()
        .map(|row| QueueDisplayRow {
            id: row.id.into(),
            platform: row.platform.into(),
            title: row.title.into(),
            streamer: row.streamer.into(),
            url: row.url.into(),
            state: row.state.into(),
            state_label: row.state_label.into(),
            state_tone: row.state_tone.into(),
            attempts: row.attempts.into(),
            message: row.message.into(),
            percent: row.percent,
            percent_label: row.percent_label.into(),
            part_progress: row.part_progress.into(),
            output_directory: row.output_directory.into(),
            output_file: row.output_file.into(),
            created_at: row.created_at.into(),
            started_at: row.started_at.into(),
            finished_at: row.finished_at.into(),
            can_cancel: row.can_cancel,
            can_retry: row.can_retry,
            can_remove: row.can_remove,
        })
        .collect::<Vec<_>>();
    let state = ui.global::<QueueHistoryState>();
    state.set_queue_rows(ModelRc::new(VecModel::from(rows)));
    state.set_queue_active_count(view.active.into());
    state.set_queue_queued_count(view.queued.into());
    state.set_queue_completed_count(view.completed.into());
    state.set_queue_failed_count(view.failed.into());
    state.set_queue_loaded(true);
}

fn render_history(ui: &MainWindow, history: HistoryResponse, view: &str) {
    let rows = history_adapter::rows(history, view)
        .into_iter()
        .map(|row| HistoryDisplayRow {
            kind: row.kind.into(),
            platform: row.platform.into(),
            title: row.title.into(),
            subject: row.subject.into(),
            state: row.state.into(),
            state_tone: row.state_tone.into(),
            detail: row.detail.into(),
            timing: row.timing.into(),
            file: row.file.into(),
            meta: row.meta.into(),
        })
        .collect::<Vec<_>>();
    let state = ui.global::<QueueHistoryState>();
    state.set_history_rows(ModelRc::new(VecModel::from(rows)));
    state.set_history_loaded(true);
}

fn render_history_calendar(
    ui: &MainWindow,
    target: &str,
    calendar: history_adapter::CalendarMonthView,
) {
    let rows = calendar
        .days
        .into_iter()
        .map(|day| HistoryCalendarDay {
            day: day.day.into(),
            date: day.date.into(),
            in_month: day.in_month,
            selected: day.selected,
        })
        .collect::<Vec<_>>();
    let state = ui.global::<QueueHistoryState>();
    state.set_history_calendar_target(target.into());
    state.set_history_calendar_year(calendar.year);
    state.set_history_calendar_month(calendar.month as i32);
    state.set_history_calendar_label(calendar.label.into());
    state.set_history_calendar_days(ModelRc::new(VecModel::from(rows)));
}

fn render_logs(ui: &MainWindow, lines: Vec<String>) {
    let rows = maintenance_adapter::log_rows(lines, 200)
        .into_iter()
        .map(|row| MaintenanceLogRow {
            text: row.text.into(),
        })
        .collect::<Vec<_>>();
    let state = ui.global::<MaintenanceState>();
    state.set_log_rows(ModelRc::new(VecModel::from(rows)));
    state.set_log_message("최근 런타임 로그를 표시합니다(최대 200줄).".into());
}

fn render_maintenance(
    ui: &MainWindow,
    snapshot: BackupSnapshot,
    diagnostics: DiagnosticsSnapshot,
    logs: Vec<String>,
) {
    let backup_rows = maintenance_adapter::backup_rows(&snapshot)
        .into_iter()
        .map(|row| MaintenanceBackupRow {
            file_name: row.file_name.into(),
            kind: row.kind.into(),
            created_at: row.created_at.into(),
            size: row.size.into(),
            sha256: row.sha256.into(),
            integrity: row.integrity.into(),
            integrity_tone: row.integrity_tone.into(),
            can_restore: row.can_restore,
        })
        .collect::<Vec<_>>();

    let diagnostic_rows = maintenance_adapter::diagnostic_rows(&diagnostics)
        .into_iter()
        .map(|row| MaintenanceDiagnosticRow {
            name: row.name.into(),
            status: row.status.into(),
            detail: row.detail.into(),
            status_tone: row.status_tone.into(),
        })
        .collect::<Vec<_>>();

    let state = ui.global::<MaintenanceState>();
    state.set_backup_directory(snapshot.directory.into());
    state.set_backup_directory_editable(snapshot.directory_editable);
    state.set_backup_enabled(snapshot.policy.enabled);
    state.set_backup_interval_hours(snapshot.policy.interval_hours.to_string().into());
    state.set_backup_keep_count(snapshot.policy.keep_count.to_string().into());
    state.set_backup_retention_days(snapshot.policy.retention_days.to_string().into());
    state.set_backup_rows(ModelRc::new(VecModel::from(backup_rows)));
    state.set_diagnostic_rows(ModelRc::new(VecModel::from(diagnostic_rows)));
    state.set_loaded(true);
    drop(state);
    render_logs(ui, logs);
}

pub fn bind_core_snapshot(ui: &MainWindow, diagnostics: DiagnosticsSnapshot) {
    let state = ui.global::<AppState>();
    state.set_runtime_ready(diagnostics.runtime_ready);
    state.set_runtime_status(format!("환경: {}", diagnostics.status.label()).into());
    let rows: Vec<_> = diagnostics
        .items
        .into_iter()
        .map(|item| DiagnosticRow {
            name: item.name.into(),
            status: item.status.label().into(),
            detail: item.detail.into(),
        })
        .collect();
    state.set_diagnostics_rows(ModelRc::new(VecModel::from(rows)));
}

fn send_settings(ui: &MainWindow, sender: &mpsc::Sender<Request>, request: Request) {
    let state = ui.global::<AppState>();
    if state.get_settings_busy() {
        return;
    }
    match sender.send(request) {
        Ok(()) => {
            state.set_settings_busy(true);
            state.set_settings_message("처리 중...".into());
        }
        Err(_) => state.set_settings_message(
            "런타임 Worker를 사용할 수 없습니다. 시작 오류를 해결한 뒤 다시 실행하세요.".into(),
        ),
    }
}

fn send_config(ui: &MainWindow, sender: &mpsc::Sender<Request>, request: Request) {
    let state = ui.global::<AppState>();
    if state.get_config_busy() {
        return;
    }
    match sender.send(request) {
        Ok(()) => {
            state.set_config_busy(true);
            state.set_config_message("처리 중...".into());
        }
        Err(_) => state.set_config_message("설정 Worker를 사용할 수 없습니다".into()),
    }
}

fn send_live(ui: &MainWindow, sender: &mpsc::Sender<Request>, request: Request) {
    let state = ui.global::<AppState>();
    if state.get_live_busy() {
        return;
    }
    match sender.send(request) {
        Ok(()) => {
            state.set_live_busy(true);
            state.set_live_message("처리 중...".into());
        }
        Err(_) => state.set_live_message("LIVE 런타임 Worker를 사용할 수 없습니다".into()),
    }
}

fn send_vod(ui: &MainWindow, sender: &mpsc::Sender<Request>, request: Request) {
    let state = ui.global::<AppState>();
    if state.get_vod_busy() {
        return;
    }
    match sender.send(request) {
        Ok(()) => {
            state.set_vod_busy(true);
            state.set_vod_message("처리 중...".into());
        }
        Err(_) => state.set_vod_message("VOD 런타임 Worker를 사용할 수 없습니다".into()),
    }
}

fn send_queue(ui: &MainWindow, sender: &mpsc::Sender<Request>, request: Request) {
    let state = ui.global::<QueueHistoryState>();
    if state.get_queue_busy() {
        return;
    }
    match sender.send(request) {
        Ok(()) => {
            state.set_queue_busy(true);
            state.set_queue_message("처리 중...".into());
        }
        Err(_) => state.set_queue_message("대기열 런타임 Worker를 사용할 수 없습니다".into()),
    }
}

fn send_history(ui: &MainWindow, sender: &mpsc::Sender<Request>, request: Request) {
    let state = ui.global::<QueueHistoryState>();
    if state.get_history_busy() {
        return;
    }
    match sender.send(request) {
        Ok(()) => {
            state.set_history_busy(true);
            state.set_history_message("기록을 불러오는 중...".into());
        }
        Err(_) => state.set_history_message("기록 런타임 Worker를 사용할 수 없습니다".into()),
    }
}

fn send_maintenance(ui: &MainWindow, sender: &mpsc::Sender<Request>, request: Request) {
    let state = ui.global::<MaintenanceState>();
    if state.get_busy() {
        return;
    }
    match sender.send(request) {
        Ok(()) => {
            state.set_busy(true);
            state.set_message("처리 중...".into());
        }
        Err(_) => state.set_message("관리 런타임 Worker를 사용할 수 없습니다".into()),
    }
}

pub fn bind(ui: &MainWindow) -> Controller {
    let (sender, requests) = mpsc::channel();
    let (responses, receiver) = mpsc::channel();
    let state = ui.global::<AppState>();
    state.set_settings_busy(true);
    state.set_config_busy(true);
    state.set_live_busy(true);
    state.set_vod_busy(true);
    let queue_history = ui.global::<QueueHistoryState>();
    queue_history.set_queue_busy(true);
    queue_history.set_history_busy(true);
    let maintenance = ui.global::<MaintenanceState>();
    maintenance.set_busy(true);
    if let Err(error) = std::thread::Builder::new()
        .name("native-runtime".into())
        .spawn(move || worker(requests, responses))
    {
        state.set_settings_busy(false);
        state.set_config_busy(false);
        state.set_live_busy(false);
        state.set_vod_busy(false);
        queue_history.set_queue_busy(false);
        queue_history.set_history_busy(false);
        maintenance.set_busy(false);
        queue_history.set_queue_message(format!("Worker를 시작할 수 없습니다: {error}").into());
        queue_history.set_history_message(format!("Worker를 시작할 수 없습니다: {error}").into());
        maintenance.set_message(format!("Worker를 시작할 수 없습니다: {error}").into());
        state.set_settings_message(format!("Worker를 시작할 수 없습니다: {error}").into());
        state.set_config_message(format!("Worker를 시작할 수 없습니다: {error}").into());
        state.set_live_message(format!("Worker를 시작할 수 없습니다: {error}").into());
        state.set_vod_message(format!("Worker를 시작할 수 없습니다: {error}").into());
    }

    let draft = Rc::new(RefCell::new(SettingsDraft::default()));
    let channels_draft = Rc::new(RefCell::new(ChannelsDraft::default()));
    let vod_draft = Rc::new(RefCell::new(VodDraft::default()));
    render_vod_draft(ui, &vod_draft.borrow());
    let live_poll_in_flight = Rc::new(Cell::new(false));
    let vod_poll_in_flight = Rc::new(Cell::new(false));
    let queue_poll_in_flight = Rc::new(Cell::new(false));
    let maintenance_log_poll_in_flight = Rc::new(Cell::new(false));

    let weak = ui.as_weak();
    let edit_draft = draft.clone();
    state.on_setting_edited(move |index, value| {
        if let Some(ui) = weak.upgrade() {
            if index >= 0 && !ui.global::<AppState>().get_settings_busy() {
                let mut draft = edit_draft.borrow_mut();
                draft.edit(index as usize, value.to_string());
                ui.global::<AppState>()
                    .set_settings_dirty(!draft.patch().is_empty());
            }
        }
    });

    let weak = ui.as_weak();
    let save_draft = draft.clone();
    let save_sender = sender.clone();
    state.on_save_settings(move || {
        if let Some(ui) = weak.upgrade() {
            send_settings(
                &ui,
                &save_sender,
                Request::Save(save_draft.borrow().patch()),
            );
        }
    });

    let weak = ui.as_weak();
    let reload_sender = sender.clone();
    state.on_reload_settings(move || {
        if let Some(ui) = weak.upgrade() {
            send_settings(&ui, &reload_sender, Request::Reload);
        }
    });

    let weak = ui.as_weak();
    let refresh_sender = sender.clone();
    state.on_refresh_requested(move || {
        if let Some(ui) = weak.upgrade() {
            send_settings(&ui, &refresh_sender, Request::Refresh);
        }
    });

    let weak = ui.as_weak();
    let pick_draft = draft.clone();
    let pick_sender = sender.clone();
    state.on_pick_setting(move |index| {
        if let Some(ui) = weak.upgrade() {
            if let Some(field) = usize::try_from(index)
                .ok()
                .and_then(|index| pick_draft.borrow().fields.get(index).cloned())
            {
                send_settings(
                    &ui,
                    &pick_sender,
                    Request::Pick(index as usize, field.kind, field.value),
                );
            }
        }
    });

    let weak = ui.as_weak();
    let add_draft = channels_draft.clone();
    state.on_channel_add(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.global::<AppState>().get_config_busy() {
                return;
            }
            add_draft.borrow_mut().add();
            render_channels(&ui, &add_draft.borrow());
            ui.global::<AppState>()
                .set_config_message("새 채널을 추가했습니다. 저장할 때 유효성을 확인합니다.".into());
        }
    });

    let weak = ui.as_weak();
    let remove_draft = channels_draft.clone();
    state.on_channel_remove(move |index| {
        if let Some(ui) = weak.upgrade() {
            if index < 0 || ui.global::<AppState>().get_config_busy() {
                return;
            }
            remove_draft.borrow_mut().remove(index as usize);
            render_channels(&ui, &remove_draft.borrow());
            ui.global::<AppState>()
                .set_config_message("채널을 편집 목록에서 삭제했습니다. 저장하면 반영됩니다.".into());
        }
    });

    let weak = ui.as_weak();
    let edit_channels = channels_draft.clone();
    state.on_channel_edited(move |index, field, value| {
        if let Some(ui) = weak.upgrade() {
            if index < 0 || ui.global::<AppState>().get_config_busy() {
                return;
            }
            edit_channels
                .borrow_mut()
                .edit(index as usize, field.as_str(), value.to_string());
            ui.global::<AppState>()
                .set_channels_dirty(edit_channels.borrow().dirty());
        }
    });

    let weak = ui.as_weak();
    let enabled_channels = channels_draft.clone();
    state.on_channel_toggle_enabled(move |index| {
        if let Some(ui) = weak.upgrade() {
            if index < 0 || ui.global::<AppState>().get_config_busy() {
                return;
            }
            let index = index as usize;
            let enabled = enabled_channels
                .borrow()
                .rows
                .get(index)
                .map(|channel| !channel.enabled);
            if let Some(enabled) = enabled {
                enabled_channels.borrow_mut().set_enabled(index, enabled);
                render_channels(&ui, &enabled_channels.borrow());
            }
        }
    });

    let weak = ui.as_weak();
    let platform_channels = channels_draft.clone();
    state.on_channel_toggle_platform(move |index| {
        if let Some(ui) = weak.upgrade() {
            if index < 0 || ui.global::<AppState>().get_config_busy() {
                return;
            }
            platform_channels
                .borrow_mut()
                .toggle_platform(index as usize);
            render_channels(&ui, &platform_channels.borrow());
        }
    });

    let weak = ui.as_weak();
    let resolve_channels = channels_draft.clone();
    let resolve_sender = sender.clone();
    state.on_channel_resolve(move |index| {
        if let Some(ui) = weak.upgrade() {
            if index < 0 || ui.global::<AppState>().get_config_busy() {
                return;
            }
            let index = index as usize;
            let channel = resolve_channels.borrow().rows.get(index).cloned();
            let Some(channel) = channel else {
                return;
            };
            if channel.account.trim().is_empty() {
                ui.global::<AppState>()
                    .set_config_message("이름을 조회하려면 계정 / 채널 ID를 먼저 입력하세요.".into());
                return;
            }
            send_config(
                &ui,
                &resolve_sender,
                Request::ChannelResolve {
                    index,
                    platform: channel.platform,
                    account: channel.account,
                },
            );
        }
    });

    let weak = ui.as_weak();
    let save_channels = channels_draft.clone();
    let config_sender = sender.clone();
    state.on_save_channels(move || {
        if let Some(ui) = weak.upgrade() {
            send_config(
                &ui,
                &config_sender,
                Request::ChannelsSave(save_channels.borrow().snapshot()),
            );
        }
    });

    let weak = ui.as_weak();
    let config_sender = sender.clone();
    state.on_reload_configuration(move || {
        if let Some(ui) = weak.upgrade() {
            send_config(&ui, &config_sender, Request::ConfigReload);
        }
    });

    let weak = ui.as_weak();
    let provider_sender = sender.clone();
    state.on_save_provider(move || {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<AppState>();
            let secrets = BTreeMap::from([
                (
                    "SOOP_PASSWORD".into(),
                    state.get_soop_password_draft().to_string(),
                ),
                (
                    "CLOUDFLARE_API_KEY".into(),
                    state.get_cloudflare_key_draft().to_string(),
                ),
                (
                    "CHZZK_NID_AUT".into(),
                    state.get_chzzk_nid_aut_draft().to_string(),
                ),
                (
                    "CHZZK_NID_SES".into(),
                    state.get_chzzk_nid_ses_draft().to_string(),
                ),
            ]);
            send_config(
                &ui,
                &provider_sender,
                Request::ProviderSave {
                    username: state.get_soop_username().to_string(),
                    worker_url: state.get_cloudflare_worker_url().to_string(),
                    secrets,
                },
            );
        }
    });

    let weak = ui.as_weak();
    let provider_sender = sender.clone();
    state.on_test_soop_auth(move || {
        if let Some(ui) = weak.upgrade() {
            send_config(&ui, &provider_sender, Request::ProviderTestSoop);
        }
    });

    let weak = ui.as_weak();
    let live_sender = sender.clone();
    state.on_live_start(move || {
        if let Some(ui) = weak.upgrade() {
            send_live(&ui, &live_sender, Request::LiveStart);
        }
    });

    let weak = ui.as_weak();
    let live_sender = sender.clone();
    state.on_live_stop(move || {
        if let Some(ui) = weak.upgrade() {
            send_live(&ui, &live_sender, Request::LiveStop);
        }
    });

    let weak = ui.as_weak();
    let live_sender = sender.clone();
    state.on_live_refresh(move || {
        if let Some(ui) = weak.upgrade() {
            send_live(&ui, &live_sender, Request::LiveStatus { poll: false });
        }
    });

    let weak = ui.as_weak();
    let live_sender = sender.clone();
    state.on_live_action(move |target, action| {
        if let Some(ui) = weak.upgrade() {
            send_live(
                &ui,
                &live_sender,
                Request::LiveAction {
                    target: target.to_string(),
                    action: action.to_string(),
                },
            );
        }
    });

    let weak = ui.as_weak();
    let live_sender = sender.clone();
    state.on_live_password_submit(move |target, password| {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<AppState>();
            state.set_live_password_target("".into());
            state.set_live_password_label("".into());
            state.set_live_password_draft("".into());
            if password.is_empty() {
                state.set_live_message("비밀번호가 비어 있습니다".into());
                return;
            }
            send_live(
                &ui,
                &live_sender,
                Request::LivePassword {
                    target: target.to_string(),
                    password: password.to_string(),
                },
            );
        }
    });

    let weak = ui.as_weak();
    let edit_vod = vod_draft.clone();
    state.on_vod_url_edited(move |value| {
        if let Some(ui) = weak.upgrade() {
            if ui.global::<AppState>().get_vod_busy() {
                return;
            }
            edit_vod.borrow_mut().edit_url(value.to_string());
            render_vod_draft(&ui, &edit_vod.borrow());
            ui.global::<AppState>()
                .set_vod_message("URL이 변경되었습니다. 분석을 눌러 메타데이터와 형식을 불러오세요.".into());
        }
    });

    let weak = ui.as_weak();
    let edit_vod = vod_draft.clone();
    state.on_vod_output_edited(move |value| {
        if let Some(ui) = weak.upgrade() {
            if ui.global::<AppState>().get_vod_busy() {
                return;
            }
            edit_vod
                .borrow_mut()
                .edit_output_directory(value.to_string());
            render_vod_draft(&ui, &edit_vod.borrow());
        }
    });

    let weak = ui.as_weak();
    let analyze_draft = vod_draft.clone();
    let vod_sender = sender.clone();
    state.on_vod_analyze(move || {
        if let Some(ui) = weak.upgrade() {
            let url = analyze_draft.borrow().url.trim().to_string();
            if url.is_empty() {
                ui.global::<AppState>()
                    .set_vod_message("SOOP 또는 CHZZK VOD URL을 먼저 입력하세요.".into());
                return;
            }
            send_vod(&ui, &vod_sender, Request::VodAnalyze { url });
        }
    });

    let weak = ui.as_weak();
    let output_draft = vod_draft.clone();
    let vod_sender = sender.clone();
    state.on_vod_pick_output(move || {
        if let Some(ui) = weak.upgrade() {
            let initial = output_draft.borrow().output_directory.clone();
            send_vod(&ui, &vod_sender, Request::VodPickOutput { initial });
        }
    });

    let weak = ui.as_weak();
    let quality_draft = vod_draft.clone();
    state.on_vod_select_quality(move |index| {
        if let Some(ui) = weak.upgrade() {
            if index < 0 || ui.global::<AppState>().get_vod_busy() {
                return;
            }
            quality_draft.borrow_mut().select_quality(index as usize);
            render_vod_draft(&ui, &quality_draft.borrow());
        }
    });

    let weak = ui.as_weak();
    let part_draft = vod_draft.clone();
    state.on_vod_toggle_part(move |index| {
        if let Some(ui) = weak.upgrade() {
            if index < 0 || ui.global::<AppState>().get_vod_busy() {
                return;
            }
            part_draft.borrow_mut().toggle_part(index as usize);
            render_vod_draft(&ui, &part_draft.borrow());
        }
    });

    let weak = ui.as_weak();
    let part_draft = vod_draft.clone();
    state.on_vod_select_all_parts(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.global::<AppState>().get_vod_busy() {
                return;
            }
            part_draft.borrow_mut().select_all_parts();
            render_vod_draft(&ui, &part_draft.borrow());
        }
    });

    let weak = ui.as_weak();
    let part_draft = vod_draft.clone();
    state.on_vod_clear_parts(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.global::<AppState>().get_vod_busy() {
                return;
            }
            part_draft.borrow_mut().clear_parts();
            render_vod_draft(&ui, &part_draft.borrow());
        }
    });

    let weak = ui.as_weak();
    let merge_draft = vod_draft.clone();
    state.on_vod_toggle_merge(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.global::<AppState>().get_vod_busy() {
                return;
            }
            merge_draft.borrow_mut().toggle_merge();
            render_vod_draft(&ui, &merge_draft.borrow());
        }
    });

    let weak = ui.as_weak();
    let download_draft = vod_draft.clone();
    let vod_sender = sender.clone();
    state.on_vod_download(move || {
        if let Some(ui) = weak.upgrade() {
            let Some(request) = download_draft.borrow().download_request() else {
                ui.global::<AppState>().set_vod_message(
                    "URL을 분석한 뒤 화질/PART를 선택하고 출력 폴더를 지정하세요."
                        .into(),
                );
                return;
            };
            send_vod(&ui, &vod_sender, Request::VodDownload(request));
        }
    });

    let weak = ui.as_weak();
    let vod_sender = sender.clone();
    state.on_vod_cancel(move || {
        if let Some(ui) = weak.upgrade() {
            send_vod(&ui, &vod_sender, Request::VodCancel);
        }
    });

    let weak = ui.as_weak();
    let vod_sender = sender.clone();
    state.on_vod_refresh(move || {
        if let Some(ui) = weak.upgrade() {
            send_vod(&ui, &vod_sender, Request::VodStatus { poll: false });
        }
    });

    let queue_state = ui.global::<QueueHistoryState>();

    let weak = ui.as_weak();
    let queue_sender = sender.clone();
    queue_state.on_queue_refresh(move || {
        if let Some(ui) = weak.upgrade() {
            send_queue(&ui, &queue_sender, Request::QueueStatus { poll: false });
        }
    });

    let weak = ui.as_weak();
    let queue_sender = sender.clone();
    queue_state.on_queue_action(move |id, action| {
        if let Some(ui) = weak.upgrade() {
            send_queue(
                &ui,
                &queue_sender,
                Request::QueueAction {
                    id: id.to_string(),
                    action: action.to_string(),
                },
            );
        }
    });

    let weak = ui.as_weak();
    let queue_sender = sender.clone();
    let enqueue_draft = vod_draft.clone();
    queue_state.on_enqueue_current(move || {
          if let Some(ui) = weak.upgrade() {
    let Some(request) = enqueue_draft.borrow().download_request() else {
        ui.global::<QueueHistoryState>().set_queue_message(
            "VOD를 분석한 뒤 화질/PART와 출력 폴더를 지정하고 대기열에 추가하세요."
                .into(),
        );
        return;
    };
    send_queue(&ui, &queue_sender, Request::QueueEnqueue(request));
          }
      });

    let weak = ui.as_weak();
    queue_state.on_history_calendar_open(move |target| {
        if let Some(ui) = weak.upgrade() {
            let target = target.to_string();
            let state = ui.global::<QueueHistoryState>();
            let selected = if target == "to" {
                state.get_history_to_date().to_string()
            } else {
                state.get_history_from_date().to_string()
            };
            drop(state);
            render_history_calendar(
                &ui,
                if target == "to" { "to" } else { "from" },
                history_adapter::calendar_initial(&selected),
            );
        }
    });

    let weak = ui.as_weak();
    queue_state.on_history_calendar_shift(move |delta| {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<QueueHistoryState>();
            let target = state.get_history_calendar_target().to_string();
            let year = state.get_history_calendar_year();
            let month = state.get_history_calendar_month().max(1) as u32;
            let selected = if target == "to" {
                state.get_history_to_date().to_string()
            } else {
                state.get_history_from_date().to_string()
            };
            drop(state);
            render_history_calendar(
                &ui,
                &target,
                history_adapter::calendar_shift(year, month, delta, &selected),
            );
        }
    });

    let weak = ui.as_weak();
    queue_state.on_history_calendar_select(move |date| {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<QueueHistoryState>();
            if state.get_history_calendar_target().as_str() == "to" {
                state.set_history_to_date(date);
            } else {
                state.set_history_from_date(date);
            }
        }
    });

    let weak = ui.as_weak();
    queue_state.on_history_calendar_clear(move || {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<QueueHistoryState>();
            if state.get_history_calendar_target().as_str() == "to" {
                state.set_history_to_date("".into());
            } else {
                state.set_history_from_date("".into());
            }
        }
    });

    let weak = ui.as_weak();
    let history_sender = sender.clone();
    queue_state.on_history_refresh(move || {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<QueueHistoryState>();
            let limit_text = state.get_history_limit().to_string();
            let limit = if limit_text.trim().is_empty() {
                None
            } else {
                match limit_text.trim().parse::<usize>() {
                    Ok(limit) => Some(limit),
                    Err(_) => {
                        state.set_history_message(
                            "최대 조회 건수는 1 이상의 정수여야 합니다.".into(),
                        );
                        return;
                    }
                }
            };
            let optional = |value: slint::SharedString| {
                let value = value.to_string();
                (!value.trim().is_empty()).then_some(value)
            };
            let filter = HistoryFilter {
                q: optional(state.get_history_search()),
                status: optional(state.get_history_status()),
                from: optional(state.get_history_from_date()),
                to: optional(state.get_history_to_date()),
                limit,
            };
            let view = state.get_history_view().to_string();
            drop(state);
            send_history(&ui, &history_sender, Request::HistoryLoad { filter, view });
        }
    });

    let maintenance_state = ui.global::<MaintenanceState>();

    let weak = ui.as_weak();
    let maintenance_sender = sender.clone();
    maintenance_state.on_refresh(move || {
        if let Some(ui) = weak.upgrade() {
            send_maintenance(&ui, &maintenance_sender, Request::MaintenanceLoad);
        }
    });

    let weak = ui.as_weak();
    let maintenance_sender = sender.clone();
    maintenance_state.on_pick_backup_directory(move || {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<MaintenanceState>();
            let initial = state.get_backup_directory().to_string();
            drop(state);
            send_maintenance(
                &ui,
                &maintenance_sender,
                Request::BackupPickDirectory { initial },
            );
        }
    });

    let weak = ui.as_weak();
    maintenance_state.on_toggle_backup_enabled(move || {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<MaintenanceState>();
            if !state.get_busy() {
                state.set_backup_enabled(!state.get_backup_enabled());
                state.set_message("백업 정책이 변경되었습니다. 정책 저장을 눌러 반영하세요.".into());
            }
        }
    });

    let weak = ui.as_weak();
    let maintenance_sender = sender.clone();
    maintenance_state.on_save_backup_policy(move || {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<MaintenanceState>();
            if state.get_busy() {
                return;
            }

            let interval = match state
                .get_backup_interval_hours()
                .to_string()
                .trim()
                .parse::<u64>()
            {
                Ok(value) => value,
                Err(_) => {
                    state.set_message("백업 간격은 시간 단위 정수여야 합니다.".into());
                    return;
                }
            };
            let keep_count = match state
                .get_backup_keep_count()
                .to_string()
                .trim()
                .parse::<usize>()
            {
                Ok(value) => value,
                Err(_) => {
                    state.set_message("백업 보관 개수는 0 이상의 정수여야 합니다.".into());
                    return;
                }
            };
            let retention_days = match state
                .get_backup_retention_days()
                .to_string()
                .trim()
                .parse::<i64>()
            {
                Ok(value) => value,
                Err(_) => {
                    state.set_message(
                        "백업 보관 기간은 0일 이상의 정수여야 합니다.".into(),
                    );
                    return;
                }
            };

            let directory = state
                .get_backup_directory_editable()
                .then(|| state.get_backup_directory().to_string());
            let policy = BackupPolicy {
                enabled: state.get_backup_enabled(),
                interval_hours: interval,
                keep_count,
                retention_days,
            };
            drop(state);
            send_maintenance(
                &ui,
                &maintenance_sender,
                Request::BackupSave { policy, directory },
            );
        }
    });

    let weak = ui.as_weak();
    let maintenance_sender = sender.clone();
    maintenance_state.on_create_backup(move || {
        if let Some(ui) = weak.upgrade() {
            send_maintenance(&ui, &maintenance_sender, Request::BackupCreate);
        }
    });

    let weak = ui.as_weak();
    maintenance_state.on_request_restore(move |file_name| {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<MaintenanceState>();
            if !state.get_busy() {
                state.set_restore_pending_file(file_name);
                state.set_message(
                    "계속하려면 복원 확인을 누르세요. 먼저 pre_restore 안전 백업을 생성합니다."
                        .into(),
                );
            }
        }
    });

    let weak = ui.as_weak();
    maintenance_state.on_cancel_restore(move || {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<MaintenanceState>();
            if !state.get_busy() {
                state.set_restore_pending_file("".into());
                state.set_message("복원을 취소했습니다. 변경된 데이터는 없습니다.".into());
            }
        }
    });

    let weak = ui.as_weak();
    let maintenance_sender = sender.clone();
    maintenance_state.on_confirm_restore(move || {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<MaintenanceState>();
            let file_name = state.get_restore_pending_file().to_string();
            if file_name.trim().is_empty() {
                state.set_message("복원할 유효한 백업을 선택하세요.".into());
                return;
            }
            drop(state);
            send_maintenance(
                &ui,
                &maintenance_sender,
                Request::BackupRestore { file_name },
            );
        }
    });

    let weak = ui.as_weak();
    let maintenance_sender = sender.clone();
    maintenance_state.on_refresh_logs(move || {
        if let Some(ui) = weak.upgrade() {
            send_maintenance(&ui, &maintenance_sender, Request::LogsLoad { poll: false });
        }
    });

    let weak = ui.as_weak();
    maintenance_state.on_toggle_log_auto_refresh(move || {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<MaintenanceState>();
            if !state.get_busy() {
                state.set_log_auto_refresh(!state.get_log_auto_refresh());
            }
        }
    });

    let weak = ui.as_weak();
    let response_live_poll_flag = live_poll_in_flight.clone();
    let response_vod_poll_flag = vod_poll_in_flight.clone();
    let response_queue_poll_flag = queue_poll_in_flight.clone();
    let response_maintenance_log_poll_flag = maintenance_log_poll_in_flight.clone();
    let response_channels = channels_draft.clone();
    let response_vod_draft = vod_draft.clone();
    let response_timer = Timer::default();
    response_timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        while let Ok(response) = receiver.try_recv() {
            let state = ui.global::<AppState>();
            match response {
                Response::Snapshot {
                    fields,
                    diagnostics,
                    first_run,
                    message,
                } => {
                    state.set_settings_busy(false);
                    let has_settings_snapshot = fields.is_some();
                    let startup_failed = !state.get_live_loaded() && fields.is_none();
                    if let Some(fields) = fields {
                        draft.borrow_mut().load(fields);
                        render_draft(&ui, &draft.borrow());
                        state.set_settings_loaded(true);
                    }
                    bind_core_snapshot(&ui, diagnostics);
                    if first_run && has_settings_snapshot && state.get_active_page() == "LIVE" {
                        state.set_active_page("Settings".into());
                    }
                    state.set_settings_message(message.clone().into());
                    if startup_failed {
                        state.set_config_busy(false);
                        state.set_config_message(message.clone().into());
                        state.set_live_busy(false);
                        state.set_live_message(message.clone().into());
                        state.set_vod_busy(false);
                        state.set_vod_message(message.clone().into());
                        let maintenance_state = ui.global::<MaintenanceState>();
                        maintenance_state.set_busy(false);
                        maintenance_state.set_message(message.into());
                    }
                }
                Response::Configuration {
                    channels,
                    username,
                    worker_url,
                    secrets,
                    message,
                } => {
                    state.set_config_busy(false);
                    response_channels.borrow_mut().load(channels);
                    render_channels(&ui, &response_channels.borrow());
                    state.set_soop_username(username.into());
                    state.set_cloudflare_worker_url(worker_url.into());
                    state.set_soop_password_configured(
                        secrets.get("SOOP_PASSWORD").copied().unwrap_or(false),
                    );
                    state.set_cloudflare_key_configured(
                        secrets.get("CLOUDFLARE_API_KEY").copied().unwrap_or(false),
                    );
                    state.set_chzzk_nid_aut_configured(
                        secrets.get("CHZZK_NID_AUT").copied().unwrap_or(false),
                    );
                    state.set_chzzk_nid_ses_configured(
                        secrets.get("CHZZK_NID_SES").copied().unwrap_or(false),
                    );
                    state.set_soop_password_draft("".into());
                    state.set_cloudflare_key_draft("".into());
                    state.set_chzzk_nid_aut_draft("".into());
                    state.set_chzzk_nid_ses_draft("".into());
                    state.set_config_loaded(true);
                    state.set_config_message(message.into());
                }
                Response::ChannelResolved {
                    index,
                    account,
                    name,
                } => {
                    state.set_config_busy(false);
                    let applied = {
                        let mut channels = response_channels.borrow_mut();
                        if channels
                            .rows
                            .get(index)
                            .is_some_and(|channel| channel.account == account)
                        {
                            channels.edit(index, "name", name.clone());
                            true
                        } else {
                            false
                        }
                    };
                    if applied {
                        render_channels(&ui, &response_channels.borrow());
                        state.set_config_message(format!("채널 이름 조회 완료: {name}").into());
                    } else {
                        state.set_config_message(
                            "조회 중 채널 정보가 변경되어 결과를 적용하지 않았습니다."
                                .into(),
                        );
                    }
                }
                Response::ConfigMessage(message) => {
                    state.set_config_busy(false);
                    state.set_config_message(message.into());
                }
                Response::ConfigError(message) => {
                    state.set_config_busy(false);
                    state.set_config_message(message.into());
                }
                Response::Picked(index, path) => {
                    state.set_settings_busy(false);
                    if draft.borrow_mut().accept_selection(index, path) {
                        render_draft(&ui, &draft.borrow());
                        state.set_settings_message(
                            "선택한 경로를 편집 내용에 반영했습니다. 저장 시 유효성을 확인하고 적용합니다.".into(),
                        );
                    } else {
                        state.set_settings_message("경로 선택을 취소했습니다. 편집 내용은 변경되지 않았습니다.".into());
                    }
                }
                Response::Live {
                    status,
                    message,
                    poll,
                } => {
                    if poll {
                        response_live_poll_flag.set(false);
                    } else {
                        state.set_live_busy(false);
                    }
                    render_live(&ui, status);
                    if let Some(message) = message {
                        state.set_live_message(message.into());
                    } else if !poll {
                        state.set_live_message("LIVE 상태를 새로고침했습니다".into());
                    }
                }
                Response::LiveError { message, poll } => {
                    if poll {
                        response_live_poll_flag.set(false);
                    } else {
                        state.set_live_busy(false);
                    }
                    state.set_live_message(message.into());
                }
                Response::Vod {
                    status,
                    message,
                    poll,
                } => {
                    if poll {
                        response_vod_poll_flag.set(false);
                    } else {
                        state.set_vod_busy(false);
                    }
                    let sync = render_vod_status(&ui, &mut response_vod_draft.borrow_mut(), status);
                    if sync == AnalysisSync::Stale {
                        state.set_vod_message(
                            "이전 URL의 분석 결과는 무시했습니다. 현재 URL을 다시 분석하세요."
                                .into(),
                        );
                    } else if let Some(message) = message {
                        state.set_vod_message(message.into());
                    } else if !poll {
                        state.set_vod_message("VOD 상태를 새로고침했습니다".into());
                    }
                }
                Response::VodPicked(path) => {
                    state.set_vod_busy(false);
                    if response_vod_draft
                        .borrow_mut()
                        .accept_output_selection(path)
                    {
                        render_vod_draft(&ui, &response_vod_draft.borrow());
                        state.set_vod_message("출력 폴더를 선택했습니다.".into());
                    } else {
                        state.set_vod_message("출력 폴더 선택을 취소했습니다.".into());
                    }
                }
                Response::VodError { message, poll } => {
                    if poll {
                        response_vod_poll_flag.set(false);
                    } else {
                        state.set_vod_busy(false);
                    }
                    state.set_vod_message(message.into());
                }
                Response::Queue {
                    snapshot,
                    message,
                    poll,
                } => {
                    let queue_state = ui.global::<QueueHistoryState>();
                    if poll {
                        response_queue_poll_flag.set(false);
                    } else {
                        queue_state.set_queue_busy(false);
                    }
                    render_queue(&ui, snapshot);
                    if let Some(message) = message {
                        queue_state.set_queue_message(message.into());
                    } else if !poll {
                        queue_state.set_queue_message("대기열을 새로고침했습니다".into());
                    }
                }
                Response::QueueError { message, poll } => {
                    let queue_state = ui.global::<QueueHistoryState>();
                    if poll {
                        response_queue_poll_flag.set(false);
                    } else {
                        queue_state.set_queue_busy(false);
                    }
                    queue_state.set_queue_message(message.into());
                }
                Response::History {
                    history,
                    view,
                    message,
                } => {
                    let history_state = ui.global::<QueueHistoryState>();
                    history_state.set_history_busy(false);
                    history_state.set_history_view(view.clone().into());
                    render_history(&ui, history, &view);
                    history_state.set_history_message(
                        message.unwrap_or_else(|| "기록을 새로고침했습니다".into()).into(),
                    );
                }
                Response::HistoryError(message) => {
                    let history_state = ui.global::<QueueHistoryState>();
                    history_state.set_history_busy(false);
                    history_state.set_history_message(message.into());
                }
                Response::Maintenance {
                    snapshot,
                    diagnostics,
                    logs,
                    message,
                } => {
                    let maintenance_state = ui.global::<MaintenanceState>();
                    maintenance_state.set_busy(false);
                    if message.starts_with("복원 완료:") {
                        maintenance_state.set_restore_pending_file("".into());
                    }
                    render_maintenance(&ui, snapshot, diagnostics, logs);
                    maintenance_state.set_message(message.into());
                }
                Response::MaintenancePicked(path) => {
                    let maintenance_state = ui.global::<MaintenanceState>();
                    maintenance_state.set_busy(false);
                    if let Some(path) = path {
                        maintenance_state.set_backup_directory(path.into());
                        maintenance_state.set_message(
                            "백업 폴더가 변경되었습니다. 정책 저장을 눌러 반영하세요.".into(),
                        );
                    } else {
                        maintenance_state.set_message(
                            "백업 폴더 선택을 취소했습니다. 기존 값을 유지합니다.".into(),
                        );
                    }
                }
                Response::Logs { lines, poll } => {
                    let maintenance_state = ui.global::<MaintenanceState>();
                    if poll {
                        response_maintenance_log_poll_flag.set(false);
                    } else {
                        maintenance_state.set_busy(false);
                        maintenance_state.set_message("런타임 로그를 새로고침했습니다.".into());
                    }
                    render_logs(&ui, lines);
                }
                Response::MaintenanceError { message, poll } => {
                    let maintenance_state = ui.global::<MaintenanceState>();
                    if poll {
                        response_maintenance_log_poll_flag.set(false);
                    } else {
                        maintenance_state.set_busy(false);
                    }
                    maintenance_state.set_message(message.into());
                }

                Response::Error(message) => {
                    state.set_settings_busy(false);
                    if !state.get_config_loaded() {
                        state.set_config_busy(false);
                        state.set_config_message(message.clone().into());
                    }
                    if !state.get_live_loaded() {
                        state.set_live_busy(false);
                        state.set_live_message(message.clone().into());
                    }
                    if !state.get_vod_loaded() {
                        state.set_vod_busy(false);
                        state.set_vod_message(message.clone().into());
                    }
                    let queue_state = ui.global::<QueueHistoryState>();
                    if !queue_state.get_queue_loaded() {
                        queue_state.set_queue_busy(false);
                        queue_state.set_queue_message(message.clone().into());
                    }
                    if !queue_state.get_history_loaded() {
                        queue_state.set_history_busy(false);
                        queue_state.set_history_message(message.clone().into());
                    }
                    let maintenance_state = ui.global::<MaintenanceState>();
                    if !maintenance_state.get_loaded() {
                        maintenance_state.set_busy(false);
                        maintenance_state.set_message(message.clone().into());
                    }
                    state.set_settings_message(message.into());
                }
            }
        }
    });

    let weak = ui.as_weak();
    let live_poll_sender = sender.clone();
    let live_poll_flag = live_poll_in_flight;
    let live_poll_timer = Timer::default();
    live_poll_timer.start(
        TimerMode::Repeated,
        Duration::from_millis(1500),
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let state = ui.global::<AppState>();
            if state.get_active_page().as_str() != "LIVE"
                || state.get_live_busy()
                || live_poll_flag.get()
            {
                return;
            }
            if live_poll_sender
                .send(Request::LiveStatus { poll: true })
                .is_ok()
            {
                live_poll_flag.set(true);
            }
        },
    );

    let weak = ui.as_weak();
    let vod_poll_sender = sender.clone();
    let vod_poll_flag = vod_poll_in_flight;
    let vod_poll_timer = Timer::default();
    vod_poll_timer.start(
        TimerMode::Repeated,
        Duration::from_millis(1000),
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let state = ui.global::<AppState>();
            if state.get_active_page().as_str() != "VOD"
                || state.get_vod_busy()
                || vod_poll_flag.get()
            {
                return;
            }
            if vod_poll_sender
                .send(Request::VodStatus { poll: true })
                .is_ok()
            {
                vod_poll_flag.set(true);
            }
        },
    );

    let weak = ui.as_weak();
    let queue_poll_sender = sender.clone();
    let queue_poll_flag = queue_poll_in_flight;
    let queue_poll_timer = Timer::default();
    queue_poll_timer.start(
        TimerMode::Repeated,
        Duration::from_millis(1000),
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let app_state = ui.global::<AppState>();
            let queue_state = ui.global::<QueueHistoryState>();
            if app_state.get_active_page().as_str() != "Queue"
                || queue_state.get_queue_busy()
                || queue_poll_flag.get()
            {
                return;
            }
            if queue_poll_sender
                .send(Request::QueueStatus { poll: true })
                .is_ok()
            {
                queue_poll_flag.set(true);
            }
        },
    );

    let weak = ui.as_weak();
    let maintenance_log_poll_sender = sender;
    let maintenance_log_poll_flag = maintenance_log_poll_in_flight;
    let maintenance_log_poll_timer = Timer::default();
    maintenance_log_poll_timer.start(
        TimerMode::Repeated,
        Duration::from_millis(1500),
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let app_state = ui.global::<AppState>();
            let maintenance_state = ui.global::<MaintenanceState>();
            if app_state.get_active_page().as_str() != "Maintenance"
                || maintenance_state.get_section().as_str() != "Logs"
                || !maintenance_state.get_log_auto_refresh()
                || maintenance_state.get_busy()
                || maintenance_log_poll_flag.get()
            {
                return;
            }
            if maintenance_log_poll_sender
                .send(Request::LogsLoad { poll: true })
                .is_ok()
            {
                maintenance_log_poll_flag.set(true);
            }
        },
    );

    Controller {
        _response_timer: response_timer,
        _live_poll_timer: live_poll_timer,
        _vod_poll_timer: vod_poll_timer,
        _queue_poll_timer: queue_poll_timer,
        _maintenance_log_poll_timer: maintenance_log_poll_timer,
    }
}
