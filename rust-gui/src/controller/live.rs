//! LIVE watcher, storage diagnostics, transfer speed tracking, and alert handling.

use super::{ActiveRecordingMeta, AppController, DesktopSignal, request_thumbnails_with};
use crate::{
    core_worker::Request,
    formatting::{format_bytes, format_elapsed_since, format_transfer_rate},
    live_adapter,
    live_model::LiveModel,
    state::{LiveChannelRow, StorageDisplayRow},
    storage_adapter,
};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use stream_archive_server::{model::NativeWatcherStatus, storage_service::StorageSnapshot};

impl AppController {
    pub fn live_start(&mut self) {
        if self.state.live_busy || !self.state.runtime_ready || self.state.desktop_exit_pending {
            return;
        }
        self.state.live_busy = true;
        self.state.live_message = "LIVE Watcher를 시작하는 중...".into();
        let _ = self.sender.send(Request::LiveStart);
    }

    pub fn live_stop(&mut self) {
        if self.state.live_busy || !self.state.runtime_ready || self.state.desktop_exit_pending {
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

    pub fn set_live_search(&mut self, query: String) {
        if self.live_model.set_search(query.clone()) {
            self.state.live_search = query;
            if let Some(status) = self.live_model.latest.clone() {
                self.render_live(status);
            }
        }
    }

    pub fn set_live_sort_mode(&mut self, mode: &str) {
        if self.live_model.set_sort_mode(mode) {
            self.state.live_sort_mode = self.live_model.sort_mode().to_string();
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

    pub fn open_media_file(&mut self, file_path: String) {
        let _ = self.sender.send(Request::OpenMediaFile(file_path));
    }

    pub(super) fn maybe_trigger_auto_start_watcher(&mut self) {
        if self.auto_start_watcher_triggered
            || self.initial_auto_start_watcher != Some(true)
            || !self.state.settings_loaded
            || !self.state.live_loaded
            || !self.state.runtime_ready
            || self.state.first_run
            || self.state.live_running
            || self.state.live_busy
            || self.state.desktop_exit_pending
        {
            return;
        }
        self.auto_start_watcher_triggered = true;
        self.live_start();
    }

    pub fn live_open_stream(&mut self, platform: String, account: String) {
        let _ = self
            .sender
            .send(Request::OpenLiveStream { platform, account });
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

    pub(super) fn request_live_snapshot(&mut self, generation: u64) {
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

    pub(super) fn maybe_request_storage_reopen(&mut self) {
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

    pub(super) fn request_thumbnails(&mut self) {
        request_thumbnails_with(
            self.window_visible,
            self.state.active_page,
            &mut self.thumbnails,
            &self.sender,
        );
    }

    pub(super) fn render_live_summary(&mut self, status: &NativeWatcherStatus) {
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

    pub(super) fn update_recording_speeds(&mut self, status: &NativeWatcherStatus) {
        let now = Instant::now();
        let mut active_targets = HashSet::new();
        for ch in &status.channels {
            if ch.status != "RECORDING" {
                continue;
            }
            let target = live_adapter::scoped_target(ch.platform, &ch.account);
            active_targets.insert(target.clone());
            match self.recording_speed_samples.get_mut(&target) {
                Some((prev_bytes, prev_time, prev_bps)) => {
                    if ch.size_bytes > *prev_bytes {
                        let dt = now.duration_since(*prev_time).as_secs_f64();
                        if dt >= 0.4 {
                            let inst_bps = (ch.size_bytes - *prev_bytes) as f64 / dt;
                            *prev_bps = if *prev_bps > 0.0 {
                                *prev_bps * 0.6 + inst_bps * 0.4
                            } else {
                                inst_bps
                            };
                            *prev_bytes = ch.size_bytes;
                            *prev_time = now;
                        }
                    } else if ch.size_bytes < *prev_bytes {
                        *prev_bytes = ch.size_bytes;
                        *prev_time = now;
                        *prev_bps = 0.0;
                    } else if now.duration_since(*prev_time) >= Duration::from_secs(12) {
                        *prev_bps = 0.0;
                    }
                }
                None => {
                    self.recording_speed_samples
                        .insert(target, (ch.size_bytes, now, 0.0));
                }
            }
        }
        self.recording_speed_samples
            .retain(|target, _| active_targets.contains(target));
    }

    pub(super) fn observe_live_alerts(&mut self, status: &NativeWatcherStatus) {
        if self.last_watcher_running != Some(status.running) {
            self.last_watcher_running = Some(status.running);
            self.pending_signals
                .push(DesktopSignal::WatcherStateChanged(status.running));
        }

        let mut current_recordings = HashMap::new();
        let mut current_errors = HashSet::new();
        let mut new_recordings = Vec::new();
        let mut new_errors = Vec::new();

        for ch in &status.channels {
            let target = live_adapter::scoped_target(ch.platform, &ch.account);
            let display_name = if ch.name.trim().is_empty() {
                ch.account.clone()
            } else {
                ch.name.clone()
            };
            if ch.status == "RECORDING" {
                let session_key = ch
                    .bno
                    .clone()
                    .filter(|v| !v.is_empty())
                    .or_else(|| ch.started_at.clone())
                    .unwrap_or_default();
                let key = (target.clone(), session_key);
                if self.seen_initial_live && !self.active_recordings.contains_key(&key) {
                    new_recordings.push((ch.platform.as_str().to_string(), display_name.clone()));
                }
                current_recordings.insert(
                    key,
                    ActiveRecordingMeta {
                        platform: ch.platform.as_str().to_string(),
                        name: display_name,
                        started_at: ch.started_at.clone(),
                        size_bytes: ch.size_bytes,
                    },
                );
            } else if matches!(
                ch.status.as_str(),
                "ERROR" | "AUTH" | "LOW_DISK" | "STALLED"
            ) {
                if self.seen_initial_live && !self.notified_error_targets.contains(&target) {
                    new_errors.push((display_name, ch.status.clone()));
                }
                current_errors.insert(target);
            }
        }

        if self.seen_initial_live {
            if self.state.notify_record_start && !new_recordings.is_empty() {
                let body = if new_recordings.len() == 1 {
                    let (plat, name) = &new_recordings[0];
                    format!("[{plat}] {name} 방송 녹화를 시작했습니다.")
                } else {
                    let names = new_recordings
                        .iter()
                        .take(3)
                        .map(|(_, name)| name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{}개 채널 녹화 시작: {names}", new_recordings.len())
                };
                self.pending_signals.push(DesktopSignal::TrayAlert {
                    title: "Stream Archive LIVE 녹화 시작".into(),
                    body,
                    warning: false,
                });
            }

            if self.state.notify_record_finish {
                let mut finished_recordings: Vec<&ActiveRecordingMeta> = self
                    .active_recordings
                    .iter()
                    .filter(|(key, _)| {
                        !current_recordings.contains_key(*key) && !current_errors.contains(&key.0)
                    })
                    .map(|(_, meta)| meta)
                    .collect();
                finished_recordings.sort_by(|a, b| {
                    a.platform
                        .cmp(&b.platform)
                        .then_with(|| a.name.cmp(&b.name))
                });
                if !finished_recordings.is_empty() {
                    let body = if finished_recordings.len() == 1 {
                        let meta = finished_recordings[0];
                        let elapsed = meta
                            .started_at
                            .as_deref()
                            .map(format_elapsed_since)
                            .unwrap_or_default();
                        let size = if meta.size_bytes > 0 {
                            format_bytes(meta.size_bytes)
                        } else {
                            String::new()
                        };
                        match (!elapsed.is_empty(), !size.is_empty()) {
                            (true, true) => format!(
                                "[{}] {} 녹화 완료 ({elapsed} · {size})",
                                meta.platform, meta.name
                            ),
                            (false, true) => {
                                format!("[{}] {} 녹화 완료 ({size})", meta.platform, meta.name)
                            }
                            (true, false) => {
                                format!("[{}] {} 녹화 완료 ({elapsed})", meta.platform, meta.name)
                            }
                            (false, false) => {
                                format!(
                                    "[{}] {} 방송 녹화가 완료되었습니다.",
                                    meta.platform, meta.name
                                )
                            }
                        }
                    } else {
                        let names = finished_recordings
                            .iter()
                            .take(3)
                            .map(|meta| meta.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{}개 채널 녹화 완료: {names}", finished_recordings.len())
                    };
                    self.pending_signals.push(DesktopSignal::TrayAlert {
                        title: "Stream Archive LIVE 녹화 완료".into(),
                        body,
                        warning: false,
                    });
                }
            }

            if self.state.notify_warning && !new_errors.is_empty() {
                let body = if new_errors.len() == 1 {
                    let (name, st) = &new_errors[0];
                    format!("{name} 채널 상태 경고 ({st}). LIVE 탭에서 확인하세요.")
                } else {
                    let names = new_errors
                        .iter()
                        .take(3)
                        .map(|(name, _)| name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{}개 채널 경고 발생: {names}", new_errors.len())
                };
                self.pending_signals.push(DesktopSignal::TrayAlert {
                    title: "Stream Archive 채널 경고".into(),
                    body,
                    warning: true,
                });
            }
        } else {
            self.seen_initial_live = true;
        }

        self.active_recordings = current_recordings;
        self.notified_error_targets = current_errors;
    }

    pub(super) fn observe_storage_alerts(&mut self, snapshot: &StorageSnapshot) {
        let mut current_critical = HashSet::new();
        let mut newly_critical = Vec::new();
        for row in storage_adapter::rows(snapshot) {
            if row.status == "공간 부족" {
                if !self.notified_critical_volumes.contains(&row.volume) {
                    newly_critical.push(format!("{} ({})", row.volume, row.used));
                }
                current_critical.insert(row.volume);
            }
        }
        if self.state.notify_warning && !newly_critical.is_empty() {
            self.pending_signals.push(DesktopSignal::TrayAlert {
                title: "Stream Archive 저장 공간 부족".into(),
                body: format!(
                    "여유 공간이 임계값 미만입니다: {}",
                    newly_critical.join(", ")
                ),
                warning: true,
            });
        }
        self.notified_critical_volumes = current_critical;
    }

    pub(super) fn render_live(&mut self, status: NativeWatcherStatus) {
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
        let search = self.live_model.search().to_string();
        let sort_mode = self.live_model.sort_mode().to_string();
        let mut rows: Vec<LiveChannelRow> = view
            .channels
            .into_iter()
            .filter(|ch| {
                LiveModel::matches_filter(&filter, &ch.status)
                    && LiveModel::matches_search(
                        &search,
                        &ch.name,
                        &ch.account,
                        &ch.title,
                        &ch.platform,
                    )
            })
            .map(|ch| {
                let thumbnail_image = self.thumbnails.image(&ch.target);
                let speed = if ch.status == "RECORDING" {
                    self.recording_speed_samples
                        .get(&ch.target)
                        .map(|(_, _, bps)| format_transfer_rate(*bps))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
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
                    elapsed: ch.elapsed,
                    speed,
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
        if sort_mode == "STATUS" {
            rows.sort_by_key(|row| LiveModel::status_sort_rank(&row.status));
        }
        self.live_model.reconcile(rows);
    }

    pub(super) fn render_storage(&mut self, snapshot: StorageSnapshot) {
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
}
