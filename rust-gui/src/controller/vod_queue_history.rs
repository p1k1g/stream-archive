//! VOD analysis/download, Queue management, and History/Calendar controller actions.

use super::AppController;
use crate::{
    core_worker::Request,
    history_adapter, queue_adapter,
    state::{HistoryCalendarDay, HistoryDisplayRow, QueueDisplayRow, VodPartRow, VodQualityRow},
    vod_adapter::{self, AnalysisSync},
};
use stream_archive_server::{
    history_service::HistoryFilter,
    model::{HistoryResponse, VodJobStatus, VodQueueSnapshot},
};

impl AppController {
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

    // ---- Rendering helpers ----

    pub(super) fn sync_vod_draft(&mut self) {
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

    pub(super) fn render_vod_status(&mut self, status: VodJobStatus) -> AnalysisSync {
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
    pub fn apply_queue_snapshot_for_test(&mut self, snapshot: VodQueueSnapshot) {
        self.render_queue(snapshot);
    }

    pub(super) fn render_queue(&mut self, snapshot: VodQueueSnapshot) {
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

    pub(super) fn render_history(&mut self, history: HistoryResponse, view: &str) {
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

    pub(super) fn apply_calendar(&mut self, cal: history_adapter::CalendarMonthView) {
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
}
