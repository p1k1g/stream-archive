//! Desktop close/exit/tray coordination and periodic UI polling tick.

use super::{AppController, DesktopSignal, live_poll_needed};
use crate::{
    core_worker::Request,
    state::{MaintenanceSection, Page},
};
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use stream_archive_server::resource_trace;

impl AppController {
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
        if self.sender.send(Request::PrepareExit).is_err() {
            self.pending_signals.push(DesktopSignal::ExitNow);
        }
    }

    pub fn complete_exit_confirmation(&mut self, confirmed: bool) {
        self.state.exit_confirm_visible = false;
        self.state.exit_confirm_error.clear();
        if confirmed {
            self.state.exit_shutting_down = true;
            self.stopping.store(true, Ordering::Release);
            if self.sender.send(Request::Shutdown).is_err() {
                self.pending_signals.push(DesktopSignal::ExitNow);
            }
        } else {
            self.state.exit_shutting_down = false;
            self.state.desktop_exit_pending = false;
        }
    }

    pub(super) fn apply_close_action(&mut self, action: &str) {
        if action == "TRAY" {
            self.pending_signals.push(DesktopSignal::HideToTray);
        } else {
            self.begin_exit_check();
        }
    }

    // ---- Periodic Polling & Response Draining ----

    pub fn poll_tick(&mut self) -> bool {
        let mut changed = false;

        while let Ok(response) = self.responses.try_recv() {
            changed = true;
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

            // While hidden in tray (or on another tab), still watch LIVE/storage transitions
            // at a low cadence when LIVE/warning tray alerts are enabled.
            let tray_alert_enabled = self.state.notify_record_start
                || self.state.notify_record_finish
                || self.state.notify_warning;
            if tray_alert_enabled
                && self.state.live_running
                && now.duration_since(self.last_tray_alert_poll) >= Duration::from_secs(5)
            {
                self.last_tray_alert_poll = now;
                if !self.state.live_busy && !self.live_poll_in_flight {
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

            if self.state.notify_warning
                && !self.window_visible
                && now.duration_since(self.last_tray_storage_poll) >= Duration::from_secs(60)
            {
                self.last_tray_storage_poll = now;
                if !self.state.storage_busy && !self.storage_poll_in_flight {
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

        changed
    }
}
