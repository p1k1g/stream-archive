//! Request dispatch handlers executed on `core_worker`'s dedicated worker thread.

use super::{
    Request, Response,
    snapshots::{
        configuration_snapshot, desktop_snapshot, live_status_async, maintenance_snapshot,
        queue_status, read_active_snapshot, read_snapshot, save_channels, selected_parts_label,
        storage_status, vod_analyze_request, vod_status,
    },
    spawn_detached_task,
};
use crate::{live_adapter, native_picker, native_shell};
use std::{collections::BTreeMap, sync::mpsc};
use stream_archive_server::{
    app_core::StreamArchiveCore,
    backup_service::BackupPolicy,
    environment_settings::SettingKind,
    history_service::HistoryFilter,
    model::{Channel, VodDownloadRequest},
    support::platform::PlatformId,
};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum LoopAction {
    Continue,
    Stop,
}

fn send_response(responses: &mpsc::Sender<Response>, response: Response) -> LoopAction {
    if responses.send(response).is_err() {
        LoopAction::Stop
    } else {
        LoopAction::Continue
    }
}

pub(super) fn handle_request(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    responses: &mpsc::Sender<Response>,
    request: Request,
) -> LoopAction {
    let response = match request {
        Request::Shutdown => return LoopAction::Stop,
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
            return LoopAction::Continue;
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
            return LoopAction::Continue;
        }
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
            return LoopAction::Continue;
        }
        Request::PrepareExit => Response::ExitCheck(
            runtime
                .block_on(desktop_snapshot(core))
                .map(|(_, active)| active),
        ),
        Request::SaveWindowBounds(spec) => {
            let _ = core.save_window_bounds(&spec);
            return LoopAction::Continue;
        }
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
        Request::Save(patch) => handle_save_settings(core, runtime, responses, patch),
        Request::Pick(index, kind, initial) => {
            return handle_pick_setting(responses, index, kind, initial);
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
        } => return handle_channel_pick_output(responses, index, expected, initial),
        Request::ChannelResolve {
            index,
            platform,
            account,
        } => return handle_channel_resolve(core, runtime, responses, index, platform, account),
        Request::ProviderSave {
            username,
            worker_url,
            secrets,
        } => handle_provider_save(core, runtime, username, worker_url, secrets),
        Request::ClearProviderCredentials(provider) => {
            handle_clear_provider_credentials(core, runtime, provider)
        }
        Request::ClearKickToken => match runtime.block_on(core.clear_kick_token()) {
            Ok(()) => configuration_snapshot(core, false, true, "KICK 인증정보를 삭제했습니다."),
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
            return LoopAction::Continue;
        }
        Request::LiveStatus { poll, generation } => {
            let core = core.clone();
            let responses = responses.clone();
            runtime.spawn(async move {
                let _ =
                    responses.send(live_status_async(&core, poll, Some(generation), false).await);
            });
            return LoopAction::Continue;
        }
        Request::LiveRefresh => {
            let core = core.clone();
            let responses = responses.clone();
            runtime.spawn(async move {
                let _ = responses.send(live_status_async(&core, false, None, true).await);
            });
            return LoopAction::Continue;
        }
        Request::StorageLoad { poll, generation } => storage_status(core, poll, generation),
        Request::LiveStart => handle_live_start(core, runtime),
        Request::LiveStop => handle_live_stop(core, runtime),
        Request::LiveAction { target, action } => {
            return handle_live_action(core, runtime, responses, target, action);
        }
        Request::LivePassword { target, password } => {
            handle_live_password(core, runtime, target, password)
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
        Request::VodPickOutput { initial } => return handle_vod_pick_output(responses, initial),
        Request::VodDownload(req) => handle_vod_download(core, runtime, req),
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
        Request::QueueEnqueue(req) => handle_queue_enqueue(core, runtime, req),
        Request::QueueAction { id, action } => handle_queue_action(core, runtime, id, action),
        Request::HistoryLoad { filter, view } => handle_history_load(core, filter, view),
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
            return handle_backup_pick_directory(responses, initial);
        }
        Request::BackupSave { policy, directory } => {
            handle_backup_save(core, runtime, policy, directory)
        }
        Request::BackupCreate => handle_backup_create(core, runtime),
        Request::BackupRestore { file_name } => {
            handle_backup_restore(core, runtime, responses, file_name)
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
            return handle_open_live_folder(responses, file_path);
        }
        Request::OpenMediaFile(file_path) => {
            return handle_open_media_file(responses, file_path);
        }
        Request::OpenLiveStream { platform, account } => {
            return handle_open_live_stream(responses, platform, account);
        }
        Request::OpenLogsFolder => return handle_open_logs_folder(core, responses),
        Request::OpenReleaseUrl(url) => return handle_open_release_url(responses, url),
        Request::CheckUpdates { manual } => {
            let core = core.clone();
            let responses = responses.clone();
            runtime.spawn(async move {
                let result = core
                    .check_latest_release()
                    .await
                    .map_err(|e| format!("{e:#}"));
                let _ = responses.send(Response::UpdateCheck { manual, result });
            });
            return LoopAction::Continue;
        }
        Request::SetWindowsStartup(enabled) => {
            let responses = responses.clone();
            match spawn_detached_task("windows-startup", move || {
                let result = crate::desktop::windows_startup::set_enabled(enabled);
                let _ = responses.send(Response::WindowsStartup(result));
            }) {
                Ok(()) => return LoopAction::Continue,
                Err(error) => Response::WindowsStartup(Err(error)),
            }
        }
    };
    send_response(responses, response)
}

fn handle_save_settings(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    responses: &mpsc::Sender<Response>,
    patch: BTreeMap<String, String>,
) -> Response {
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

fn handle_pick_setting(
    responses: &mpsc::Sender<Response>,
    index: usize,
    kind: SettingKind,
    initial: String,
) -> LoopAction {
    let responses_clone = responses.clone();
    match spawn_detached_task("native-picker", move || {
        let response = match native_picker::pick(kind, &initial) {
            Ok(path) => Response::Picked(index, path),
            Err(error) => Response::Error(format!("폴더 선택기 오류: {error}")),
        };
        let _ = responses_clone.send(response);
    }) {
        Ok(()) => LoopAction::Continue,
        Err(error) => send_response(responses, Response::Error(error)),
    }
}

fn handle_channel_pick_output(
    responses: &mpsc::Sender<Response>,
    index: usize,
    expected: Vec<Channel>,
    initial: String,
) -> LoopAction {
    let responses_clone = responses.clone();
    match spawn_detached_task("native-picker", move || {
        let response = match native_picker::pick_directory(&initial) {
            Ok(path) => Response::ChannelPicked {
                index,
                expected,
                path,
            },
            Err(message) => Response::ConfigError(message),
        };
        let _ = responses_clone.send(response);
    }) {
        Ok(()) => LoopAction::Continue,
        Err(error) => send_response(responses, Response::ConfigError(error)),
    }
}

fn handle_channel_resolve(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    responses: &mpsc::Sender<Response>,
    index: usize,
    platform: PlatformId,
    account: String,
) -> LoopAction {
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
    LoopAction::Continue
}

fn handle_provider_save(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    username: String,
    worker_url: String,
    secrets: BTreeMap<String, String>,
) -> Response {
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
        Err(error) => {
            Response::ConfigError(format!("서비스 연결 정보를 저장하지 못했습니다: {error:#}"))
        }
    }
}

fn handle_clear_provider_credentials(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    provider: String,
) -> Response {
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

fn handle_live_start(core: &StreamArchiveCore, runtime: &tokio::runtime::Runtime) -> Response {
    match runtime.block_on(core.start_watcher()) {
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
    }
}

fn handle_live_stop(core: &StreamArchiveCore, runtime: &tokio::runtime::Runtime) -> Response {
    match runtime.block_on(core.stop_watcher()) {
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
    }
}

fn handle_live_action(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    responses: &mpsc::Sender<Response>,
    target: String,
    action: String,
) -> LoopAction {
    let Some(action) = live_adapter::validated_action(&action) else {
        let _ = responses.send(Response::LiveError {
            message: "지원하지 않는 LIVE 채널 작업입니다".into(),
            poll: false,
        });
        return LoopAction::Continue;
    };
    let response = match runtime.block_on(core.channel_action(target, action)) {
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
                message: format!("작업은 성공했지만 상태 새로고침에 실패했습니다: {error:#}"),
                poll: false,
            },
        },
        Err(error) => Response::LiveError {
            message: format!("채널 작업 실패: {error:#}"),
            poll: false,
        },
    };
    send_response(responses, response)
}

fn handle_live_password(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    target: String,
    password: String,
) -> Response {
    match runtime.block_on(core.channel_password(target, password)) {
        Ok(()) => match runtime.block_on(core.watcher_status()) {
            Ok(status) => Response::Live {
                generation: None,
                status,
                refresh_thumbnails: false,
                message: Some("비밀번호를 메모리에 전달하고 채널 다시 확인을 요청했습니다".into()),
                poll: false,
            },
            Err(error) => Response::LiveError {
                message: format!("비밀번호는 적용됐지만 상태 새로고침에 실패했습니다: {error:#}"),
                poll: false,
            },
        },
        Err(error) => Response::LiveError {
            message: format!("비밀번호를 적용하지 못했습니다: {error:#}"),
            poll: false,
        },
    }
}

fn handle_vod_pick_output(responses: &mpsc::Sender<Response>, initial: String) -> LoopAction {
    let responses_clone = responses.clone();
    match spawn_detached_task("native-picker", move || {
        let response = match native_picker::pick_directory(&initial) {
            Ok(path) => Response::VodPicked(path),
            Err(error) => Response::VodError {
                message: format!("VOD 출력 폴더 선택기 오류: {error}"),
                poll: false,
            },
        };
        let _ = responses_clone.send(response);
    }) {
        Ok(()) => LoopAction::Continue,
        Err(error) => send_response(
            responses,
            Response::VodError {
                message: error,
                poll: false,
            },
        ),
    }
}

fn handle_vod_download(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    req: VodDownloadRequest,
) -> Response {
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

fn handle_queue_enqueue(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    req: VodDownloadRequest,
) -> Response {
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

fn handle_queue_action(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    id: String,
    action: String,
) -> Response {
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

fn handle_history_load(core: &StreamArchiveCore, filter: HistoryFilter, view: String) -> Response {
    match core.history(&filter) {
        Ok(history) => Response::History {
            history,
            view,
            message: Some("기록을 새로고침했습니다".into()),
        },
        Err(error) => Response::HistoryError(format!("기록 새로고침 실패: {error:#}")),
    }
}

fn handle_backup_pick_directory(responses: &mpsc::Sender<Response>, initial: String) -> LoopAction {
    let responses_clone = responses.clone();
    match spawn_detached_task("native-picker", move || {
        let response = match native_picker::pick_directory(&initial) {
            Ok(path) => Response::MaintenancePicked(path),
            Err(error) => Response::MaintenanceError {
                message: format!("백업 폴더 선택기 오류: {error}"),
                poll: false,
            },
        };
        let _ = responses_clone.send(response);
    }) {
        Ok(()) => LoopAction::Continue,
        Err(error) => send_response(
            responses,
            Response::MaintenanceError {
                message: error,
                poll: false,
            },
        ),
    }
}

fn handle_backup_save(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    policy: BackupPolicy,
    directory: Option<String>,
) -> Response {
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

fn handle_backup_create(core: &StreamArchiveCore, runtime: &tokio::runtime::Runtime) -> Response {
    match runtime.block_on(core.create_manual_backup()) {
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
    }
}

fn handle_backup_restore(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    responses: &mpsc::Sender<Response>,
    file_name: String,
) -> Response {
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

fn handle_open_live_folder(responses: &mpsc::Sender<Response>, file_path: String) -> LoopAction {
    let responses_clone = responses.clone();
    match spawn_detached_task("native-shell", move || {
        let result = native_shell::open_containing_directory(&file_path);
        let _ = responses_clone.send(Response::LiveFolderOpened(result));
    }) {
        Ok(()) => LoopAction::Continue,
        Err(error) => send_response(responses, Response::LiveFolderOpened(Err(error))),
    }
}

fn handle_open_live_stream(
    responses: &mpsc::Sender<Response>,
    platform: String,
    account: String,
) -> LoopAction {
    let responses_clone = responses.clone();
    match spawn_detached_task("native-shell", move || {
        let result = native_shell::open_provider_url(&platform, &account);
        let _ = responses_clone.send(Response::LiveStreamOpened(result));
    }) {
        Ok(()) => LoopAction::Continue,
        Err(error) => send_response(responses, Response::LiveStreamOpened(Err(error))),
    }
}

fn handle_open_logs_folder(
    core: &StreamArchiveCore,
    responses: &mpsc::Sender<Response>,
) -> LoopAction {
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
            let responses_clone = responses.clone();
            match spawn_detached_task("native-shell", move || {
                let result = native_shell::open_containing_directory(&dir);
                let _ = responses_clone.send(Response::LogsFolderOpened(result));
            }) {
                Ok(()) => LoopAction::Continue,
                Err(error) => send_response(responses, Response::LogsFolderOpened(Err(error))),
            }
        }
        Err(err) => send_response(responses, Response::LogsFolderOpened(Err(err))),
    }
}

fn handle_open_media_file(responses: &mpsc::Sender<Response>, file_path: String) -> LoopAction {
    let responses_clone = responses.clone();
    match spawn_detached_task("native-shell", move || {
        let result = native_shell::open_media_file(&file_path);
        let _ = responses_clone.send(Response::MediaFileOpened(result));
    }) {
        Ok(()) => LoopAction::Continue,
        Err(error) => send_response(responses, Response::MediaFileOpened(Err(error))),
    }
}

fn handle_open_release_url(responses: &mpsc::Sender<Response>, url: String) -> LoopAction {
    let responses_clone = responses.clone();
    match spawn_detached_task("native-shell", move || {
        let result = native_shell::open_release_url(&url);
        let _ = responses_clone.send(Response::ReleaseUrlOpened(result));
    }) {
        Ok(()) => LoopAction::Continue,
        Err(error) => send_response(responses, Response::ReleaseUrlOpened(Err(error))),
    }
}
