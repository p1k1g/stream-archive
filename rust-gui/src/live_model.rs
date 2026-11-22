//! Latest-only presentation state. Runtime tracking never depends on repainting.
use crate::state::LiveChannelRow;
use stream_archive_server::model::NativeWatcherStatus;

pub struct LiveModel {
    pub rows: Vec<LiveChannelRow>,
    pub latest: Option<NativeWatcherStatus>,
    pub resets: u64,
    pub row_updates: u64,
    filter: String,
    search: String,
    sort_mode: String,
    visible: bool,
    dirty: bool,
    awaiting_snapshot: bool,
    generation: u64,
}

impl Default for LiveModel {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            latest: None,
            resets: 0,
            row_updates: 0,
            filter: "ALL".into(),
            search: String::new(),
            sort_mode: "STATUS".into(),
            visible: false,
            dirty: false,
            awaiting_snapshot: false,
            generation: 0,
        }
    }
}

impl LiveModel {
    pub fn set_filter(&mut self, filter: &str) -> bool {
        if self.filter == filter {
            return false;
        }
        self.filter = filter.to_string();
        self.dirty = true;
        true
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn set_search(&mut self, search: String) -> bool {
        if self.search == search {
            return false;
        }
        self.search = search;
        self.dirty = true;
        true
    }

    pub fn search(&self) -> &str {
        &self.search
    }

    pub fn set_sort_mode(&mut self, mode: &str) -> bool {
        let normalized = if mode == "ORDER" { "ORDER" } else { "STATUS" };
        if self.sort_mode == normalized {
            return false;
        }
        self.sort_mode = normalized.to_string();
        self.dirty = true;
        true
    }

    pub fn sort_mode(&self) -> &str {
        &self.sort_mode
    }

    pub fn matches_filter(filter: &str, status: &str) -> bool {
        match filter {
            "RECORDING" => status == "RECORDING",
            "OFFLINE" => status == "OFFLINE",
            "ERROR" => matches!(status, "ERROR" | "LOW_DISK" | "STALLED" | "AUTH"),
            _ => true,
        }
    }

    pub fn matches_search(
        query: &str,
        name: &str,
        account: &str,
        title: &str,
        platform: &str,
    ) -> bool {
        let needle = query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return true;
        }
        [name, account, title, platform]
            .into_iter()
            .any(|field| field.to_ascii_lowercase().contains(&needle))
    }

    pub fn status_sort_rank(status: &str) -> u8 {
        match status {
            "RECORDING" => 0,
            "PASSWORD_REQUIRED" => 1,
            "LOW_DISK" | "STALLED" | "AUTH" | "ERROR" => 2,
            "PAUSED" => 3,
            "OFFLINE" => 4,
            "UNKNOWN" => 5,
            "WATCHER_STOPPED" => 6,
            "DISABLED" => 7,
            _ => 8,
        }
    }

    pub fn set_visible(&mut self, visible: bool) -> bool {
        if self.visible == visible {
            return false;
        }
        self.visible = visible;
        self.dirty = true;
        if visible {
            // A hidden cache is not necessarily the latest runtime state.
            self.generation = self.generation.saturating_add(1);
            self.awaiting_snapshot = true;
        } else if !self.rows.is_empty() {
            self.rows.clear();
            self.resets = self.resets.saturating_add(1);
        }
        visible
    }

    pub fn needs_snapshot(&self) -> bool {
        self.visible && self.awaiting_snapshot
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn receive(&mut self, status: NativeWatcherStatus, generation: Option<u64>) {
        self.latest = Some(status);
        self.dirty = true;
        if generation == Some(self.generation) {
            self.awaiting_snapshot = false;
        }
    }

    pub fn invalidate_images(&mut self) {
        self.dirty = true;
    }

    pub fn take_for_display(&mut self) -> Option<NativeWatcherStatus> {
        if !self.visible || !self.dirty || self.awaiting_snapshot {
            return None;
        }
        let status = self.latest.clone()?;
        self.dirty = false;
        Some(status)
    }

    pub fn reconcile(&mut self, rows: Vec<LiveChannelRow>) {
        if self.rows.len() != rows.len()
            || self
                .rows
                .iter()
                .zip(&rows)
                .any(|(old, new)| old.target != new.target)
        {
            self.rows = rows;
            self.resets = self.resets.saturating_add(1);
            return;
        }
        for (index, row) in rows.into_iter().enumerate() {
            if !same_row(&self.rows[index], &row) {
                self.rows[index] = row;
                self.row_updates = self.row_updates.saturating_add(1);
            }
        }
    }
}

// Reopen refresh intent survives an older in-flight request. Only responses
// initiated in this visibility generation may populate the storage model.
#[derive(Default)]
pub struct StorageRefresh {
    generation: u64,
    pending: bool,
}

impl StorageRefresh {
    pub fn reopen(&mut self, generation: u64) {
        self.generation = generation;
        self.pending = true;
    }

    pub fn needs_request(&self, busy: bool, in_flight: bool) -> bool {
        self.pending && !busy && !in_flight
    }

    pub fn requested(&mut self) {
        self.pending = false;
    }

    pub fn accepts(&self, generation: u64) -> bool {
        self.generation == generation
    }
}

fn same_row(a: &LiveChannelRow, b: &LiveChannelRow) -> bool {
    a.thumbnail_image == b.thumbnail_image
        && a.target == b.target
        && a.platform == b.platform
        && a.name == b.name
        && a.account == b.account
        && a.status == b.status
        && a.status_label == b.status_label
        && a.status_tone == b.status_tone
        && a.title == b.title
        && a.bno == b.bno
        && a.file == b.file
        && a.output_dir == b.output_dir
        && a.size == b.size
        && a.started_at == b.started_at
        && a.elapsed == b.elapsed
        && a.speed == b.speed
        && a.suppressed == b.suppressed
        && a.detail == b.detail
        && a.can_stop_once == b.can_stop_once
        && a.can_resume == b.can_resume
        && a.can_recheck == b.can_recheck
        && a.password_required == b.password_required
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thumbnail_adapter::rgba_to_render_image;
    use stream_archive_server::thumbnail_service::ThumbnailImage;

    fn row(key: &str, size: &str) -> LiveChannelRow {
        LiveChannelRow {
            target: key.into(),
            size: size.into(),
            ..Default::default()
        }
    }

    #[test]
    fn repeated_progress_keeps_model_identity_and_only_changes_affected_rows() {
        let mut model = LiveModel::default();
        model.reconcile(vec![row("a", "1"), row("b", "1")]);
        for index in 2..1000 {
            model.reconcile(vec![row("a", &index.to_string()), row("b", "1")]);
        }
        assert_eq!(model.resets, 1);
        assert_eq!(model.row_updates, 998);
        model.reconcile(vec![row("a", "999"), row("b", "1")]);
        assert_eq!(model.row_updates, 998);
        model.reconcile(vec![row("b", "1"), row("a", "999")]);
        assert_eq!(model.resets, 2);
    }

    #[test]
    fn empty_and_replaced_images_follow_live_preview_identity() {
        let mut model = LiveModel::default();
        model.reconcile(vec![row("a", "1")]);
        model.reconcile(vec![row("a", "1")]);
        assert_eq!(model.row_updates, 0);
        let mut preview = row("a", "1");
        preview.thumbnail_image = rgba_to_render_image(&ThumbnailImage {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        });
        model.reconcile(vec![preview.clone()]);
        assert_eq!(model.row_updates, 1);
        model.reconcile(vec![preview]);
        assert_eq!(model.row_updates, 1);
        model.reconcile(vec![row("a", "1")]);
        assert_eq!(model.row_updates, 2);
    }

    #[test]
    fn hidden_snapshots_keep_only_latest_and_reopen_waits_for_fresh_snapshot() {
        let mut model = LiveModel::default();
        model.set_visible(true);
        model.receive(NativeWatcherStatus::default(), Some(model.generation()));
        assert!(model.take_for_display().is_some());
        model.reconcile(vec![row("a", "1")]);
        model.set_visible(false);
        let resets = model.resets;
        for count in 0..1000 {
            model.receive(
                NativeWatcherStatus {
                    recording_count: count,
                    ..Default::default()
                },
                Some(model.generation()),
            );
            model.invalidate_images();
            assert!(model.take_for_display().is_none());
        }
        assert_eq!(model.rows.len(), 0);
        assert_eq!(model.resets, resets);
        assert_eq!(model.latest.as_ref().unwrap().recording_count, 999);
        model.set_visible(true);
        assert!(model.needs_snapshot());
        assert!(model.take_for_display().is_none());
        model.receive(
            NativeWatcherStatus {
                recording_count: 7,
                ..Default::default()
            },
            Some(model.generation()),
        );
        assert_eq!(model.take_for_display().unwrap().recording_count, 7);
        assert!(model.take_for_display().is_none());
    }

    #[test]
    fn pre_reopen_and_untagged_responses_cannot_satisfy_fresh_wait() {
        let mut model = LiveModel::default();
        model.set_visible(true);
        let old = model.generation();
        model.receive(NativeWatcherStatus::default(), Some(old));
        model.take_for_display().unwrap();
        model.set_visible(false);
        model.set_visible(true);
        assert_ne!(old, model.generation());
        for generation in [Some(old), None] {
            model.receive(NativeWatcherStatus::default(), generation);
            assert!(model.latest.is_some());
            assert!(model.needs_snapshot());
            assert!(model.take_for_display().is_none());
        }
        model.receive(NativeWatcherStatus::default(), Some(model.generation()));
        assert!(!model.needs_snapshot());
        assert!(model.take_for_display().is_some());
    }

    #[test]
    fn storage_reopen_waits_for_old_poll_then_requests_current_generation() {
        let mut refresh = StorageRefresh::default();
        refresh.reopen(1);
        assert!(!refresh.needs_request(false, true));
        assert!(!refresh.needs_request(true, false));
        assert!(!refresh.accepts(0));
        assert!(refresh.needs_request(false, false));
        refresh.requested();
        assert!(!refresh.needs_request(false, false));
        assert!(refresh.accepts(1));
        refresh.reopen(2);
        assert!(!refresh.accepts(1));
        assert!(!refresh.needs_request(false, true));
        assert!(refresh.needs_request(false, false));
        refresh.requested();
        assert!(refresh.accepts(2));
    }

    #[test]
    fn filter_matching_aligns_offline_and_error_counts() {
        assert!(LiveModel::matches_filter("ALL", "PAUSED"));
        assert!(LiveModel::matches_filter("RECORDING", "RECORDING"));
        assert!(!LiveModel::matches_filter("RECORDING", "OFFLINE"));
        assert!(LiveModel::matches_filter("OFFLINE", "OFFLINE"));
        assert!(!LiveModel::matches_filter("OFFLINE", "PAUSED"));
        assert!(!LiveModel::matches_filter("OFFLINE", "DISABLED"));
        for status in ["ERROR", "LOW_DISK", "STALLED", "AUTH"] {
            assert!(LiveModel::matches_filter("ERROR", status));
        }
        assert!(!LiveModel::matches_filter("ERROR", "OFFLINE"));
    }
}
