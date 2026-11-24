//! Application of background `core_worker::Response` events to `AppController` state.

use super::{AppController, DesktopSignal, bind_core_snapshot};
use crate::{
    core_worker::{Request, Response},
    state::Page,
    thumbnail_adapter::rgba_to_render_image,
    vod_adapter::AnalysisSync,
};

impl AppController {
    #[cfg(test)]
    pub fn apply_response_for_test(&mut self, response: Response) {
        self.apply_response(response);
    }

    pub(super) fn apply_response(&mut self, response: Response) {
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
                window_bounds,
                first_run,
                startup_failed,
                message,
            } => {
                self.state.settings_busy = false;
                self.state.startup_failed = startup_failed;
                self.state.first_run = first_run;
                self.state.runtime_ready = !startup_failed;
                self.state.settings_message = message;
                if let Some(bounds) = window_bounds {
                    let apply_late =
                        !self.window_bounds_initialized && self.window_opened_before_bounds;
                    self.state.window_bounds_spec = Some(bounds.clone());
                    self.last_saved_window_bounds = Some(bounds.clone());
                    if apply_late {
                        self.pending_signals
                            .push(DesktopSignal::ApplyWindowBounds(bounds));
                    }
                }
                self.window_bounds_initialized = true;
                self.window_opened_before_bounds = false;
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
                    if let Some(notify_start) =
                        fields.iter().find(|f| f.key == "GUI_NOTIFY_RECORD_START")
                    {
                        self.state.notify_record_start =
                            notify_start.value.trim().eq_ignore_ascii_case("Y");
                    }
                    if let Some(notify_warn) = fields.iter().find(|f| f.key == "GUI_NOTIFY_WARNING")
                    {
                        self.state.notify_warning =
                            !notify_warn.value.trim().eq_ignore_ascii_case("N");
                    }
                    if let Some(auto_watch) =
                        fields.iter().find(|f| f.key == "GUI_AUTO_START_WATCHER")
                    {
                        let enabled = auto_watch.value.trim().eq_ignore_ascii_case("Y");
                        self.state.auto_start_watcher = enabled;
                        self.initial_auto_start_watcher.get_or_insert(enabled);
                    } else {
                        self.initial_auto_start_watcher.get_or_insert(false);
                    }
                    if let Some(start_tray) =
                        fields.iter().find(|f| f.key == "GUI_START_MINIMIZED_TRAY")
                    {
                        self.state.start_minimized_tray =
                            start_tray.value.trim().eq_ignore_ascii_case("Y");
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
                self.maybe_trigger_auto_start_watcher();
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
                self.update_recording_speeds(&status);
                self.observe_live_alerts(&status);
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
                self.maybe_trigger_auto_start_watcher();
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
                self.observe_storage_alerts(&snapshot);
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
            Response::MediaFileOpened(result) => {
                if let Err(err) = result {
                    let message = format!("영상 재생 실패: {err}");
                    match self.state.active_page {
                        Page::History => self.state.history_message = message,
                        Page::Vod => self.state.vod_message = message,
                        _ => self.state.live_message = message,
                    }
                }
            }
            Response::LiveStreamOpened(result) => {
                if let Err(err) = result {
                    self.state.live_message = format!("방송 페이지 열기 실패: {err}");
                }
            }
            Response::LogsFolderOpened(result) => {
                if let Err(err) = result {
                    self.state.maintenance_message = format!("로그 폴더 열기 실패: {err}");
                }
            }
            Response::ReleaseUrlOpened(result) => {
                if let Err(err) = result {
                    self.state.maintenance_message = format!("릴리스 페이지 열기 실패: {err}");
                }
            }
            Response::UpdateCheck { manual, result } => {
                self.state.update_checking = false;
                match result {
                    Ok(info) => {
                        self.state.update_available = info.update_available;
                        self.state.update_latest_version = info.latest_version.clone();
                        self.state.update_release_url = info.release_url;
                        if info.update_available {
                            if manual {
                                self.state.update_dismissed = false;
                            }
                            self.state.update_status_message = format!(
                                "새 버전 v{}이(가) 출시되었습니다 (현재 v{}).",
                                info.latest_version, info.current_version
                            );
                        } else if manual {
                            self.state.update_status_message = format!(
                                "현재 최신 버전(v{})을 사용 중입니다.",
                                info.current_version
                            );
                        }
                    }
                    Err(err) => {
                        if manual {
                            self.state.update_status_message = format!("업데이트 확인 실패: {err}");
                        }
                    }
                }
            }
            Response::WindowsStartup(result) => {
                self.state.windows_startup_busy = false;
                match result {
                    Ok(enabled) => {
                        self.state.windows_startup_enabled = enabled;
                    }
                    Err(err) => {
                        self.state.settings_message =
                            format!("Windows 시작프로그램 설정 실패: {err}");
                    }
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
            Response::ExitCheck(result) => match result {
                Ok(false) => self.complete_exit_confirmation(true),
                Ok(true) => {
                    self.state.exit_confirm_visible = true;
                    self.state.exit_confirm_error.clear();
                    self.pending_signals
                        .push(DesktopSignal::EnsureWindowVisible);
                }
                Err(err) => {
                    self.state.exit_confirm_visible = true;
                    self.state.exit_confirm_error = format!("활성 작업 확인 중 오류: {err}");
                    self.pending_signals
                        .push(DesktopSignal::EnsureWindowVisible);
                }
            },
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
}
