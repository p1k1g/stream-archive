//! Session-only, bounded VOD result events. No persisted replay, paths, URLs or secrets.
use crate::{model::VodJobStatus, support::platform::PlatformId};
use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadEvent {
    pub job_id: String,
    pub platform: PlatformId,
    pub completed: bool,
}

#[derive(Clone)]
struct PublishedDownload {
    event: DownloadEvent,
    epoch: u64,
}

/// Current-generation results only, including events queued before a transition.
pub struct DownloadSubscription {
    receiver: broadcast::Receiver<PublishedDownload>,
    epoch: std::sync::Arc<std::sync::Mutex<u64>>,
}

impl DownloadSubscription {
    fn is_current(&self, epoch: u64) -> bool {
        *self
            .epoch
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            == epoch
    }

    pub fn try_recv(&mut self) -> Result<DownloadEvent, broadcast::error::TryRecvError> {
        loop {
            let published = self.receiver.try_recv()?;
            if self.is_current(published.epoch) {
                return Ok(published.event);
            }
        }
    }

    pub async fn recv(&mut self) -> Result<DownloadEvent, broadcast::error::RecvError> {
        loop {
            let published = self.receiver.recv().await?;
            if self.is_current(published.epoch) {
                return Ok(published.event);
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct DownloadEvents {
    sender: broadcast::Sender<PublishedDownload>,
    epoch: std::sync::Arc<std::sync::Mutex<u64>>,
}

impl Default for DownloadEvents {
    fn default() -> Self {
        Self {
            sender: broadcast::channel(128).0,
            epoch: std::sync::Arc::new(std::sync::Mutex::new(0)),
        }
    }
}

impl DownloadEvents {
    pub fn subscribe(&self) -> DownloadSubscription {
        DownloadSubscription {
            receiver: self.sender.subscribe(),
            epoch: self.epoch.clone(),
        }
    }

    pub fn epoch(&self) -> u64 {
        *self
            .epoch
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub fn invalidate(&self) -> u64 {
        let mut epoch = self
            .epoch
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        *epoch = epoch.wrapping_add(1);
        *epoch
    }

    fn publish(&self, event: DownloadEvent, epoch: u64) {
        // Serialize generation validation + send against Restore/settings changes.
        let current = self
            .epoch
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if *current == epoch {
            let _ = self.sender.send(PublishedDownload { event, epoch });
        }
    }

    pub fn terminal(&self, status: &VodJobStatus, download: bool, epoch: u64) {
        if !download || status.running || !matches!(status.state.as_str(), "COMPLETED" | "FAILED") {
            return;
        }
        if let Some(job_id) = &status.job_id {
            self.publish(
                DownloadEvent {
                    job_id: job_id.clone(),
                    platform: status.platform,
                    completed: status.state == "COMPLETED",
                },
                epoch,
            );
        }
    }

    pub fn start_failed(&self, id: &str, attempt: u32, platform: PlatformId, epoch: u64) {
        self.publish(
            DownloadEvent {
                job_id: format!("queue:{id}:{attempt}"),
                platform,
                completed: false,
            },
            epoch,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_download_results_are_emitted_without_sensitive_details() {
        let events = DownloadEvents::default();
        let mut receiver = events.subscribe();
        for platform in [PlatformId::Soop, PlatformId::Chzzk] {
            let mut status = VodJobStatus {
                platform,
                job_id: Some("job".into()),
                state: "COMPLETED".into(),
                message: "secret error".into(),
                output_file: Some("private path".into()),
                ..Default::default()
            };
            events.terminal(&status, false, events.epoch());
            assert!(receiver.try_recv().is_err());
            for state in ["READY", "CANCELLED", "CANCELLING"] {
                status.state = state.into();
                events.terminal(&status, true, events.epoch());
                assert!(receiver.try_recv().is_err());
            }
            for (state, completed) in [("COMPLETED", true), ("FAILED", false)] {
                status.state = state.into();
                events.terminal(&status, true, events.epoch());
                assert_eq!(
                    receiver.try_recv().unwrap(),
                    DownloadEvent {
                        job_id: "job".into(),
                        platform,
                        completed,
                    }
                );
            }
        }
    }

    #[test]
    fn new_subscriber_does_not_replay_old_results_and_queue_retries_are_distinct() {
        let events = DownloadEvents::default();
        events.start_failed("old", 1, PlatformId::Soop, events.epoch());
        let mut receiver = events.subscribe();
        assert!(receiver.try_recv().is_err());
        for attempt in [1, 2] {
            events.start_failed("item", attempt, PlatformId::Chzzk, events.epoch());
            assert_eq!(
                receiver.try_recv().unwrap().job_id,
                format!("queue:item:{attempt}")
            );
        }
    }

    #[test]
    fn restore_invalidates_late_old_results_but_keeps_new_jobs() {
        let events = DownloadEvents::default();
        let mut receiver = events.subscribe();
        let old_epoch = events.epoch();
        let status = VodJobStatus {
            state: "COMPLETED".into(),
            job_id: Some("late-old-job".into()),
            ..Default::default()
        };
        events.invalidate();
        events.terminal(&status, true, old_epoch);
        assert!(receiver.try_recv().is_err());
        events.terminal(&status, true, events.epoch());
        assert!(receiver.try_recv().unwrap().completed);
    }

    #[test]
    fn transition_filters_queued_old_results_without_dropping_new_completions() {
        let events = DownloadEvents::default();
        let mut receiver = events.subscribe();
        for attempt in 0..300 {
            events.start_failed("old", attempt, PlatformId::Soop, events.epoch());
        }
        events.invalidate();
        events.start_failed("new", 1, PlatformId::Chzzk, events.epoch());
        assert!(matches!(
            receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(_))
        ));
        // Remaining old queued events are filtered, while the new result survives.
        assert_eq!(receiver.try_recv().unwrap().job_id, "queue:new:1");
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn lag_is_bounded_and_does_not_block_downloads() {
        let events = DownloadEvents::default();
        let mut receiver = events.subscribe();
        for attempt in 0..300 {
            events.start_failed("item", attempt, PlatformId::Soop, events.epoch());
        }
        assert!(matches!(
            receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(_))
        ));
        assert!(receiver.try_recv().is_ok());
    }
}
