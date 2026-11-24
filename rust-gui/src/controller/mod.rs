//! Presentation controller bridging GPUI views, `core_worker`, and `windows_tray`.
//! Holds no direct SQLite, HTTP, or process-management code.

mod channels_settings;
mod desktop_poll;
mod live;
mod responses;
mod vod_queue_history;

use crate::{
    channels_adapter::ChannelsDraft,
    core_worker::{self, Request, Response},
    history_adapter,
    live_model::{LiveModel, StorageRefresh},
    maintenance_adapter, notifications,
    settings_adapter::SettingsDraft,
    state::{DiagnosticRow, MaintenanceSection, Page, SettingsView, UiState},
    thumbnail_adapter::Thumbnails,
    vod_adapter::VodDraft,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use stream_archive_server::diagnostics::DiagnosticsSnapshot;

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
    EnsureWindowVisible,
    ApplyWindowBounds(String),
    WatcherStateChanged(bool),
    DownloadNotice {
        completed: usize,
        failed: usize,
    },
    TrayAlert {
        title: String,
        body: String,
        warning: bool,
    },
    ClearNotices,
    StatusUpdated(String),
}

pub struct AppController {
    pub state: UiState,
    pub live_model: LiveModel,
    pub(super) settings_draft: SettingsDraft,
    pub(super) channels_draft: ChannelsDraft,
    pub(super) vod_draft: VodDraft,
    pub(super) thumbnails: Thumbnails,
    pub(super) storage_refresh: StorageRefresh,
    pub(super) notice_tracker: notifications::Tracker,
    pub(super) resource_trace: Option<stream_archive_server::resource_trace::ResourceTrace>,

    pub(super) sender: mpsc::Sender<Request>,
    pub(super) responses: mpsc::Receiver<Response>,
    worker: Option<std::thread::JoinHandle<()>>,
    pub(super) stopping: Arc<AtomicBool>,

    pub(super) window_visible: bool,
    pub(super) live_poll_in_flight: bool,
    pub(super) storage_poll_in_flight: bool,
    pub(super) vod_poll_in_flight: bool,
    pub(super) queue_poll_in_flight: bool,
    pub(super) maintenance_log_poll_in_flight: bool,
    pub(super) history_refresh_queued: bool,
    pub(super) last_live_poll: Instant,
    pub(super) last_storage_poll: Instant,
    pub(super) last_vod_poll: Instant,
    pub(super) last_queue_poll: Instant,
    pub(super) last_log_poll: Instant,
    pub(super) last_trace_emit: Instant,
    pub(super) last_tray_alert_poll: Instant,
    pub(super) last_tray_storage_poll: Instant,

    pub(super) recording_speed_samples: HashMap<String, (u64, Instant, f64)>,
    pub(super) last_saved_window_bounds: Option<String>,
    pub(super) window_bounds_initialized: bool,
    pub(super) window_opened_before_bounds: bool,
    pub(super) auto_start_watcher_triggered: bool,
    pub(super) seen_initial_live: bool,
    pub(super) last_watcher_running: Option<bool>,
    pub(super) notified_recordings: HashSet<(String, String)>,
    pub(super) notified_error_targets: HashSet<String>,
    pub(super) notified_critical_volumes: HashSet<String>,

    pub(super) pending_signals: Vec<DesktopSignal>,
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
            last_tray_alert_poll: now,
            last_tray_storage_poll: now,
            recording_speed_samples: HashMap::new(),
            last_saved_window_bounds: None,
            window_bounds_initialized: false,
            window_opened_before_bounds: false,
            auto_start_watcher_triggered: false,
            seen_initial_live: false,
            last_watcher_running: None,
            notified_recordings: HashSet::new(),
            notified_error_targets: HashSet::new(),
            notified_critical_volumes: HashSet::new(),
            pending_signals: Vec::new(),
        }
    }

    pub fn wait_initial_snapshot(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while !self.state.settings_loaded && !self.state.startup_failed {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            match self.responses.recv_timeout(remaining) {
                Ok(response) => self.apply_response(response),
                Err(_) => break,
            }
        }
    }

    pub fn mark_initial_window_opened(&mut self) {
        if !self.window_bounds_initialized {
            self.window_opened_before_bounds = true;
        }
    }

    pub fn save_window_bounds(&mut self, spec: String) {
        if !self.window_bounds_initialized {
            return;
        }
        self.state.window_bounds_spec = Some(spec.clone());
        if self.last_saved_window_bounds.as_deref() == Some(spec.as_str()) {
            return;
        }
        self.last_saved_window_bounds = Some(spec.clone());
        let _ = self.sender.send(Request::SaveWindowBounds(spec));
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

    pub fn dismiss_update_banner(&mut self) {
        self.state.update_dismissed = true;
    }

    pub fn should_start_minimized_to_tray(&self) -> bool {
        self.state.start_minimized_tray && !self.state.first_run && !self.state.startup_failed
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

pub(super) fn request_thumbnails_with(
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
    use stream_archive_server::{
        model::{ChannelRuntimeStatus, NativeWatcherStatus},
        support::platform::PlatformId,
    };

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
