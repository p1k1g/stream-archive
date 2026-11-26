//! Snapshot builders and shared helpers for `core_worker`.

use super::Response;
use stream_archive_server::{
    app_core::StreamArchiveCore,
    model::{Channel, VodAnalyzeRequest},
};

pub(super) fn read_active_snapshot(
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

pub(super) fn read_snapshot(
    core: &StreamArchiveCore,
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
        diagnostics: core.diagnostics(),
        window_bounds: core.window_bounds().ok().flatten(),
        first_run: core.is_first_run_unconfigured().unwrap_or(false),
        startup_failed: false,
        message: message.into(),
    }
}

pub(super) fn configuration_snapshot(
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
        telegram_chat_id: settings
            .get("TELEGRAM_CHAT_ID")
            .cloned()
            .unwrap_or_default(),
        secrets,
        reload_channels,
        reload_provider,
        message: message.into(),
    }
}

pub(super) async fn live_status_async(
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

pub(super) fn live_status(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    poll: bool,
    generation: Option<u64>,
) -> Response {
    runtime.block_on(live_status_async(core, poll, generation, false))
}

pub(super) fn storage_status(core: &StreamArchiveCore, poll: bool, generation: u64) -> Response {
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

pub(super) fn vod_status(
    core: &StreamArchiveCore,
    runtime: &tokio::runtime::Runtime,
    poll: bool,
) -> Response {
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

pub(super) fn queue_status(
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

pub(super) fn maintenance_snapshot(
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

pub(super) fn selected_parts_label(parts: &[usize]) -> String {
    if parts.is_empty() {
        return "전체".into();
    }
    parts
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn vod_analyze_request(url: String) -> VodAnalyzeRequest {
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

pub(super) fn save_channels(
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

pub(super) async fn desktop_snapshot(core: &StreamArchiveCore) -> Result<(String, bool), String> {
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
        "FILE_NAME_PATTERN" => {
            "LIVE 녹화 파일 이름 형식입니다. 변경사항 저장 후 새 녹화부터 적용됩니다."
        }
        "LIVE_AUTO_REMUX" => {
            "LIVE 녹화 종료 후 FFmpeg 무손실 스트림 복사(-c copy)로 .ts 파일을 .mp4 또는 .mkv로 자동 변환합니다."
        }
        "LIVE_REMUX_DELETE_SOURCE" => {
            "무손실 Remux가 성공적으로 완료된 경우에만 원본 .ts 파일을 자동 삭제합니다. 실패 시 원본은 안전하게 보존됩니다."
        }
        "WEBHOOK_ENABLED" => {
            "Discord Webhook 또는 Telegram Bot을 통해 외부 알림을 발송합니다. 아래 인증정보 섹션에 URL/토큰을 저장하세요."
        }
        "WEBHOOK_NOTIFY_LIVE" => {
            "LIVE 녹화 시작 및 종료 시 설정된 Discord · Telegram 웹훅으로 알림을 발송합니다."
        }
        "WEBHOOK_NOTIFY_VOD" => {
            "VOD 다운로드 완료 또는 실패 시 설정된 Discord · Telegram 웹훅으로 알림을 발송합니다."
        }
        "WEBHOOK_NOTIFY_WARNING" => {
            "LIVE 디스크 부족, 녹화 정체, 채널 오류 또는 인증 필요 상태 시 웹훅 경고 알림을 발송합니다."
        }
        "TELEGRAM_CHAT_ID" => {
            "Telegram 알림을 받을 채팅 ID(예: -1001234567890) 또는 @채널명입니다."
        }
        "STREAM_ARCHIVE_CLOSE_ACTION" => {
            "Windows 닫기 버튼 동작입니다. 트레이에서는 녹화·다운로드·감시가 계속됩니다. 변경사항을 저장하면 적용됩니다."
        }
        "GUI_NOTIFY_RECORD_START" => {
            "새 LIVE 방송 녹화가 시작될 때 Windows 트레이 알림을 표시합니다. 변경사항 저장 후 적용됩니다."
        }
        "GUI_NOTIFY_RECORD_FINISH" => {
            "LIVE 방송 녹화가 완료되었을 때 총 녹화 시간·용량과 함께 Windows 트레이 알림을 표시합니다."
        }
        "GUI_NOTIFY_WARNING" => {
            "저장 공간 부족(CRITICAL) 또는 채널 오류·인증 필요 상태 전환 시 Windows 트레이 경고 알림을 표시합니다."
        }
        "GUI_AUTO_START_WATCHER" => {
            "앱 실행 직후 등록된 채널의 LIVE 방송 감시를 자동으로 시작합니다. 저장 후 다음 실행부터 적용됩니다."
        }
        "GUI_START_MINIMIZED_TRAY" => {
            "앱 실행 시 메인 창을 열지 않고 시스템 트레이 아이콘으로 바로 시작합니다. 저장 후 다음 실행부터 적용됩니다."
        }
        "RECORD_STALL_TIMEOUT" => {
            "녹화 파일 크기가 증가하지 않을 때 정체로 판정하고 재연결하는 시간(초)입니다. 허용 범위: 10~86400."
        }
        "RECORD_RETRY_INTERVAL" => {
            "녹화 실패·종료 후 재시도까지 대기하는 간격(초)입니다. 허용 범위: 1~3600."
        }
        "LOG_ENABLED" => {
            "일별 런타임 로그 파일(.\\log\\runtime-YYYY-MM-DD.log) 기록 여부입니다. 저장 후 다음 실행부터 적용됩니다."
        }
        "LOG_RETENTION_DAYS" => {
            "일별 런타임 로그 파일 보관 일수입니다(0은 무기한 보관). 허용 범위: 0~36500."
        }
        _ => fallback,
    }
}
