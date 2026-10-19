//! Latest-only presentation state. Runtime tracking never depends on repainting.
use crate::LiveChannelRow;
use slint::{Model, VecModel};
use std::rc::Rc;
use stream_archive_server::model::NativeWatcherStatus;

#[derive(Default)]
pub struct LiveModel {
    pub rows: Rc<VecModel<LiveChannelRow>>,
    pub latest: Option<NativeWatcherStatus>,
    pub resets: u64,
    pub row_updates: u64,
    visible: bool,
    dirty: bool,
    awaiting_snapshot: bool,
    generation: u64,
}
impl LiveModel {
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
        } else if self.rows.row_count() > 0 {
            self.rows.set_vec(Vec::new());
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
        if self.rows.row_count() != rows.len()
            || self
                .rows
                .iter()
                .zip(&rows)
                .any(|(old, new)| old.target != new.target)
        {
            self.rows.set_vec(rows);
            self.resets = self.resets.saturating_add(1);
            return;
        }
        for (index, row) in rows.into_iter().enumerate() {
            if self
                .rows
                .row_data(index)
                .is_none_or(|old| !same_row(&old, &row))
            {
                self.rows.set_row_data(index, row);
                self.row_updates = self.row_updates.saturating_add(1);
            }
        }
    }
}
// Slint 1.18 Image::default() is not equal even to another empty Image.
// LIVE images are embedded RGBA or empty; compare the empty case explicitly.
fn same_row(a: &LiveChannelRow, b: &LiveChannelRow) -> bool {
    (a.thumbnail_image == b.thumbnail_image
        || (a.thumbnail_image.size().width == 0 && b.thumbnail_image.size().width == 0))
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
        && a.size == b.size
        && a.started_at == b.started_at
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
        let identity = model.rows.clone();
        model.reconcile(vec![row("a", "1"), row("b", "1")]);
        for index in 2..1000 {
            model.reconcile(vec![row("a", &index.to_string()), row("b", "1")]);
        }
        assert!(Rc::ptr_eq(&identity, &model.rows));
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
        preview.thumbnail_image = slint::Image::from_rgba8(slint::SharedPixelBuffer::new(1, 1));
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
        assert_eq!(model.rows.row_count(), 0);
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
            assert!(model.latest.is_some()); // Background tracking still progresses.
            assert!(model.needs_snapshot());
            assert!(model.take_for_display().is_none());
        }
        model.receive(NativeWatcherStatus::default(), Some(model.generation()));
        assert!(!model.needs_snapshot());
        assert!(model.take_for_display().is_some());
    }
}
