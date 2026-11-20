//! Pure-Rust presentation state for the GPUI desktop shell.
//! Holds only presentation view-models; runtime persistence, process ownership,
//! and secret protection stay in `StreamArchiveCore`.

use gpui_kit::RenderImage;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Live,
    Channels,
    Vod,
    Queue,
    History,
    Settings,
    Diagnostics,
}

impl Page {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "LIVE",
            Self::Channels => "Channels",
            Self::Vod => "VOD",
            Self::Queue => "Queue",
            Self::History => "History",
            Self::Settings => "Settings",
            Self::Diagnostics => "Diagnostics",
        }
    }

    pub fn title_ko(self) -> &'static str {
        match self {
            Self::Live => "LIVE 감시",
            Self::Channels => "채널 관리",
            Self::Vod => "VOD 분석",
            Self::Queue => "대기열",
            Self::History => "기록",
            Self::Settings => "설정",
            Self::Diagnostics => "진단 · 로그",
        }
    }

    pub fn breadcrumb(self) -> &'static str {
        match self {
            Self::Live => "live.monitor",
            Self::Channels => "channels.config",
            Self::Vod => "vod.analyzer",
            Self::Queue => "queue.runner",
            Self::History => "history.archive",
            Self::Settings => "settings.system",
            Self::Diagnostics => "diagnostics.logs",
        }
    }

    pub const ALL: [Self; 7] = [
        Self::Live,
        Self::Channels,
        Self::Vod,
        Self::Queue,
        Self::History,
        Self::Settings,
        Self::Diagnostics,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsView {
    #[default]
    General,
    Manage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaintenanceSection {
    #[default]
    Backup,
    Diagnostics,
    Logs,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingRow {
    pub key: String,
    pub value: String,
    pub description: String,
    pub pickable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticRow {
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelConfigRow {
    pub platform: String,
    pub enabled: bool,
    pub name: String,
    pub account: String,
    pub outdir: String,
}

#[derive(Debug, Clone, Default)]
pub struct LiveChannelRow {
    pub target: String,
    pub platform: String,
    pub name: String,
    pub account: String,
    pub status: String,
    pub status_label: String,
    pub status_tone: String,
    pub title: String,
    pub bno: String,
    pub file: String,
    pub output_dir: String,
    pub size: String,
    pub started_at: String,
    pub suppressed: bool,
    pub detail: String,
    pub can_stop_once: bool,
    pub can_resume: bool,
    pub can_recheck: bool,
    pub password_required: bool,
    pub thumbnail_image: Option<Arc<RenderImage>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageDisplayRow {
    pub volume: String,
    pub roles: String,
    pub paths: String,
    pub capacity: String,
    pub used: String,
    pub status: String,
    pub status_tone: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VodQualityRow {
    pub value: String,
    pub label: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VodPartRow {
    pub part: usize,
    pub duration: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueueDisplayRow {
    pub id: String,
    pub platform: String,
    pub title: String,
    pub streamer: String,
    pub url: String,
    pub state: String,
    pub state_label: String,
    pub state_tone: String,
    pub attempts: String,
    pub message: String,
    pub percent: f32,
    pub percent_label: String,
    pub part_progress: String,
    pub output_directory: String,
    pub output_file: String,
    pub created_at: String,
    pub started_at: String,
    pub finished_at: String,
    pub can_cancel: bool,
    pub can_retry: bool,
    pub can_remove: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryDisplayRow {
    pub kind: String,
    pub platform: String,
    pub title: String,
    pub subject: String,
    pub state: String,
    pub state_tone: String,
    pub detail: String,
    pub timing: String,
    pub file: String,
    pub meta: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryCalendarDay {
    pub day: String,
    pub date: String,
    pub in_month: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaintenanceBackupRow {
    pub file_name: String,
    pub kind: String,
    pub created_at: String,
    pub size: String,
    pub sha256: String,
    pub integrity: String,
    pub integrity_tone: String,
    pub can_restore: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaintenanceDiagnosticRow {
    pub category: String,
    pub requirement: String,
    pub name: String,
    pub status: String,
    pub summary: String,
    pub detail: String,
    pub remediation: String,
    pub status_tone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaintenanceLogRow {
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct UiState {
    pub app_version: String,
    pub active_page: Page,
    pub settings_view: SettingsView,
    pub runtime_ready: bool,
    pub startup_failed: bool,
    pub first_run: bool,

    // Desktop & Close dialog
    pub close_action: String,
    pub close_to_tray: bool,
    pub close_dialog_visible: bool,
    pub close_remember: bool,
    pub close_choice_busy: bool,
    pub close_choice_error: String,
    pub desktop_exit_pending: bool,
    pub desktop_poll_pending: bool,
    pub desktop_status: String,
    pub desktop_recovery_message: String,
    pub download_notifications_enabled: bool,

    // General Settings & Providers
    pub settings_rows: Vec<SettingRow>,
    pub settings_revision: u64,
    pub diagnostics_rows: Vec<DiagnosticRow>,
    pub settings_busy: bool,
    pub settings_loaded: bool,
    pub settings_dirty: bool,
    pub settings_message: String,

    pub channels_rows: Vec<ChannelConfigRow>,
    pub channels_revision: u64,
    pub channels_dirty: bool,
    pub config_busy: bool,
    pub config_loaded: bool,
    pub config_message: String,
    pub provider_soop_username: String,
    pub provider_soop_password: String,
    pub provider_soop_password_set: bool,
    pub provider_worker_url: String,
    pub provider_worker_api_key: String,
    pub provider_worker_api_key_set: bool,
    pub provider_chzzk_nid_aut: String,
    pub provider_chzzk_nid_aut_set: bool,
    pub provider_chzzk_nid_ses: String,
    pub provider_chzzk_nid_ses_set: bool,
    pub provider_kick_token: String,
    pub provider_kick_token_set: bool,
    pub provider_revision: u64,

    // LIVE & Storage
    pub live_running: bool,
    pub live_busy: bool,
    pub live_loaded: bool,
    pub live_state_label: String,
    pub live_engine: String,
    pub live_started_at: String,
    pub live_channel_count: String,
    pub live_recording_count: String,
    pub live_offline_count: String,
    pub live_error_count: String,
    pub live_message: String,
    pub live_filter: String,
    pub password_prompt_target: Option<String>,
    pub password_prompt_name: String,

    pub storage_rows: Vec<StorageDisplayRow>,
    pub storage_busy: bool,
    pub storage_loaded: bool,
    pub storage_threshold: String,
    pub storage_message: String,

    // VOD
    pub vod_url: String,
    pub vod_output_directory: String,
    pub vod_output_revision: u64,
    pub vod_merge: bool,
    pub vod_busy: bool,
    pub vod_loaded: bool,
    pub vod_running: bool,
    pub vod_can_analyze: bool,
    pub vod_can_download: bool,
    pub vod_can_enqueue: bool,
    pub vod_can_cancel: bool,
    pub vod_platform: String,
    pub vod_title: String,
    pub vod_streamer: String,
    pub vod_streamer_id: String,
    pub vod_duration: String,
    pub vod_started_at: String,
    pub vod_finished_at: String,
    pub vod_state: String,
    pub vod_state_label: String,
    pub vod_state_tone: String,
    pub vod_percent: f32,
    pub vod_percent_label: String,
    pub vod_part_progress: String,
    pub vod_status_message: String,
    pub vod_message: String,
    pub vod_output_file: String,
    pub vod_thumbnail_image: Option<Arc<RenderImage>>,
    pub vod_qualities: Vec<VodQualityRow>,
    pub vod_parts: Vec<VodPartRow>,

    // Queue
    pub queue_rows: Vec<QueueDisplayRow>,
    pub expanded_queue_id: String,
    pub queue_busy: bool,
    pub queue_loaded: bool,
    pub queue_message: String,
    pub queue_active_count: String,
    pub queue_queued_count: String,
    pub queue_completed_count: String,
    pub queue_failed_count: String,

    // History
    pub history_rows: Vec<HistoryDisplayRow>,
    pub history_busy: bool,
    pub history_loaded: bool,
    pub history_message: String,
    pub history_view: String,
    pub history_search: String,
    pub history_status: String,
    pub history_from_date: String,
    pub history_to_date: String,
    pub history_limit: String,
    pub history_calendar_open: bool,
    pub history_calendar_target: String,
    pub history_calendar_label: String,
    pub history_calendar_year: i32,
    pub history_calendar_month: u32,
    pub history_calendar_days: Vec<HistoryCalendarDay>,

    // Maintenance (Backup / Diagnostics / Logs)
    pub maintenance_section: MaintenanceSection,
    pub maintenance_busy: bool,
    pub maintenance_loaded: bool,
    pub maintenance_message: String,
    pub backup_enabled: bool,
    pub backup_interval_hours: String,
    pub backup_keep_count: String,
    pub backup_retention_days: String,
    pub backup_directory: String,
    pub backup_directory_locked: bool,
    pub backup_revision: u64,
    pub backup_rows: Vec<MaintenanceBackupRow>,
    pub restore_confirm_file: Option<String>,
    pub maintenance_diagnostic_rows: Vec<MaintenanceDiagnosticRow>,
    pub log_rows: Vec<MaintenanceLogRow>,
    pub log_auto_refresh: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            app_version: format!("v{}", env!("CARGO_PKG_VERSION")),
            active_page: Page::Live,
            settings_view: SettingsView::General,
            runtime_ready: false,
            startup_failed: false,
            first_run: false,

            close_action: "EXIT".into(),
            close_to_tray: false,
            close_dialog_visible: false,
            close_remember: false,
            close_choice_busy: false,
            close_choice_error: String::new(),
            desktop_exit_pending: false,
            desktop_poll_pending: false,
            desktop_status: "상태 확인 중".into(),
            desktop_recovery_message: String::new(),
            download_notifications_enabled: true,

            settings_rows: Vec::new(),
            settings_revision: 0,
            diagnostics_rows: Vec::new(),
            settings_busy: true,
            settings_loaded: false,
            settings_dirty: false,
            settings_message: "설정을 불러오는 중...".into(),

            channels_rows: Vec::new(),
            channels_revision: 0,
            channels_dirty: false,
            config_busy: true,
            config_loaded: false,
            config_message: "채널 및 서비스 연결 정보를 불러오는 중...".into(),
            provider_soop_username: String::new(),
            provider_soop_password: String::new(),
            provider_soop_password_set: false,
            provider_worker_url: String::new(),
            provider_worker_api_key: String::new(),
            provider_worker_api_key_set: false,
            provider_chzzk_nid_aut: String::new(),
            provider_chzzk_nid_aut_set: false,
            provider_chzzk_nid_ses: String::new(),
            provider_chzzk_nid_ses_set: false,
            provider_kick_token: String::new(),
            provider_kick_token_set: false,
            provider_revision: 0,

            live_running: false,
            live_busy: true,
            live_loaded: false,
            live_state_label: "확인 중".into(),
            live_engine: "-".into(),
            live_started_at: "-".into(),
            live_channel_count: "0".into(),
            live_recording_count: "0".into(),
            live_offline_count: "0".into(),
            live_error_count: "0".into(),
            live_message: "LIVE 상태를 불러오는 중...".into(),
            live_filter: "ALL".into(),
            password_prompt_target: None,
            password_prompt_name: String::new(),

            storage_rows: Vec::new(),
            storage_busy: true,
            storage_loaded: false,
            storage_threshold: "-".into(),
            storage_message: "저장공간을 확인하는 중...".into(),

            vod_url: String::new(),
            vod_output_directory: String::new(),
            vod_output_revision: 0,
            vod_merge: true,
            vod_busy: true,
            vod_loaded: false,
            vod_running: false,
            vod_can_analyze: false,
            vod_can_download: false,
            vod_can_enqueue: false,
            vod_can_cancel: false,
            vod_platform: String::new(),
            vod_title: String::new(),
            vod_streamer: String::new(),
            vod_streamer_id: String::new(),
            vod_duration: String::new(),
            vod_started_at: "-".into(),
            vod_finished_at: "-".into(),
            vod_state: "IDLE".into(),
            vod_state_label: "대기".into(),
            vod_state_tone: "muted".into(),
            vod_percent: 0.0,
            vod_percent_label: "0.0%".into(),
            vod_part_progress: "-".into(),
            vod_status_message: String::new(),
            vod_message: "VOD 상태를 불러오는 중...".into(),
            vod_output_file: String::new(),
            vod_thumbnail_image: None,
            vod_qualities: Vec::new(),
            vod_parts: Vec::new(),

            queue_rows: Vec::new(),
            expanded_queue_id: String::new(),
            queue_busy: true,
            queue_loaded: false,
            queue_message: "VOD 대기열을 불러오는 중...".into(),
            queue_active_count: "0".into(),
            queue_queued_count: "0".into(),
            queue_completed_count: "0".into(),
            queue_failed_count: "0".into(),

            history_rows: Vec::new(),
            history_busy: true,
            history_loaded: false,
            history_message: "기록을 불러오는 중...".into(),
            history_view: "ALL".into(),
            history_search: String::new(),
            history_status: String::new(),
            history_from_date: String::new(),
            history_to_date: String::new(),
            history_limit: "100".into(),
            history_calendar_open: false,
            history_calendar_target: "from".into(),
            history_calendar_label: String::new(),
            history_calendar_year: 0,
            history_calendar_month: 0,
            history_calendar_days: Vec::new(),

            maintenance_section: MaintenanceSection::Backup,
            maintenance_busy: true,
            maintenance_loaded: false,
            maintenance_message: "관리 상태를 불러오는 중...".into(),
            backup_enabled: true,
            backup_interval_hours: "24".into(),
            backup_keep_count: "10".into(),
            backup_retention_days: "3".into(),
            backup_directory: String::new(),
            backup_directory_locked: false,
            backup_revision: 0,
            backup_rows: Vec::new(),
            restore_confirm_file: None,
            maintenance_diagnostic_rows: Vec::new(),
            log_rows: Vec::new(),
            log_auto_refresh: false,
        }
    }
}
