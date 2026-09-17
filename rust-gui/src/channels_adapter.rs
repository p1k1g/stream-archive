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

    pub fn toggle_platform(&mut self, index: usize) {
        if let Some(channel) = self.rows.get_mut(index) {
            channel.platform = match channel.platform {
                PlatformId::Soop => PlatformId::Chzzk,
                PlatformId::Chzzk => PlatformId::Soop,
            };
        }
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
        draft.toggle_platform(0);
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
    fn stale_indices_are_ignored() {
        let mut draft = ChannelsDraft::default();
        draft.load(vec![channel()]);
        draft.edit(99, "name", "ignored".into());
        draft.set_enabled(99, false);
        draft.toggle_platform(99);
        draft.remove(99);
        assert!(!draft.dirty());
    }
}
