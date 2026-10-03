//! UI-thread image cache: bounded requests, negative caching and stale reply rejection.
use crate::LiveChannelRow;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
use stream_archive_server::{profile_service::ProfileImage, support::platform::PlatformId};

pub const MAX_PROFILES: usize = 128;
const MAX_PENDING: usize = 2;
struct Entry {
    image: Image,
    expires: Instant,
}
#[derive(Default)]
pub struct Profiles {
    entries: BTreeMap<String, Entry>,
    active: BTreeSet<String>,
    pending: BTreeSet<String>,
}
impl Profiles {
    pub fn image(&self, target: &str) -> Image {
        self.entries
            .get(target)
            .map(|entry| entry.image.clone())
            .unwrap_or_default()
    }

    pub fn requests(&mut self, rows: &[LiveChannelRow]) -> Vec<(String, PlatformId, String)> {
        self.active = rows
            .iter()
            .take(MAX_PROFILES)
            .map(|row| row.target.to_string())
            .collect();
        self.entries.retain(|key, _| self.active.contains(key));
        let now = Instant::now();
        let mut requests = Vec::new();
        for row in rows.iter().take(MAX_PROFILES) {
            if self.pending.len() >= MAX_PENDING {
                break;
            }
            let target = row.target.to_string();
            if self.pending.contains(&target)
                || self
                    .entries
                    .get(&target)
                    .is_some_and(|entry| entry.expires > now)
            {
                continue;
            }
            let Ok(platform) = row.platform.parse() else {
                continue;
            };
            self.pending.insert(target.clone());
            requests.push((target, platform, row.account.to_string()));
        }
        requests
    }

    pub fn complete(&mut self, target: String, profile: Option<ProfileImage>) {
        if !self.pending.remove(&target) || !self.active.contains(&target) {
            return;
        }
        let (image, ttl) = match profile {
            Some(profile) => {
                let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    &profile.rgba,
                    profile.width,
                    profile.height,
                );
                (Image::from_rgba8(pixels), Duration::from_secs(3600))
            }
            None => (Image::default(), Duration::from_secs(300)),
        };
        self.entries.insert(
            target,
            Entry {
                image,
                expires: Instant::now() + ttl,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(index: usize) -> LiveChannelRow {
        LiveChannelRow {
            target: format!("SOOP:test{index}").into(),
            platform: "SOOP".into(),
            account: format!("test{index}").into(),
            ..Default::default()
        }
    }
    #[test]
    fn bounds_requests_and_negative_cache_and_rejects_removed_channel_reply() {
        let rows: Vec<_> = (0..150).map(row).collect();
        let mut profiles = Profiles::default();
        let requests = profiles.requests(&rows);
        assert_eq!(requests.len(), MAX_PENDING);
        assert!(profiles.requests(&rows).is_empty());
        for (target, _, _) in requests {
            profiles.complete(target, None);
        }
        assert_eq!(profiles.entries.len(), 2);
        let requests = profiles.requests(&rows);
        assert_eq!(requests[0].0, "SOOP:test2");
        profiles.requests(&[]);
        for (target, _, _) in requests {
            profiles.complete(target, None);
        }
        assert!(profiles.entries.is_empty());
        for _ in 0..MAX_PROFILES {
            for (target, _, _) in profiles.requests(&rows) {
                profiles.complete(target, None);
            }
        }
        assert_eq!(profiles.entries.len(), MAX_PROFILES);
        assert!(profiles.requests(&rows).is_empty());
        profiles.entries.get_mut("SOOP:test0").unwrap().expires = Instant::now();
        assert_eq!(profiles.requests(&rows).len(), 1);
    }
}
