//! UI-thread LIVE snapshot cache. No disk cache, network or provider logic here.
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::collections::BTreeMap;
use stream_archive_server::{
    model::NativeWatcherStatus, support::platform::PlatformId, thumbnail_service::ThumbnailImage,
};

pub const MAX_THUMBNAILS: usize = 128;
const MAX_PENDING: usize = 2;
struct Entry {
    platform: PlatformId,
    account: String,
    broadcast: String,
    completion_revision: u64,
    generation: u64,
    available: bool,
    attempted: bool,
    blocked: bool,
    image: Image,
}
#[derive(Default)]
pub struct Thumbnails {
    entries: BTreeMap<String, Entry>,
    pending: BTreeMap<u64, String>,
    sequence: u64,
}
impl Thumbnails {
    fn next_generation(&mut self) -> u64 {
        self.sequence += 1;
        self.sequence
    }
    pub fn image(&self, target: &str) -> Image {
        self.entries
            .get(target)
            .map(|entry| entry.image.clone())
            .unwrap_or_default()
    }
    pub fn sync(&mut self, status: &NativeWatcherStatus, manual_refresh: bool) {
        let active: Vec<_> = status
            .channels
            .iter()
            .filter(|row| {
                status.running
                    && row.bno.is_some()
                    && !matches!(
                        row.status.as_str(),
                        "OFFLINE" | "DISABLED" | "WATCHER_STOPPED"
                    )
            })
            .take(MAX_THUMBNAILS)
            .collect();
        self.entries.retain(|target, _| {
            active.iter().any(|row| {
                crate::live_adapter::scoped_target(row.platform, &row.account) == *target
            })
        });
        for row in active {
            let target = crate::live_adapter::scoped_target(row.platform, &row.account);
            let broadcast = row.bno.as_deref().unwrap();
            let changed = self.entries.get(&target).is_none_or(|entry| {
                entry.broadcast != broadcast || entry.completion_revision != row.completion_revision
            });
            if changed {
                let generation = self.next_generation();
                self.entries.insert(
                    target.clone(),
                    Entry {
                        platform: row.platform,
                        account: row.account.clone(),
                        broadcast: broadcast.into(),
                        completion_revision: row.completion_revision,
                        generation,
                        available: row.thumbnail_url.is_some(),
                        attempted: false,
                        blocked: row.completed_broadcast_id.as_deref() == Some(broadcast),
                        image: Image::default(),
                    },
                );
            }
            let generation = manual_refresh.then(|| self.next_generation());
            let entry = self.entries.get_mut(&target).unwrap();
            entry.available = row.thumbnail_url.is_some();
            if let Some(generation) = generation {
                entry.generation = generation;
                entry.blocked = false;
                entry.attempted = false;
                // Keep a good image until the new request succeeds.
            }
        }
    }
    pub fn requests(&mut self) -> Vec<(u64, PlatformId, String, String, u64)> {
        let mut requests = Vec::new();
        for (target, entry) in &mut self.entries {
            if self.pending.len() >= MAX_PENDING {
                break;
            }
            if entry.attempted || entry.blocked || !entry.available {
                continue;
            }
            entry.attempted = true;
            self.pending.insert(entry.generation, target.clone());
            requests.push((
                entry.generation,
                entry.platform,
                entry.account.clone(),
                entry.broadcast.clone(),
                entry.completion_revision,
            ));
        }
        requests
    }
    pub fn complete(&mut self, token: u64, image: Option<ThumbnailImage>) {
        let Some(target) = self.pending.remove(&token) else {
            return;
        };
        let Some(entry) = self
            .entries
            .get_mut(&target)
            .filter(|entry| entry.generation == token)
        else {
            return;
        };
        if let Some(image) = image {
            let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                &image.rgba,
                image.width,
                image.height,
            );
            entry.image = Image::from_rgba8(pixels);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_archive_server::model::ChannelRuntimeStatus;
    fn status(id: &str) -> NativeWatcherStatus {
        NativeWatcherStatus {
            running: true,
            channels: vec![ChannelRuntimeStatus {
                platform: PlatformId::Soop,
                account: "test".into(),
                status: "RECORDING".into(),
                bno: Some(id.into()),
                thumbnail_url: Some(format!("https://liveimg.sooplive.com/m/{id}")),
                ..Default::default()
            }],
            ..Default::default()
        }
    }
    fn image() -> ThumbnailImage {
        ThumbnailImage {
            width: 2,
            height: 1,
            rgba: vec![255; 8],
        }
    }
    fn token(cache: &mut Thumbnails) -> u64 {
        cache.requests().remove(0).0
    }
    #[test]
    fn polls_keep_first_image_and_manual_refresh_retains_good_image_on_failure() {
        let mut cache = Thumbnails::default();
        let state = status("1");
        cache.sync(&state, false);
        let t = token(&mut cache);
        cache.complete(t, Some(image()));
        cache.sync(&state, false);
        assert!(cache.requests().is_empty());
        assert_eq!(cache.image("SOOP:test").size().width, 2);
        cache.sync(&state, true);
        let t = token(&mut cache);
        cache.complete(t, None);
        assert_eq!(cache.image("SOOP:test").size().width, 2);
        assert!(cache.requests().is_empty());
    }
    #[test]
    fn completion_purges_image_and_late_reply_and_blocks_same_broadcast_until_manual_refresh() {
        let mut cache = Thumbnails::default();
        let mut state = status("1");
        cache.sync(&state, false);
        let old = token(&mut cache);
        state.channels[0].completion_revision = 1;
        state.channels[0].completed_broadcast_id = Some("1".into());
        cache.sync(&state, false);
        cache.complete(old, Some(image()));
        assert_eq!(cache.image("SOOP:test").size().width, 0);
        assert!(cache.requests().is_empty());
        cache.sync(&state, true);
        let t = token(&mut cache);
        cache.complete(t, Some(image()));
        // A second completion of the same broadcast must purge again.
        state.channels[0].completion_revision = 2;
        cache.sync(&state, false);
        assert_eq!(cache.image("SOOP:test").size().width, 0);
        assert!(cache.requests().is_empty());
        state.channels[0].bno = Some("2".into());
        cache.sync(&state, false);
        assert_eq!(cache.requests().len(), 1);
    }
    #[test]
    fn refresh_and_broadcast_change_reject_previous_generations() {
        let mut cache = Thumbnails::default();
        let mut state = status("1");
        cache.sync(&state, false);
        let old = token(&mut cache);
        cache.sync(&state, true);
        let current = token(&mut cache);
        cache.complete(old, Some(image()));
        assert_eq!(cache.image("SOOP:test").size().width, 0);
        state.channels[0].bno = Some("2".into());
        cache.sync(&state, false);
        cache.complete(current, Some(image()));
        assert_eq!(cache.image("SOOP:test").size().width, 0);
        let current = token(&mut cache);
        cache.complete(current, Some(image()));
        assert_eq!(cache.image("SOOP:test").size().width, 2);
    }
    #[test]
    fn offline_removed_and_stopped_rows_release_cache_and_reject_replies() {
        for stop in 0..3 {
            let mut cache = Thumbnails::default();
            let mut state = status("1");
            cache.sync(&state, false);
            let t = token(&mut cache);
            match stop {
                0 => state.running = false,
                1 => state.channels.clear(),
                _ => state.channels[0].status = "OFFLINE".into(),
            }
            cache.sync(&state, false);
            cache.complete(t, Some(image()));
            assert!(cache.entries.is_empty());
            assert!(cache.pending.is_empty());
        }
    }
    #[test]
    fn null_metadata_falls_back_without_requests_then_fetches_once_if_available() {
        let mut cache = Thumbnails::default();
        let mut state = status("1");
        state.channels[0].thumbnail_url = None;
        cache.sync(&state, false);
        assert!(cache.requests().is_empty());
        state.channels[0].thumbnail_url = Some("url".into());
        cache.sync(&state, false);
        let t = token(&mut cache);
        cache.complete(t, None);
        assert!(cache.requests().is_empty());
    }
    #[test]
    fn cache_and_inflight_requests_are_bounded_even_during_refresh() {
        let mut cache = Thumbnails::default();
        let mut state = status("1");
        state.channels = (0..150)
            .map(|i| {
                let mut row = state.channels[0].clone();
                row.account = format!("test{i}");
                row
            })
            .collect();
        cache.sync(&state, false);
        assert_eq!(cache.entries.len(), MAX_THUMBNAILS);
        let requests = cache.requests();
        assert_eq!(requests.len(), MAX_PENDING);
        cache.sync(&state, true);
        assert!(cache.requests().is_empty());
        for (token, _, _, _, _) in requests {
            cache.complete(token, Some(image()));
        }
        assert_eq!(cache.requests().len(), MAX_PENDING);
    }
}
