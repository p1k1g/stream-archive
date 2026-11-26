//! Request and response protocol between GPUI `AppController` and `core_worker`.

use std::collections::BTreeMap;
use stream_archive_server::{
    backup_service::{BackupPolicy, BackupSnapshot},
    diagnostics::DiagnosticsSnapshot,
    environment_settings::{EnvironmentSetting, SettingKind},
    history_service::HistoryFilter,
    model::{
        Channel, HistoryResponse, NativeWatcherStatus, VodDownloadRequest, VodJobStatus,
        VodQueueSnapshot,
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
        telegram_chat_id: String,
        secrets: BTreeMap<String, String>,
    },
    ProviderTestSoop,
    WebhookTest,
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
    QueueEnqueueBatch {
        urls: Vec<String>,
        quality: String,
        output_directory: String,
        merge: bool,
    },
    QueueAction {
        id: String,
        action: String,
    },
    QueueClearCompleted,
    QueueRetryFailed,
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
    OpenMediaFile(String),
    OpenLiveStream {
        platform: String,
        account: String,
    },
    OpenLogsFolder,
    OpenReleaseUrl(String),
    CheckUpdates {
        manual: bool,
    },
    SetWindowsStartup(bool),
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
        telegram_chat_id: String,
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
    QueueBatch {
        snapshot: VodQueueSnapshot,
        message: String,
        clear_input: bool,
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
    MediaFileOpened(Result<(), String>),
    LiveStreamOpened(Result<(), String>),
    LogsFolderOpened(Result<(), String>),
    ReleaseUrlOpened(Result<(), String>),
    UpdateCheck {
        manual: bool,
        result: Result<stream_archive_server::update_check::ReleaseUpdateInfo, String>,
    },
    WindowsStartup(Result<bool, String>),
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
