//! Presentation-only deduplication and notification pacing. Never controls downloads.
use std::collections::VecDeque;
use stream_archive_server::download_events::DownloadEvent;
use tokio::sync::broadcast::error::TryRecvError;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub completed: i32,
    pub failed: i32,
}
impl Summary {
    pub fn is_empty(self) -> bool {
        self.completed == 0 && self.failed == 0
    }
}

pub fn discard_stale(receiver: &mut tokio::sync::broadcast::Receiver<DownloadEvent>) {
    while let Ok(_) | Err(TryRecvError::Lagged(_)) = receiver.try_recv() {}
}

#[derive(Default)]
pub struct Tracker {
    seen: VecDeque<(stream_archive_server::support::platform::PlatformId, String)>,
}
impl Tracker {
    pub fn clear(&mut self) {
        self.seen.clear();
    }

    pub fn collect(&mut self, events: Vec<DownloadEvent>, enabled: bool) -> Summary {
        let mut summary = Summary::default();
        for event in events {
            let key = (event.platform, event.job_id);
            if self.seen.contains(&key) {
                continue;
            }
            if self.seen.len() == 256 {
                self.seen.pop_front();
            }
            self.seen.push_back(key);
            // Disabled results are consumed, never replayed on re-enable.
            if enabled {
                if event.completed {
                    summary.completed += 1;
                } else {
                    summary.failed += 1;
                }
            }
        }
        summary
    }
}

#[cfg(any(windows, test))]
#[derive(Default)]
pub struct NoticeQueue {
    pending: Summary,
    first_pending: Option<std::time::Instant>,
    submitted: Option<std::time::Instant>,
    in_flight: bool,
}
#[cfg(any(windows, test))]
impl NoticeQueue {
    pub fn add(&mut self, summary: Summary, now: std::time::Instant) {
        if summary.is_empty() {
            return;
        }
        self.pending.completed = self.pending.completed.saturating_add(summary.completed);
        self.pending.failed = self.pending.failed.saturating_add(summary.failed);
        self.first_pending.get_or_insert(now);
    }

    pub fn clear(&mut self) {
        self.pending = Summary::default();
        self.first_pending = None;
        self.in_flight = false;
    }

    pub fn closed(&mut self) {
        self.in_flight = false;
    }

    pub fn take(&mut self, now: std::time::Instant) -> Option<Summary> {
        use std::time::Duration;
        let first = self.first_pending?;
        if now.duration_since(first) < Duration::from_secs(2) {
            return None;
        }
        if let Some(submitted) = self.submitted {
            let elapsed = now.duration_since(submitted);
            // Shell suppression may produce no balloon lifecycle callback.
            if elapsed < Duration::from_secs(10)
                || (self.in_flight && elapsed < Duration::from_secs(45))
            {
                return None;
            }
        }
        let summary = std::mem::take(&mut self.pending);
        self.first_pending = None;
        self.submitted = Some(now);
        self.in_flight = true;
        Some(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use stream_archive_server::support::platform::PlatformId;
    fn event(id: &str, completed: bool) -> DownloadEvent {
        DownloadEvent {
            job_id: id.into(),
            platform: PlatformId::Soop,
            completed,
        }
    }

    #[test]
    fn configuration_transition_discards_buffered_and_lagged_results() {
        let (sender, mut receiver) = tokio::sync::broadcast::channel(128);
        for id in 0..300 {
            sender.send(event(&id.to_string(), true)).unwrap();
        }
        discard_stale(&mut receiver);
        assert!(receiver.try_recv().is_err());
        sender.send(event("new-session-result", true)).unwrap();
        assert_eq!(receiver.try_recv().unwrap().job_id, "new-session-result");
    }

    #[test]
    fn restored_queue_attempt_can_notify_again_after_clearing_the_old_epoch() {
        let mut tracker = Tracker::default();
        let attempt = event("queue:restored-item:2", false);
        assert_eq!(tracker.collect(vec![attempt.clone()], true).failed, 1);
        assert!(tracker.collect(vec![attempt.clone()], true).is_empty());
        // Restore may rewind persisted attempts; the same key is a new failure.
        tracker.clear();
        assert_eq!(tracker.collect(vec![attempt.clone()], true).failed, 1);
        assert!(tracker.collect(vec![attempt], true).is_empty());
    }

    #[test]
    fn duplicates_disabled_results_and_retries() {
        let mut tracker = Tracker::default();
        assert_eq!(
            tracker.collect(
                vec![event("1", true), event("1", true), event("2", false)],
                true
            ),
            Summary {
                completed: 1,
                failed: 1
            }
        );
        assert!(tracker.collect(vec![event("3", true)], false).is_empty());
        assert!(tracker.collect(vec![event("3", true)], true).is_empty());
        assert_eq!(
            tracker.collect(vec![event("retry", true)], true).completed,
            1
        );
        for id in 0..1000 {
            tracker.collect(vec![event(&id.to_string(), true)], false);
        }
        assert_eq!(tracker.seen.len(), 256);
    }

    #[test]
    fn batches_bursts_spaces_balloons_and_recovers_without_callbacks() {
        let now = Instant::now();
        let mut queue = NoticeQueue::default();
        queue.add(
            Summary {
                completed: 1,
                failed: 0,
            },
            now,
        );
        queue.add(
            Summary {
                completed: 1,
                failed: 1,
            },
            now,
        );
        assert!(queue.take(now).is_none());
        assert_eq!(
            queue.take(now + Duration::from_secs(2)),
            Some(Summary {
                completed: 2,
                failed: 1
            })
        );
        queue.add(
            Summary {
                completed: 1,
                failed: 0,
            },
            now + Duration::from_secs(3),
        );
        assert!(queue.take(now + Duration::from_secs(15)).is_none());
        assert!(queue.take(now + Duration::from_secs(47)).is_some());
        queue.add(
            Summary {
                completed: 1,
                failed: 0,
            },
            now + Duration::from_secs(48),
        );
        queue.closed();
        assert!(queue.take(now + Duration::from_secs(50)).is_none());
        assert!(queue.take(now + Duration::from_secs(57)).is_some());
        queue.add(
            Summary {
                completed: 1,
                failed: 0,
            },
            now + Duration::from_secs(58),
        );
        queue.clear();
        assert!(queue.take(now + Duration::from_secs(100)).is_none());
    }
}
