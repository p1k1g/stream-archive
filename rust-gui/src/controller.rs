//! Presentation controller bridging GPUI views, `core_worker`, and `windows_tray`.
//! Holds no direct SQLite, HTTP, or process-management code.

use crate::{
    channels_adapter::ChannelsDraft,
    core_worker::{self, Request, Response, localized_setting_description},
    history_adapter, live_adapter,
    live_model::{LiveModel, StorageRefresh},
    maintenance_adapter, notifications, queue_adapter,
    settings_adapter::SettingsDraft,
    state::{
        ChannelConfigRow, DiagnosticRow, HistoryCalendarDay, HistoryDisplayRow, LiveChannelRow,
        MaintenanceBackupRow, MaintenanceDiagnosticRow, MaintenanceLogRow, MaintenanceSection,
        Page, QueueDisplayRow, SettingRow, SettingsView, StorageDisplayRow, UiState, VodPartRow,
        VodQualityRow,
    },
    storage_adapter,
    thumbnail_adapter::{Thumbnails, rgba_to_render_image},
    vod_adapter::{self, AnalysisSync, VodDraft},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use stream_archive_server::{
    backup_service::{BackupPolicy, BackupSnapshot},
    diagnostics::DiagnosticsSnapshot,
    environment_settings::SettingKind,
    history_service::HistoryFilter,
    model::{HistoryResponse, NativeWatcherStatus, VodJobStatus, VodQueueSnapshot},
    resource_trace,
    storage_service::StorageSnapshot,
};

pub struct ProviderSaveInput {
    pub username: String,
    pub worker_url: String,
    pub soop_password: String,
    pub worker_api_key: String,
    pub chzzk_nid_aut: String,
    pub chzzk_nid_ses: String,
    pub kick_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopSignal {
    HideToTray,
    ExitNow,
    DownloadNotice { completed: usize, failed: usize },
    ClearNotices,
    StatusUpdated(String),
}

pub struct AppController {
    pub state: UiState,
    pub live_model: LiveModel,
    settings_draft: SettingsDraft,
    channels_draft: ChannelsDraft,
    vod_draft: VodDraft,
    thumbnails: Thumbnails,
    storage_refresh: StorageRefresh,
    notice_tracker: notifications::Tracker,
    resource_trace: Option<stream_archive_server::resource_trace::ResourceTrace>,

    sender: mpsc::Sender<Request>,
    responses: mpsc::Receiver<Response>,
    worker: Option<std::thread::JoinHandle<()>>,
    stopping: Arc<AtomicBool>,

    window_visible: bool,
    live_poll_in_flight: bool,
    storage_poll_in_flight: bool,
    vod_poll_in_flight: bool,
    queue_poll_in_flight: bool,
    maintenance_log_poll_in_flight: bool,
    history_refresh_queued: bool,
    last_live_poll: Instant,
    last_storage_poll: Instant,
    last_vod_poll: Instant,
    last_queue_poll: Instant,
    last_log_poll: Instant,
    last_trace_emit: Instant,

    pending_signals: Vec<DesktopSignal>,
}

impl AppController {
    pub fn start() -> Self {
        let (req_tx, req_rx) = mpsc::channel();
        let (res_tx, res_rx) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let worker = core_worker::spawn_worker(req_rx, res_tx, stopping.clone()).ok();

        let mut controller = Self::with_channels(req_tx, res_rx, worker, stopping);
        let initial_cal = history_adapter::calendar_initial("");
        controller.apply_calendar(initial_cal);
        controller
    }

    #[cfg(test)]
    pub fn new_disconnected_for_test() -> (Self, mpsc::Receiver<Request>) {
        let (req_tx, req_rx) = mpsc::channel();
        let (_res_tx, res_rx) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let mut controller = Self::with_channels(req_tx, res_rx, None, stopping);
        let initial_cal = history_adapter::calendar_initial("");
        controller.apply_calendar(initial_cal);
        (controller, req_rx)
    }

    fn with_channels(
        sender: mpsc::Sender<Request>,
        responses: mpsc::Receiver<Response>,
        worker: Option<std::thread::JoinHandle<()>>,
        stopping: Arc<AtomicBool>,
    ) -> Self {
        let now = Instant::now();
        Self {
            state: UiState::default(),
            live_model: LiveModel::default(),
            settings_draft: SettingsDraft::default(),
            channels_draft: ChannelsDraft::default(),
            vod_draft: VodDraft::default(),
            thumbnails: Thumbnails::default(),
            storage_refresh: StorageRefresh::default(),
            notice_tracker: notifications::Tracker::default(),
            resource_trace: None,
            sender,
            responses,
            worker,
            stopping,
            window_visible: true,
            live_poll_in_flight: false,
            storage_poll_in_flight: false,
            vod_poll_in_flight: false,
            queue_poll_in_flight: false,
            maintenance_log_poll_in_flight: false,
            history_refresh_queued: false,
            last_live_poll: now,
            last_storage_poll: now,
            last_vod_poll: now,
            last_queue_poll: now,
            last_log_poll: now,
            last_trace_emit: now,
            pending_signals: Vec::new(),
        }
    }

    pub fn take_desktop_signals(&mut self) -> Vec<DesktopSignal> {
        std::mem::take(&mut self.pending_signals)
    }

    pub fn is_window_visible(&self) -> bool {
        self.window_visible
    }

    pub fn set_window_visible(&mut self, visible: bool) {
        self.window_visible = visible;
        let became_visible = self.live_model.set_visible(visible);
        if became_visible {
            let generation = self.live_model.generation();
            self.storage_refresh.reopen(generation);
            if self.state.active_page == Page::Live {
                self.request_live_snapshot(generation);
                self.maybe_request_storage_reopen();
            } else {
                self.request_thumbnails();
            }
            if let Some(status) = self.live_model.take_for_display() {
                self.render_live(status);
            }
        }
    }

    pub fn select_page(&mut self, page: Page) {
        if self.state.active_page == page {
            return;
        }
        self.state.active_page = page;
        match page {
            Page::Live => {
                if self.live_model.needs_snapshot()
                    || (!self.state.live_busy && !self.live_poll_in_flight)
                {
                    self.request_live_snapshot(self.live_model.generation());
                }
                self.maybe_request_storage_reopen();
            }
            Page::Channels => {
                if !self.state.config_loaded && !self.state.config_busy {
                    self.reload_config();
                }
            }
            Page::Vod => {
                if !self.state.vod_busy && !self.vod_poll_in_flight {
                    let _ = self.sender.send(Request::VodStatus { poll: true });
                    self.vod_poll_in_flight = true;
                }
            }
            Page::Queue => {
                if !self.state.queue_busy && !self.queue_poll_in_flight {
                    let _ = self.sender.send(Request::QueueStatus { poll: true });
                    self.queue_poll_in_flight = true;
                }
            }
            Page::History => {
                if !self.state.history_loaded && !self.state.history_busy {
                    self.refresh_history();
                }
            }
            Page::Settings => {
                if self.state.settings_view == SettingsView::Manage {
                    self.state.maintenance_section = MaintenanceSection::Backup;
                    if !self.state.maintenance_loaded && !self.state.maintenance_busy {
                        self.refresh_maintenance(false);
                    }
                }
            }
            Page::Diagnostics => {
                if self.state.maintenance_section == MaintenanceSection::Backup {
                    self.state.maintenance_section = MaintenanceSection::Diagnostics;
                }
                if !self.state.maintenance_loaded && !self.state.maintenance_busy {
                    self.refresh_maintenance(false);
                }
            }
        }
    }

    pub fn set_settings_view(&mut self, view: SettingsView) {
        self.state.settings_view = view;
        if view == SettingsView::Manage {
            self.state.maintenance_section = MaintenanceSection::Backup;
            if !self.state.maintenance_loaded && !self.state.maintenance_busy {
                self.refresh_maintenance(false);
            }
        }
    }

    pub fn set_maintenance_section(&mut self, section: MaintenanceSection) {
        self.state.maintenance_section = section;
        if section == MaintenanceSection::Logs && !self.maintenance_log_poll_in_flight {
            let _ = self.sender.send(Request::LogsLoad { poll: true });
            self.maintenance_log_poll_in_flight = true;
        }
    }

    pub fn dismiss_recovery_message(&mut self) {
        self.state.desktop_recovery_message.clear();
    }

    // ---- LIVE actions ----

    pub fn live_start(&mut self) {
        if self.state.live_busy || !self.state.runtime_ready {
            return;
        }
        self.state.live_busy = true;
        self.state.live_message = "LIVE Watcher를 시작하는 중...".into();
        let _ = self.sender.send(Request::LiveStart);
    }

    pub fn live_stop(&mut self) {
        if self.state.live_busy || !self.state.runtime_ready {
            return;
        }
        self.state.live_busy = true;
        self.state.live_message = "LIVE Watcher를 중지하는 중...".into();
        let _ = self.sender.send(Request::LiveStop);
    }

    pub fn live_refresh(&mut self) {
        if self.state.live_busy {
            return;
        }
        self.state.live_busy = true;
        self.state.live_message = "LIVE 상태를 새로고침하는 중...".into();
        let _ = self.sender.send(Request::LiveRefresh);
        self.refresh_storage();
    }

    pub fn refresh_storage(&mut self) {
        if self.state.storage_busy {
            return;
        }
        self.state.storage_busy = true;
        let _ = self.sender.send(Request::StorageLoad {
            generation: self.live_model.generation(),
            poll: false,
        });
    }

    pub fn set_live_filter(&mut self, filter: &str) {
        if self.live_model.set_filter(filter) {
            self.state.live_filter = filter.to_string();
            if let Some(status) = self.live_model.latest.clone() {
                self.render_live(status);
            }
        }
    }

    pub fn live_action(&mut self, target: String, action: String) {
        if self.state.live_busy {
            return;
        }
        self.state.live_busy = true;
        let _ = self.sender.send(Request::LiveAction { target, action });
    }

    pub fn live_open_folder(&mut self, file_path: String) {
        let _ = self.sender.send(Request::OpenLiveFolder(file_path));
    }

    pub fn open_password_prompt(&mut self, target: String, name: String) {
        self.state.password_prompt_target = Some(target);
        self.state.password_prompt_name = name;
    }

    pub fn cancel_password_prompt(&mut self) {
        self.state.password_prompt_target = None;
        self.state.password_prompt_name.clear();
    }

    pub fn submit_live_password(&mut self, password: String) {
        let Some(target) = self.state.password_prompt_target.take() else {
            return;
        };
        self.state.password_prompt_name.clear();
        self.state.live_busy = true;
        let _ = self.sender.send(Request::LivePassword { target, password });
    }

    // ---- Channels actions ----

    pub fn channels_add(&mut self) {
        self.channels_draft.add();
        self.sync_channels_view(true);
    }

    pub fn channels_remove(&mut self, index: usize) {
        self.channels_draft.remove(index);
        self.sync_channels_view(true);
    }

    pub fn channels_edit(&mut self, index: usize, field: &str, value: String) {
        self.channels_draft.edit(index, field, value);
        self.sync_channels_view(false);
    }

    pub fn channels_set_enabled(&mut self, index: usize, enabled: bool) {
        self.channels_draft.set_enabled(index, enabled);
        self.sync_channels_view(false);
    }

    pub fn channels_select_platform(&mut self, index: usize, platform: &str) {
        self.channels_draft.select_platform(index, platform);
        self.sync_channels_view(false);
    }

    pub fn channels_pick_output(&mut self, index: usize) {
        if self.state.config_busy || index >= self.channels_draft.rows.len() {
            return;
        }
        let expected = self.channels_draft.snapshot();
        let initial = expected[index].outdir.clone();
        let _ = self.sender.send(Request::ChannelPickOutput {
            index,
            expected,
            initial,
        });
    }

    pub fn channels_resolve(&mut self, index: usize) {
        if self.state.config_busy || index >= self.channels_draft.rows.len() {
            return;
        }
        let row = &self.channels_draft.rows[index];
        if row.account.trim().is_empty() {
            self.state.config_message = "채널 ID / 계정을 먼저 입력하세요.".into();
            return;
        }
        let _ = self.sender.send(Request::ChannelResolve {
            index,
            platform: row.platform,
            account: row.account.clone(),
        });
    }

    pub fn channels_save(&mut self) {
        if self.state.config_busy {
            return;
        }
        self.state.config_busy = true;
        self.state.config_message = "채널 목록을 저장하는 중...".into();
        let _ = self
            .sender
            .send(Request::ChannelsSave(self.channels_draft.snapshot()));
    }

    pub fn reload_config(&mut self) {
        if self.state.config_busy {
            return;
        }
        self.state.config_busy = true;
        self.state.config_message = "채널 및 서비스 연결 정보를 다시 불러오는 중...".into();
        let _ = self.sender.send(Request::ConfigReload);
    }

    // ---- Provider credentials ----

    pub fn provider_save(&mut self, input: ProviderSaveInput) {
        if self.state.config_busy {
            return;
        }
        let mut secrets = BTreeMap::new();
        if !input.soop_password.trim().is_empty() {
            secrets.insert("SOOP_PASSWORD".into(), input.soop_password);
        }
        if !input.worker_api_key.trim().is_empty() {
            secrets.insert("CLOUDFLARE_API_KEY".into(), input.worker_api_key);
        }
        if !input.chzzk_nid_aut.trim().is_empty() {
            secrets.insert("CHZZK_NID_AUT".into(), input.chzzk_nid_aut);
        }
        if !input.chzzk_nid_ses.trim().is_empty() {
            secrets.insert("CHZZK_NID_SES".into(), input.chzzk_nid_ses);
        }
        if !input.kick_token.trim().is_empty() {
            secrets.insert("KICK_SESSION_TOKEN".into(), input.kick_token);
        }
        self.state.config_busy = true;
        self.state.config_message = "서비스 연결 정보를 저장하는 중...".into();
        let _ = self.sender.send(Request::ProviderSave {
            username: input.username,
            worker_url: input.worker_url,
            secrets,
        });
    }

    pub fn provider_test_soop(&mut self) {
        if self.state.config_busy {
            return;
        }
        self.state.config_message = "SOOP / Worker 연결을 테스트하는 중...".into();
        let _ = self.sender.send(Request::ProviderTestSoop);
    }

    pub fn provider_clear(&mut self, provider: &str) {
        if self.state.config_busy {
            return;
        }
        self.state.config_busy = true;
        if provider == "KICK" {
            let _ = self.sender.send(Request::ClearKickToken);
        } else {
            let _ = self
                .sender
                .send(Request::ClearProviderCredentials(provider.to_string()));
        }
    }

    // ---- VOD actions ----

    pub fn vod_edit_url(&mut self, url: String) {
        self.vod_draft.edit_url(url);
        self.state.vod_thumbnail_image = None;
        self.sync_vod_draft();
    }

    pub fn vod_edit_output(&mut self, output: String) {
        self.vod_draft.edit_output_directory(output);
        self.sync_vod_draft();
    }

    pub fn vod_pick_output(&mut self) {
        let _ = self.sender.send(Request::VodPickOutput {
            initial: self.vod_draft.output_directory.clone(),
        });
    }

    pub fn vod_analyze(&mut self) {
        if self.state.vod_busy {
            return;
        }
        let url = self.vod_draft.url.trim().to_string();
        if url.is_empty() {
            self.state.vod_message = "분석할 VOD URL을 입력하세요.".into();
            return;
        }
        self.vod_draft.begin_analysis();
        self.state.vod_thumbnail_image = None;
        self.sync_vod_draft();
        self.state.vod_busy = true;
        self.state.vod_message = "VOD를 분석하는 중...".into();
        let _ = self.sender.send(Request::VodAnalyze { url });
    }

    pub fn vod_select_quality(&mut self, index: usize) {
        self.vod_draft.select_quality(index);
        self.sync_vod_draft();
    }

    pub fn vod_toggle_part(&mut self, index: usize) {
        self.vod_draft.toggle_part(index);
        self.sync_vod_draft();
    }

    pub fn vod_select_all_parts(&mut self) {
        self.vod_draft.select_all_parts();
        self.sync_vod_draft();
    }

    pub fn vod_clear_parts(&mut self) {
        self.vod_draft.clear_parts();
        self.sync_vod_draft();
    }

    pub fn vod_toggle_merge(&mut self) {
        self.vod_draft.toggle_merge();
        self.sync_vod_draft();
    }

    pub fn vod_download(&mut self) {
        if self.state.vod_busy {
            return;
        }
        if let Some(req) = self.vod_draft.download_request() {
            self.state.vod_busy = true;
            let _ = self.sender.send(Request::VodDownload(req));
        } else {
            self.state.vod_message = "URL 분석 후 화질·PART·저장 경로를 선택하세요.".into();
        }
    }

    pub fn vod_enqueue(&mut self) {
        if self.state.queue_busy {
            return;
        }
        if let Some(req) = self.vod_draft.download_request() {
            self.state.queue_busy = true;
            let _ = self.sender.send(Request::QueueEnqueue(req));
        } else {
            self.state.vod_message = "URL 분석 후 화질·PART·저장 경로를 선택하세요.".into();
        }
    }

    pub fn vod_cancel(&mut self) {
        if self.state.vod_busy {
            return;
        }
        self.state.vod_busy = true;
        let _ = self.sender.send(Request::VodCancel);
    }

    // ---- Queue actions ----

    pub fn queue_refresh(&mut self) {
        if self.state.queue_busy {
            return;
        }
        self.state.queue_busy = true;
        let _ = self.sender.send(Request::QueueStatus { poll: false });
    }

    pub fn queue_action(&mut self, id: String, action: String) {
        if self.state.queue_busy {
            return;
        }
        self.state.queue_busy = true;
        let _ = self.sender.send(Request::QueueAction { id, action });
    }

    pub fn queue_toggle_expand(&mut self, id: &str) {
        if self.state.expanded_queue_id == id {
            self.state.expanded_queue_id.clear();
        } else {
            self.state.expanded_queue_id = id.to_string();
        }
    }

    // ---- History actions ----

    pub fn history_set_view(&mut self, view: &str) {
        self.state.history_view = view.to_string();
        self.refresh_history();
    }

    pub fn history_set_search(&mut self, search: String) {
        self.state.history_search = search;
    }

    pub fn history_set_status(&mut self, status: &str) {
        self.state.history_status = status.to_string();
        self.refresh_history();
    }

    pub fn history_set_limit(&mut self, limit: &str) {
        self.state.history_limit = limit.to_string();
        self.refresh_history();
    }

    pub fn history_calendar_open(&mut self, target: &str) {
        if self.state.history_calendar_open && self.state.history_calendar_target == target {
            self.state.history_calendar_open = false;
            return;
        }
        self.state.history_calendar_target = target.to_string();
        let selected = if target == "to" {
            self.state.history_to_date.clone()
        } else {
            self.state.history_from_date.clone()
        };
        let cal = history_adapter::calendar_initial(&selected);
        self.apply_calendar(cal);
        self.state.history_calendar_open = true;
    }

    pub fn history_calendar_shift(&mut self, delta: i32) {
        let selected = if self.state.history_calendar_target == "to" {
            self.state.history_to_date.clone()
        } else {
            self.state.history_from_date.clone()
        };
        let cal = history_adapter::calendar_shift(
            self.state.history_calendar_year,
            self.state.history_calendar_month,
            delta,
            &selected,
        );
        self.apply_calendar(cal);
    }

    pub fn history_calendar_select(&mut self, date: String) {
        if self.state.history_calendar_target == "to" {
            self.state.history_to_date = date;
        } else {
            self.state.history_from_date = date;
        }
        self.state.history_calendar_open = false;
        self.refresh_history();
    }

    pub fn history_calendar_clear(&mut self) {
        if self.state.history_calendar_target == "to" {
            self.state.history_to_date.clear();
        } else {
            self.state.history_from_date.clear();
        }
        self.state.history_calendar_open = false;
        self.refresh_history();
    }

    pub fn refresh_history(&mut self) {
        if self.state.history_busy {
            self.history_refresh_queued = true;
            return;
        }
        self.history_refresh_queued = false;
        let limit = self.state.history_limit.trim().parse::<usize>().ok();
        let filter = HistoryFilter {
            q: (!self.state.history_search.trim().is_empty())
                .then(|| self.state.history_search.trim().to_string()),
            status: (!self.state.history_status.trim().is_empty())
                .then(|| self.state.history_status.trim().to_string()),
            from: (!self.state.history_from_date.trim().is_empty())
                .then(|| self.state.history_from_date.trim().to_string()),
            to: (!self.state.history_to_date.trim().is_empty())
                .then(|| self.state.history_to_date.trim().to_string()),
            limit,
        };
        self.state.history_busy = true;
        self.state.history_message = "기록을 불러오는 중...".into();
        let _ = self.sender.send(Request::HistoryLoad {
            filter,
            view: self.state.history_view.clone(),
        });
    }

    // ---- General Settings actions ----

    pub fn settings_edit(&mut self, index: usize, value: String) {
        self.settings_draft.edit(index, value);
        self.sync_settings_view(false);
    }

    pub fn settings_set_key(&mut self, key: &str, value: String) {
        if let Some(index) = self
            .settings_draft
            .fields
            .iter()
            .position(|field| field.key == key)
        {
            self.settings_draft.edit(index, value);
            self.sync_settings_view(false);
        }
    }

    pub fn settings_pick(&mut self, index: usize) {
        if self.state.settings_busy {
            return;
        }
        let Some(field) = self.settings_draft.fields.get(index) else {
            return;
        };
        if field.kind == SettingKind::Text {
            return;
        }
        let _ = self
            .sender
            .send(Request::Pick(index, field.kind, field.value.clone()));
    }

    pub fn settings_save(&mut self) {
        if self.state.settings_busy {
            return;
        }
        let patch = self.settings_draft.patch();
        if patch.is_empty() {
            self.state.settings_message = "변경된 설정이 없습니다.".into();
            return;
        }
        self.state.settings_busy = true;
        self.state.settings_message = "설정을 저장하는 중...".into();
        let _ = self.sender.send(Request::Save(patch));
    }

    pub fn settings_reload(&mut self) {
        if self.state.settings_busy {
            return;
        }
        self.state.settings_busy = true;
        self.state.settings_message = "저장된 설정을 다시 불러오는 중...".into();
        let _ = self.sender.send(Request::Reload);
    }

    pub fn refresh_requested(&mut self) {
        if self.state.settings_busy {
            return;
        }
        self.state.settings_busy = true;
        let _ = self.sender.send(Request::Refresh);
    }

    // ---- Maintenance / Backup / Logs actions ----

    pub fn refresh_maintenance(&mut self, active_tools: bool) {
        if self.state.maintenance_busy {
            return;
        }
        self.state.maintenance_busy = true;
        self.state.maintenance_message = if active_tools {
            "로컬 도구 버전 및 관리 상태를 확인하는 중...".into()
        } else {
            "관리 상태를 새로고침하는 중...".into()
        };
        let _ = self.sender.send(Request::MaintenanceLoad { active_tools });
    }

    pub fn backup_pick_directory(&mut self) {
        if self.state.backup_directory_locked || self.state.maintenance_busy {
            return;
        }
        let _ = self.sender.send(Request::BackupPickDirectory {
            initial: self.state.backup_directory.clone(),
        });
    }

    pub fn backup_save_policy(
        &mut self,
        enabled: bool,
        interval_hours: String,
        keep_count: String,
        retention_days: String,
        directory: String,
    ) {
        if self.state.maintenance_busy {
            return;
        }
        let Ok(interval_hours) = interval_hours.trim().parse::<u64>() else {
            self.state.maintenance_message = "백업 간격(시간)은 1 이상의 정수여야 합니다.".into();
            return;
        };
        let Ok(keep_count) = keep_count.trim().parse::<usize>() else {
            self.state.maintenance_message = "보관 개수는 1 이상의 정수여야 합니다.".into();
            return;
        };
        let Ok(retention_days) = retention_days.trim().parse::<i64>() else {
            self.state.maintenance_message = "보관 일수는 1 이상의 정수여야 합니다.".into();
            return;
        };
        let policy = BackupPolicy {
            enabled,
            interval_hours,
            keep_count,
            retention_days,
        };
        let dir_opt = (!self.state.backup_directory_locked).then_some(directory);
        self.state.maintenance_busy = true;
        self.state.maintenance_message = "백업 정책을 저장하는 중...".into();
        let _ = self.sender.send(Request::BackupSave {
            policy,
            directory: dir_opt,
        });
    }

    pub fn backup_create(&mut self) {
        if self.state.maintenance_busy {
            return;
        }
        self.state.maintenance_busy = true;
        self.state.maintenance_message = "수동 백업을 생성하는 중...".into();
        let _ = self.sender.send(Request::BackupCreate);
    }

    pub fn request_restore(&mut self, file_name: String) {
        self.state.restore_confirm_file = Some(file_name);
    }

    pub fn cancel_restore(&mut self) {
        self.state.restore_confirm_file = None;
    }

    pub fn confirm_restore(&mut self) {
        let Some(file_name) = self.state.restore_confirm_file.take() else {
            return;
        };
        if self.state.maintenance_busy {
            return;
        }
        self.state.maintenance_busy = true;
        self.state.maintenance_message = format!("백업 복원 중: {file_name}");
        let _ = self.sender.send(Request::BackupRestore { file_name });
    }

    pub fn refresh_logs(&mut self) {
        let _ = self.sender.send(Request::LogsLoad { poll: false });
    }

    pub fn toggle_log_auto_refresh(&mut self) {
        self.state.log_auto_refresh = !self.state.log_auto_refresh;
    }

    pub fn open_logs_folder(&mut self) {
        let _ = self.sender.send(Request::OpenLogsFolder);
    }

    pub fn clear_logs(&mut self) {
        let _ = self.sender.send(Request::LogsClear);
    }

    // ---- Desktop Close / Exit / Tray coordination ----

    pub fn request_desktop_status(&mut self) {
        if self.state.desktop_poll_pending || self.state.desktop_exit_pending {
            return;
        }
        self.state.desktop_poll_pending = true;
        let _ = self.sender.send(Request::DesktopStatus);
    }

    pub fn handle_window_close_requested(&mut self) -> bool {
        if crate::desktop::close_request_blocked(
            self.state.settings_loaded,
            self.state.settings_busy,
            self.state.close_choice_busy,
            self.state.close_dialog_visible,
            self.state.desktop_exit_pending,
        ) {
            return false;
        }
        match self.state.close_action.as_str() {
            "TRAY" => {
                self.pending_signals.push(DesktopSignal::HideToTray);
                false
            }
            "ASK" => {
                self.state.close_remember = false;
                self.state.close_choice_error.clear();
                self.state.close_dialog_visible = true;
                false
            }
            _ => {
                self.begin_exit_check();
                false
            }
        }
    }

    pub fn submit_close_choice(&mut self, action: &str, remember: bool) {
        if self.state.close_choice_busy {
            return;
        }
        self.state.close_choice_error.clear();
        if remember {
            self.state.close_choice_busy = true;
            let _ = self.sender.send(Request::RememberClose(action.to_string()));
        } else {
            self.state.close_dialog_visible = false;
            self.apply_close_action(action);
        }
    }

    pub fn cancel_close_dialog(&mut self) {
        if !self.state.close_choice_busy {
            self.state.close_dialog_visible = false;
            self.state.close_choice_error.clear();
        }
    }

    pub fn begin_exit_check(&mut self) {
        if self.state.desktop_exit_pending {
            return;
        }
        self.state.desktop_exit_pending = true;
        let _ = self.sender.send(Request::PrepareExit);
    }

    pub fn complete_exit_confirmation(&mut self, confirmed: bool) {
        if confirmed {
            self.stopping.store(true, Ordering::Release);
            let _ = self.sender.send(Request::Shutdown);
        } else {
            self.state.desktop_exit_pending = false;
        }
    }

    fn apply_close_action(&mut self, action: &str) {
        if action == "TRAY" {
            self.pending_signals.push(DesktopSignal::HideToTray);
        } else {
            self.begin_exit_check();
        }
    }

    // ---- Periodic Polling & Response Draining ----

    pub fn poll_tick(&mut self) -> (bool, Option<Result<bool, String>>) {
        let mut changed = false;
        let mut exit_check = None;

        while let Ok(response) = self.responses.try_recv() {
            changed = true;
            if let Response::ExitCheck(res) = response {
                exit_check = Some(res);
                continue;
            }
            self.apply_response(response);
        }

        let now = Instant::now();
        if !self.state.desktop_exit_pending {
            if now.duration_since(self.last_live_poll) >= Duration::from_millis(1500) {
                self.last_live_poll = now;
                if live_poll_needed(
                    self.state.active_page.as_str(),
                    self.window_visible,
                    self.state.live_busy,
                    self.live_poll_in_flight,
                    self.thumbnails.is_tracking(),
                ) {
                    let generation = self.live_model.generation();
                    if self
                        .sender
                        .send(Request::LiveStatus {
                            poll: true,
                            generation,
                        })
                        .is_ok()
                    {
                        self.live_poll_in_flight = true;
                    }
                }
            }

            if now.duration_since(self.last_storage_poll) >= Duration::from_secs(30) {
                self.last_storage_poll = now;
                if self.window_visible
                    && self.state.active_page == Page::Live
                    && !self.state.storage_busy
                    && !self.storage_poll_in_flight
                {
                    let generation = self.live_model.generation();
                    if self
                        .sender
                        .send(Request::StorageLoad {
                            poll: true,
                            generation,
                        })
                        .is_ok()
                    {
                        self.storage_poll_in_flight = true;
                    }
                }
            }

            if now.duration_since(self.last_vod_poll) >= Duration::from_secs(1) {
                self.last_vod_poll = now;
                if self.window_visible
                    && self.state.active_page == Page::Vod
                    && !self.state.vod_busy
                    && !self.vod_poll_in_flight
                    && self.sender.send(Request::VodStatus { poll: true }).is_ok()
                {
                    self.vod_poll_in_flight = true;
                }
            }

            if now.duration_since(self.last_queue_poll) >= Duration::from_secs(1) {
                self.last_queue_poll = now;
                if self.window_visible
                    && self.state.active_page == Page::Queue
                    && !self.state.queue_busy
                    && !self.queue_poll_in_flight
                    && self
                        .sender
                        .send(Request::QueueStatus { poll: true })
                        .is_ok()
                {
                    self.queue_poll_in_flight = true;
                }
            }

            if now.duration_since(self.last_log_poll) >= Duration::from_millis(1500) {
                self.last_log_poll = now;
                if self.window_visible
                    && self.state.active_page == Page::Diagnostics
                    && self.state.maintenance_section == MaintenanceSection::Logs
                    && self.state.log_auto_refresh
                    && !self.state.maintenance_busy
                    && !self.maintenance_log_poll_in_flight
                    && self.sender.send(Request::LogsLoad { poll: true }).is_ok()
                {
                    self.maintenance_log_poll_in_flight = true;
                }
            }

            if self.window_visible && self.state.active_page == Page::Live {
                self.maybe_request_storage_reopen();
            }

            if let Some(trace) = &self.resource_trace
                && now.duration_since(self.last_trace_emit) >= Duration::from_secs(15)
            {
                self.last_trace_emit = now;
                let (entries, pending, bytes) = self.thumbnails.metrics();
                trace.record(
                    resource_trace::Event::Sample,
                    resource_trace::Sample {
                        visible: Some(self.window_visible),
                        page: Some(resource_trace::Page::from_name(
                            self.state.active_page.as_str(),
                        )),
                        channels: Some(self.live_model.rows.len()),
                        thumbnails: Some(entries),
                        pending: Some(pending),
                        rgba_bytes: Some(bytes),
                        model_resets: Some(self.live_model.resets),
                        row_updates: Some(self.live_model.row_updates),
                        ..Default::default()
                    },
                );
            }
        }

        (changed, exit_check)
    }

    fn apply_response(&mut self, response: Response) {
        match response {
            Response::ResourceTrace(trace) => {
                self.resource_trace = Some(trace);
            }
            Response::VodThumbnail {
                generation,
                url,
                image,
            } => {
                if self.vod_draft.thumbnail_is_current(generation, &url) {
                    self.state.vod_thumbnail_image = image.as_ref().and_then(rgba_to_render_image);
                }
            }
            Response::Thumbnail { token, image } => {
                let updated = self.thumbnails.complete(token, image);
                if updated {
                    self.live_model.invalidate_images();
                    if let Some(status) = self.live_model.take_for_display() {
                        self.render_live(status);
                    }
                }
                self.request_thumbnails();
            }
            Response::RememberedClose(result) => {
                self.state.close_choice_busy = false;
                match result {
                    Ok(action) => {
                        self.settings_draft
                            .accept_saved_value("STREAM_ARCHIVE_CLOSE_ACTION", &action);
                        self.state.close_action = action.clone();
                        self.state.close_to_tray = action == "TRAY";
                        self.state.close_dialog_visible = false;
                        self.sync_settings_view(true);
                        self.apply_close_action(&action);
                    }
                    Err(err) => {
                        self.state.close_choice_error = err;
                    }
                }
            }
            Response::Snapshot {
                fields,
                diagnostics,
                first_run,
                startup_failed,
                message,
            } => {
                self.state.settings_busy = false;
                self.state.startup_failed = startup_failed;
                self.state.first_run = first_run;
                self.state.runtime_ready = !startup_failed;
                self.state.settings_message = message;
                bind_core_snapshot(&mut self.state, diagnostics);
                if let Some(fields) = fields {
                    self.state.settings_loaded = true;
                    if let Some(action) = fields
                        .iter()
                        .find(|f| f.key == "STREAM_ARCHIVE_CLOSE_ACTION")
                    {
                        self.state.close_action = action.value.clone();
                        self.state.close_to_tray = action.value == "TRAY";
                    }
                    if let Some(notif) = fields
                        .iter()
                        .find(|f| f.key == "STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS")
                    {
                        let enabled = !notif.value.eq_ignore_ascii_case("false");
                        self.state.download_notifications_enabled = enabled;
                    }
                    if let Some(out) = fields.iter().find(|f| f.key == "OUTPUT_DIR") {
                        self.vod_draft.sync_default_output(&out.value);
                        self.state.vod_output_directory = self.vod_draft.output_directory.clone();
                        self.state.vod_output_revision =
                            self.state.vod_output_revision.wrapping_add(1);
                    }
                    self.settings_draft.load(fields);
                    self.sync_settings_view(true);
                }
                if first_run && self.state.active_page == Page::Live {
                    self.state.active_page = Page::Settings;
                }
            }
            Response::Configuration {
                channels,
                username,
                worker_url,
                secrets,
                reload_channels,
                reload_provider,
                message,
            } => {
                self.state.config_busy = false;
                self.state.config_loaded = true;
                self.state.config_message = message;
                if reload_channels {
                    self.channels_draft.load(channels);
                    self.sync_channels_view(true);
                }
                if reload_provider {
                    self.state.provider_soop_username = username;
                    self.state.provider_worker_url = worker_url;
                    self.state.provider_soop_password.clear();
                    self.state.provider_worker_api_key.clear();
                    self.state.provider_chzzk_nid_aut.clear();
                    self.state.provider_chzzk_nid_ses.clear();
                    self.state.provider_kick_token.clear();
                    self.state.provider_soop_password_set =
                        secrets.get("SOOP_PASSWORD").copied().unwrap_or(false);
                    self.state.provider_worker_api_key_set =
                        secrets.get("CLOUDFLARE_API_KEY").copied().unwrap_or(false);
                    self.state.provider_chzzk_nid_aut_set =
                        secrets.get("CHZZK_NID_AUT").copied().unwrap_or(false);
                    self.state.provider_chzzk_nid_ses_set =
                        secrets.get("CHZZK_NID_SES").copied().unwrap_or(false);
                    self.state.provider_kick_token_set =
                        secrets.get("KICK_SESSION_TOKEN").copied().unwrap_or(false);
                    self.state.provider_revision = self.state.provider_revision.wrapping_add(1);
                }
            }
            Response::ChannelPicked {
                index,
                expected,
                path,
            } => {
                if let Some(path) = path
                    && self
                        .channels_draft
                        .apply_selected_path(index, &expected, path)
                {
                    self.sync_channels_view(true);
                }
            }
            Response::ChannelResolved {
                index,
                account,
                name,
            } => {
                if let Some(row) = self.channels_draft.rows.get(index)
                    && row.account == account
                {
                    self.channels_draft.edit(index, "name", name.clone());
                    self.sync_channels_view(true);
                    self.state.config_message = format!("채널 이름을 확인했습니다: {name}");
                }
            }
            Response::ConfigMessage(message) => {
                self.state.config_busy = false;
                self.state.config_message = message;
            }
            Response::ConfigError(message) => {
                self.state.config_busy = false;
                self.state.config_message = message;
            }
            Response::Picked(index, selection) => {
                if self.settings_draft.accept_selection(index, selection) {
                    self.sync_settings_view(true);
                }
            }
            Response::Live {
                generation,
                status,
                refresh_thumbnails,
                message,
                poll,
            } => {
                if poll {
                    self.live_poll_in_flight = false;
                } else {
                    self.state.live_busy = false;
                }
                self.state.live_loaded = true;
                if let Some(message) = message {
                    self.state.live_message = message;
                }
                self.thumbnails.sync(&status, refresh_thumbnails);
                self.request_thumbnails();
                self.live_model.receive(status, generation);
                if self.window_visible {
                    if let Some(status) = self.live_model.take_for_display() {
                        self.render_live(status);
                    } else if let Some(status) = self.live_model.latest.clone() {
                        self.render_live_summary(&status);
                    }
                } else if let Some(status) = self.live_model.latest.clone() {
                    self.render_live_summary(&status);
                }
            }
            Response::LiveError { message, poll } => {
                if poll {
                    self.live_poll_in_flight = false;
                } else {
                    self.state.live_busy = false;
                    self.state.live_message = message;
                }
            }
            Response::Storage {
                generation,
                snapshot,
                message,
                poll,
            } => {
                if poll {
                    self.storage_poll_in_flight = false;
                } else {
                    self.state.storage_busy = false;
                }
                if !poll || self.storage_refresh.accepts(generation) {
                    self.render_storage(snapshot);
                    if let Some(message) = message {
                        self.state.storage_message = message;
                    }
                }
            }
            Response::StorageError {
                generation,
                message,
                poll,
            } => {
                if poll {
                    self.storage_poll_in_flight = false;
                } else {
                    self.state.storage_busy = false;
                }
                if !poll || self.storage_refresh.accepts(generation) {
                    self.state.storage_message = message;
                }
            }
            Response::Vod {
                status,
                message,
                poll,
            } => {
                if poll {
                    self.vod_poll_in_flight = false;
                } else {
                    self.state.vod_busy = false;
                }
                self.state.vod_loaded = true;
                if let Some(message) = message {
                    self.state.vod_message = message;
                }
                let thumb_job_id = status.job_id.clone();
                let sync = self.render_vod_status(status);
                if sync == AnalysisSync::Applied
                    && let Some(job_id) = thumb_job_id
                    && !job_id.is_empty()
                {
                    let _ = self.sender.send(Request::VodThumbnail {
                        generation: self.vod_draft.thumbnail_generation,
                        job_id,
                        url: self.vod_draft.url.clone(),
                    });
                }
            }
            Response::VodPicked(selection) => {
                if self.vod_draft.accept_output_selection(selection) {
                    self.sync_vod_draft();
                    self.state.vod_output_revision = self.state.vod_output_revision.wrapping_add(1);
                }
            }
            Response::VodError { message, poll } => {
                if poll {
                    self.vod_poll_in_flight = false;
                } else {
                    self.state.vod_busy = false;
                    self.state.vod_message = message;
                }
            }
            Response::Queue {
                snapshot,
                message,
                poll,
            } => {
                if poll {
                    self.queue_poll_in_flight = false;
                } else {
                    self.state.queue_busy = false;
                }
                self.state.queue_loaded = true;
                if let Some(message) = message {
                    self.state.queue_message = message;
                }
                self.render_queue(snapshot);
            }
            Response::QueueError { message, poll } => {
                if poll {
                    self.queue_poll_in_flight = false;
                } else {
                    self.state.queue_busy = false;
                    self.state.queue_message = message;
                }
            }
            Response::History {
                history,
                view,
                message,
            } => {
                self.state.history_busy = false;
                self.state.history_loaded = true;
                if self.history_refresh_queued {
                    self.refresh_history();
                } else {
                    if let Some(message) = message {
                        self.state.history_message = message;
                    }
                    self.render_history(history, &view);
                }
            }
            Response::HistoryError(message) => {
                self.state.history_busy = false;
                if self.history_refresh_queued {
                    self.refresh_history();
                } else {
                    self.state.history_message = message;
                }
            }
            Response::Maintenance {
                snapshot,
                diagnostics,
                logs,
                message,
            } => {
                self.state.maintenance_busy = false;
                self.state.maintenance_loaded = true;
                self.state.maintenance_message = message;
                self.render_maintenance(snapshot, diagnostics, logs);
            }
            Response::MaintenancePicked(selection) => {
                if let Some(path) = selection {
                    self.state.backup_directory = path;
                    self.state.backup_dir_revision = self.state.backup_dir_revision.wrapping_add(1);
                }
            }
            Response::Logs { lines, poll } => {
                if poll {
                    self.maintenance_log_poll_in_flight = false;
                }
                self.render_logs(lines);
            }
            Response::LogsCleared => {
                self.state.log_rows.clear();
                self.state.maintenance_message = "런타임 로그 버퍼를 비웠습니다.".into();
            }
            Response::LiveFolderOpened(result) => {
                if let Err(err) = result {
                    self.state.live_message = format!("폴더 열기 실패: {err}");
                }
            }
            Response::LogsFolderOpened(result) => {
                if let Err(err) = result {
                    self.state.maintenance_message = format!("로그 폴더 열기 실패: {err}");
                }
            }
            Response::MaintenanceError { message, poll } => {
                if poll {
                    self.maintenance_log_poll_in_flight = false;
                } else {
                    self.state.maintenance_busy = false;
                    self.state.maintenance_message = message;
                }
            }
            Response::Error(message) => {
                self.state.settings_busy = false;
                self.state.settings_message = message;
            }
            Response::DesktopStatus(summary) => {
                self.state.desktop_poll_pending = false;
                self.state.desktop_status = summary.clone();
                self.pending_signals
                    .push(DesktopSignal::StatusUpdated(summary));
            }
            Response::ExitCheck(_) => {}
            Response::ExitComplete => {
                self.pending_signals.push(DesktopSignal::ExitNow);
            }
            Response::Downloads(events) => {
                let summary = self
                    .notice_tracker
                    .collect(events, self.state.download_notifications_enabled);
                if !summary.is_empty() {
                    self.pending_signals.push(DesktopSignal::DownloadNotice {
                        completed: usize::try_from(summary.completed).unwrap_or(0),
                        failed: usize::try_from(summary.failed).unwrap_or(0),
                    });
                }
            }
            Response::ClearNotices => {
                self.notice_tracker.clear();
                self.pending_signals.push(DesktopSignal::ClearNotices);
            }
        }
    }

    fn request_live_snapshot(&mut self, generation: u64) {
        if self
            .sender
            .send(Request::LiveStatus {
                poll: true,
                generation,
            })
            .is_ok()
        {
            self.live_poll_in_flight = true;
        }
    }

    fn maybe_request_storage_reopen(&mut self) {
        if self
            .storage_refresh
            .needs_request(self.state.storage_busy, self.storage_poll_in_flight)
        {
            let generation = self.live_model.generation();
            if self
                .sender
                .send(Request::StorageLoad {
                    poll: true,
                    generation,
                })
                .is_ok()
            {
                self.storage_poll_in_flight = true;
                self.storage_refresh.requested();
            }
        }
    }

    fn request_thumbnails(&mut self) {
        request_thumbnails_with(
            self.window_visible,
            self.state.active_page,
            &mut self.thumbnails,
            &self.sender,
        );
    }

    fn sync_settings_view(&mut self, bump_revision: bool) {
        self.state.settings_rows = self
            .settings_draft
            .fields
            .iter()
            .map(|field| SettingRow {
                key: field.key.clone(),
                value: field.value.clone(),
                description: localized_setting_description(&field.key, &field.description).into(),
                pickable: field.kind != SettingKind::Text,
            })
            .collect();
        self.state.settings_dirty = !self.settings_draft.patch().is_empty();
        if bump_revision {
            self.state.settings_revision = self.state.settings_revision.wrapping_add(1);
        }
    }

    fn sync_channels_view(&mut self, bump_revision: bool) {
        self.state.channels_rows = self
            .channels_draft
            .rows
            .iter()
            .map(|row| ChannelConfigRow {
                platform: row.platform.to_string(),
                enabled: row.enabled,
                name: row.name.clone(),
                account: row.account.clone(),
                outdir: row.outdir.clone(),
            })
            .collect();
        self.state.channels_dirty = self.channels_draft.dirty();
        if bump_revision {
            self.state.channels_revision = self.state.channels_revision.wrapping_add(1);
        }
    }

    fn render_live_summary(&mut self, status: &NativeWatcherStatus) {
        self.state.live_running = status.running;
        self.state.live_state_label = if status.running {
            "실행 중"
        } else {
            "중지됨"
        }
        .into();
        self.state.live_engine = status.engine.into();
        self.state.live_started_at = status.started_at.clone().unwrap_or_else(|| "-".into());
        self.state.live_channel_count = status.channel_count.to_string();
        self.state.live_recording_count = status.recording_count.to_string();
        self.state.live_offline_count = status.offline_count.to_string();
        self.state.live_error_count = status.error_count.to_string();
    }

    fn render_live(&mut self, status: NativeWatcherStatus) {
        let view = live_adapter::view(status);
        self.state.live_running = view.running;
        self.state.live_state_label = view.state_label;
        self.state.live_engine = view.engine;
        self.state.live_started_at = view.started_at;
        self.state.live_channel_count = view.channel_count;
        self.state.live_recording_count = view.recording_count;
        self.state.live_offline_count = view.offline_count;
        self.state.live_error_count = view.error_count;

        let filter = self.live_model.filter().to_string();
        let rows: Vec<LiveChannelRow> = view
            .channels
            .into_iter()
            .filter(|ch| LiveModel::matches_filter(&filter, &ch.status))
            .map(|ch| {
                let thumbnail_image = self.thumbnails.image(&ch.target);
                LiveChannelRow {
                    target: ch.target,
                    platform: ch.platform,
                    name: ch.name,
                    account: ch.account,
                    status: ch.status,
                    status_label: ch.status_label,
                    status_tone: ch.status_tone,
                    title: ch.title,
                    bno: ch.bno,
                    file: ch.file,
                    output_dir: ch.output_dir,
                    size: ch.size,
                    started_at: ch.started_at,
                    suppressed: ch.suppressed,
                    detail: ch.detail,
                    can_stop_once: ch.can_stop_once,
                    can_resume: ch.can_resume,
                    can_recheck: ch.can_recheck,
                    password_required: ch.password_required,
                    thumbnail_image,
                }
            })
            .collect();
        self.live_model.reconcile(rows);
    }

    fn render_storage(&mut self, snapshot: StorageSnapshot) {
        self.state.storage_loaded = true;
        self.state.storage_threshold = storage_adapter::threshold_label(snapshot.threshold_gb);
        self.state.storage_rows = storage_adapter::rows(&snapshot)
            .into_iter()
            .map(|row| StorageDisplayRow {
                volume: row.volume,
                roles: row.roles,
                paths: row.paths,
                capacity: row.capacity,
                used: row.used,
                status: row.status,
                status_tone: row.status_tone,
                detail: row.detail,
            })
            .collect();
    }

    fn sync_vod_draft(&mut self) {
        self.state.vod_url = self.vod_draft.url.clone();
        self.state.vod_output_directory = self.vod_draft.output_directory.clone();
        self.state.vod_merge = self.vod_draft.merge;
        self.state.vod_can_analyze = self.vod_draft.can_analyze();
        self.state.vod_can_download = self.vod_draft.can_download();
        self.state.vod_can_enqueue = self.vod_draft.can_download();
        self.state.vod_qualities = self
            .vod_draft
            .qualities
            .iter()
            .map(|q| VodQualityRow {
                value: q.value.clone(),
                label: q.label.clone(),
                selected: q.selected,
            })
            .collect();
        self.state.vod_parts = self
            .vod_draft
            .parts
            .iter()
            .map(|p| VodPartRow {
                part: p.part,
                duration: vod_adapter::format_duration(p.duration_seconds),
                selected: p.selected,
            })
            .collect();
    }

    fn render_vod_status(&mut self, status: VodJobStatus) -> AnalysisSync {
        let sync = status
            .analysis
            .as_ref()
            .map_or(AnalysisSync::Stale, |analysis| {
                self.vod_draft.sync_analysis(analysis)
            });
        let duration_secs: u64 = status
            .analysis
            .as_ref()
            .map(|analysis| analysis.parts.iter().map(|p| p.duration_seconds).sum())
            .unwrap_or(0);
        let view = vod_adapter::view(&status);
        self.state.vod_running = view.running;
        self.state.vod_can_cancel = view.running;
        self.state.vod_platform = view.platform;
        self.state.vod_title = view.title;
        self.state.vod_streamer = view.streamer;
        self.state.vod_streamer_id = view.streamer_id;
        self.state.vod_duration = vod_adapter::format_duration(duration_secs);
        self.state.vod_started_at = view.started_at;
        self.state.vod_finished_at = view.finished_at;
        self.state.vod_state = status.state;
        self.state.vod_state_label = view.state;
        self.state.vod_state_tone = view.state_tone;
        self.state.vod_percent = view.percent;
        self.state.vod_percent_label = view.percent_label;
        self.state.vod_part_progress = if view.part_count == "0" {
            "-".into()
        } else {
            format!("PART {} / {}", view.current_part, view.part_count)
        };
        self.state.vod_status_message = view.message;
        self.state.vod_output_file = view.output_file;
        self.sync_vod_draft();
        sync
    }

    #[cfg(test)]
    pub fn apply_response_for_test(&mut self, response: Response) {
        self.apply_response(response);
    }

    #[cfg(test)]
    pub fn apply_queue_snapshot_for_test(&mut self, snapshot: VodQueueSnapshot) {
        self.render_queue(snapshot);
    }

    fn render_queue(&mut self, snapshot: VodQueueSnapshot) {
        let view = queue_adapter::view(snapshot);
        self.state.queue_active_count = view.active;
        self.state.queue_queued_count = view.queued;
        self.state.queue_completed_count = view.completed;
        self.state.queue_failed_count = view.failed;
        self.state.queue_rows = view
            .rows
            .into_iter()
            .map(|row| QueueDisplayRow {
                id: row.id,
                platform: row.platform,
                title: row.title,
                streamer: row.streamer,
                url: row.url,
                state: row.state,
                state_label: row.state_label,
                state_tone: row.state_tone,
                attempts: row.attempts,
                message: row.message,
                percent: row.percent,
                percent_label: row.percent_label,
                part_progress: row.part_progress,
                output_directory: row.output_directory,
                output_file: row.output_file,
                created_at: row.created_at,
                started_at: row.started_at,
                finished_at: row.finished_at,
                can_cancel: row.can_cancel,
                can_retry: row.can_retry,
                can_remove: row.can_remove,
            })
            .collect();
        if !self.state.expanded_queue_id.is_empty()
            && !self
                .state
                .queue_rows
                .iter()
                .any(|row| row.id == self.state.expanded_queue_id)
        {
            self.state.expanded_queue_id.clear();
        }
    }

    fn render_history(&mut self, history: HistoryResponse, view: &str) {
        self.state.history_rows = history_adapter::rows(history, view)
            .into_iter()
            .map(|row| HistoryDisplayRow {
                kind: row.kind,
                platform: row.platform,
                title: row.title,
                subject: row.subject,
                state: row.state,
                state_tone: row.state_tone,
                detail: row.detail,
                timing: row.timing,
                file: row.file,
                meta: row.meta,
            })
            .collect();
    }

    fn apply_calendar(&mut self, cal: history_adapter::CalendarMonthView) {
        self.state.history_calendar_year = cal.year;
        self.state.history_calendar_month = cal.month;
        self.state.history_calendar_label = cal.label;
        self.state.history_calendar_days = cal
            .days
            .into_iter()
            .map(|d| HistoryCalendarDay {
                day: d.day,
                date: d.date,
                in_month: d.in_month,
                selected: d.selected,
            })
            .collect();
    }

    fn render_logs(&mut self, lines: Vec<String>) {
        self.state.log_rows = maintenance_adapter::log_rows(lines, 200)
            .into_iter()
            .map(|row| MaintenanceLogRow { text: row.text })
            .collect();
    }

    fn render_maintenance(
        &mut self,
        snapshot: BackupSnapshot,
        diagnostics: DiagnosticsSnapshot,
        logs: Vec<String>,
    ) {
        self.state.backup_enabled = snapshot.policy.enabled;
        self.state.backup_interval_hours = snapshot.policy.interval_hours.to_string();
        self.state.backup_keep_count = snapshot.policy.keep_count.to_string();
        self.state.backup_retention_days = snapshot.policy.retention_days.to_string();
        self.state.backup_directory = snapshot.directory.clone();
        self.state.backup_directory_locked = !snapshot.directory_editable;
        self.state.backup_revision = self.state.backup_revision.wrapping_add(1);
        self.state.backup_dir_revision = self.state.backup_dir_revision.wrapping_add(1);
        self.state.backup_rows = maintenance_adapter::backup_rows(&snapshot)
            .into_iter()
            .map(|row| MaintenanceBackupRow {
                file_name: row.file_name,
                kind: row.kind,
                created_at: row.created_at,
                size: row.size,
                sha256: row.sha256,
                integrity: row.integrity,
                integrity_tone: row.integrity_tone,
                can_restore: row.can_restore,
            })
            .collect();
        self.state.maintenance_diagnostic_rows = maintenance_adapter::diagnostic_rows(&diagnostics)
            .into_iter()
            .map(|row| MaintenanceDiagnosticRow {
                category: row.category,
                requirement: row.requirement,
                name: row.name,
                status: row.status,
                summary: row.summary,
                detail: row.detail,
                remediation: row.remediation,
                status_tone: row.status_tone,
            })
            .collect();
        self.render_logs(logs);
    }
}

impl Drop for AppController {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        let _ = self.sender.send(Request::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn bind_core_snapshot(state: &mut UiState, diagnostics: DiagnosticsSnapshot) {
    state.diagnostics_rows = maintenance_adapter::diagnostic_rows(&diagnostics)
        .into_iter()
        .map(|row| DiagnosticRow {
            name: row.name,
            status: row.status,
            detail: row.detail,
        })
        .collect();
}

pub fn live_poll_needed(
    page: &str,
    visible: bool,
    busy: bool,
    in_flight: bool,
    tracking_thumbnails: bool,
) -> bool {
    !busy && !in_flight && ((visible && page == "LIVE") || tracking_thumbnails)
}

fn request_thumbnails_with(
    visible: bool,
    page: Page,
    thumbnails: &mut Thumbnails,
    sender: &mpsc::Sender<Request>,
) {
    if !visible || page != Page::Live {
        return;
    }
    for (token, platform, account, broadcast_id, completion_revision) in thumbnails.requests() {
        let _ = sender.send(Request::Thumbnail {
            token,
            platform,
            account,
            broadcast_id,
            completion_revision,
        });
    }
}

#[cfg(test)]
mod thumbnail_poll_tests {
    use super::*;
    use stream_archive_server::{model::ChannelRuntimeStatus, support::platform::PlatformId};

    #[test]
    fn cached_images_observe_completion_even_on_other_tabs_without_duplicate_polls() {
        assert!(live_poll_needed("LIVE", true, false, false, false));
        assert!(!live_poll_needed("LIVE", false, false, false, false));
        assert!(live_poll_needed("LIVE", false, false, false, true));
        for page in ["Channels", "Queue", "History", "Settings", "Diagnostics"] {
            assert!(!live_poll_needed(page, true, false, false, false));
            assert!(live_poll_needed(page, true, false, false, true));
            assert!(!live_poll_needed(page, true, true, false, true));
            assert!(!live_poll_needed(page, true, false, true, true));
        }
    }

    #[test]
    fn hidden_live_window_does_not_fetch_or_drain_thumbnails_until_reopened() {
        let mut status = NativeWatcherStatus {
            running: true,
            channels: (0..4)
                .map(|index| ChannelRuntimeStatus {
                    platform: PlatformId::Soop,
                    account: format!("test{index}"),
                    status: "RECORDING".into(),
                    bno: Some("1".into()),
                    thumbnail_url: Some("https://fixture.invalid/thumbnail.jpg".into()),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let mut thumbnails = Thumbnails::default();
        thumbnails.sync(&status, false);
        let (sender, receiver) = mpsc::channel();
        request_thumbnails_with(false, Page::Live, &mut thumbnails, &sender);
        assert!(receiver.try_recv().is_err());

        request_thumbnails_with(true, Page::Live, &mut thumbnails, &sender);
        let tokens: Vec<_> = receiver
            .try_iter()
            .map(|request| {
                let Request::Thumbnail { token, .. } = request else {
                    panic!("thumbnail");
                };
                token
            })
            .collect();
        assert_eq!(tokens.len(), 2);

        for token in tokens {
            thumbnails.complete(token, None);
            request_thumbnails_with(false, Page::Live, &mut thumbnails, &sender);
            assert!(receiver.try_recv().is_err());
        }
        status.channels[0].bno = Some("2".into());
        thumbnails.sync(&status, false);
        request_thumbnails_with(false, Page::Live, &mut thumbnails, &sender);
        assert!(receiver.try_recv().is_err());

        request_thumbnails_with(true, Page::Live, &mut thumbnails, &sender);
        assert_eq!(receiver.try_iter().count(), 2);
    }
}
