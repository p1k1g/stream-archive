//! Native channel draft editing. Persistence and validation stay in StreamArchiveCore.
use stream_archive_server::{model::Channel, support::platform::PlatformId};

#[derive(Default)]
pub struct ChannelsDraft {
    pub rows: Vec<Channel>,
    original: Vec<Channel>,
}

impl ChannelsDraft {
    pub fn load(&mut self, channels: Vec<Channel>) {
        self.original = channels.clone();
        self.rows = channels;
    }

    pub fn dirty(&self) -> bool {
        self.rows != self.original
    }

    pub fn add(&mut self) {
        self.rows.push(Channel {
            platform: PlatformId::Soop,
            enabled: true,
            name: String::new(),
            account: String::new(),
            outdir: String::new(),
        });
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.rows.len() {
            self.rows.remove(index);
        }
    }

    pub fn edit(&mut self, index: usize, field: &str, value: String) {
        let Some(channel) = self.rows.get_mut(index) else {
            return;
        };
        match field {
            "name" => channel.name = value,
            "account" => channel.account = value,
            "outdir" => channel.outdir = value,
            _ => {}
        }
    }

    pub fn set_enabled(&mut self, index: usize, enabled: bool) {
        if let Some(channel) = self.rows.get_mut(index) {
            channel.enabled = enabled;
        }
    }

    pub fn select_platform(&mut self, index: usize, value: &str) {
        let platform = match value {
            "SOOP" => PlatformId::Soop,
            "CHZZK" => PlatformId::Chzzk,
            _ => return,
        };
        if let Some(channel) = self.rows.get_mut(index) {
            channel.platform = platform;
        }
    }

    pub fn apply_selected_path(
        &mut self,
        index: usize,
        expected: &[Channel],
        path: String,
    ) -> bool {
        if self.rows != expected || index >= self.rows.len() {
            return false;
        }
        self.edit(index, "outdir", path);
        true
    }

    pub fn snapshot(&self) -> Vec<Channel> {
        self.rows.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel() -> Channel {
        Channel {
            platform: PlatformId::Soop,
            enabled: true,
            name: "Example".into(),
            account: "example".into(),
            outdir: String::new(),
        }
    }

    #[test]
    fn add_edit_toggle_remove_tracks_dirty_state() {
        let mut draft = ChannelsDraft::default();
        draft.load(vec![channel()]);
        assert!(!draft.dirty());

        draft.edit(0, "name", "Renamed".into());
        assert!(draft.dirty());
        draft.select_platform(0, "CHZZK");
        assert_eq!(draft.rows[0].platform, PlatformId::Chzzk);
        draft.set_enabled(0, false);
        assert!(!draft.rows[0].enabled);

        draft.add();
        assert_eq!(draft.rows.len(), 2);
        draft.remove(1);
        assert_eq!(draft.rows.len(), 1);

        draft.load(vec![channel()]);
        assert!(!draft.dirty());
    }

    #[test]
    fn selecting_current_or_unknown_platform_does_not_modify_channel() {
        let mut draft = ChannelsDraft::default();
        draft.load(vec![channel()]);
        draft.select_platform(0, "SOOP");
        draft.select_platform(0, "unknown");
        assert!(!draft.dirty());
        draft.select_platform(0, "CHZZK");
        draft.select_platform(0, "CHZZK");
        assert_eq!(draft.rows[0].platform, PlatformId::Chzzk);
        assert_eq!(draft.rows[0].account, "example");
    }

    #[test]
    fn stale_indices_are_ignored() {
        let mut draft = ChannelsDraft::default();
        draft.load(vec![channel()]);
        draft.edit(99, "name", "ignored".into());
        draft.set_enabled(99, false);
        draft.select_platform(99, "CHZZK");
        draft.remove(99);
        assert!(!draft.dirty());
    }
}

#[cfg(test)]
mod picker_tests {
    use super::*;
    #[test]
    fn picker_updates_only_matching_draft_and_does_not_persist() {
        let mut draft = ChannelsDraft::default();
        draft.add();
        let before = draft.snapshot();
        assert!(draft.apply_selected_path(0, &before, "G:\\한글 폴더".into()));
        assert!(draft.dirty());
        assert_eq!(draft.rows[0].outdir, "G:\\한글 폴더");
        assert!(!draft.apply_selected_path(0, &before, "stale".into()));
        let before = draft.snapshot();
        draft.remove(0);
        draft.add();
        assert!(!draft.apply_selected_path(0, &before, "wrong row".into()));
        assert!(draft.rows[0].outdir.is_empty());
    }
}
