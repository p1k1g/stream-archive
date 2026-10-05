use super::{PlatformId, chzzk, detect_vod_platform, kick, provider, soop};
use crate::{
    backend::LogBuffer,
    model::{VodAnalyzeRequest, VodDownloadRequest, VodJobStatus},
};
use anyhow::{Result, bail};
use std::path::PathBuf;
use tokio::sync::Mutex;

pub(crate) fn validate_thumbnail_url(platform: PlatformId, url: &url::Url) -> bool {
    match platform {
        PlatformId::Soop => soop::vod::valid_thumbnail_url(url),
        PlatformId::Chzzk => chzzk::vod::valid_thumbnail_url(url),
        PlatformId::Kick => false,
    }
}

/// Platform-neutral VOD facade.
///
/// Queue/history/UI code talks only to this type. Platform-specific metadata,
/// authentication and download mechanics live under the selected provider.
pub struct VodManager {
    soop: soop::vod::VodManager,
    chzzk: chzzk::vod::VodManager,
    kick: kick::vod::VodManager,
    selected: Mutex<PlatformId>,
    lifecycle: Mutex<()>,
    events: crate::download_events::DownloadEvents,
}

impl VodManager {
    pub fn new(backend_dir: PathBuf, logs: LogBuffer) -> Self {
        let events = crate::download_events::DownloadEvents::default();
        Self {
            soop: soop::vod::VodManager::new_with_events(
                backend_dir.clone(),
                logs.clone(),
                events.clone(),
            ),
            chzzk: chzzk::vod::VodManager::new_with_events(
                backend_dir.clone(),
                logs.clone(),
                events.clone(),
            ),
            kick: kick::vod::VodManager::new_with_events(backend_dir, logs, events.clone()),
            events,
            selected: Mutex::new(PlatformId::Soop),
            lifecycle: Mutex::new(()),
        }
    }

    pub fn subscribe_download_events(&self) -> crate::download_events::DownloadSubscription {
        self.events.subscribe()
    }

    pub(crate) fn invalidate_download_events(&self) {
        self.events.invalidate();
    }

    pub(crate) async fn notification_settings_changed(&self) {
        let (soop_status, soop_epoch) = self.soop.notification_state();
        let (chzzk_status, chzzk_epoch) = self.chzzk.notification_state();
        let (kick_status, kick_epoch) = self.kick.notification_state();
        // Idle commits and new job initialization use these same status locks.
        // Already idle jobs keep their invalidated token; active jobs are rearmed.
        let soop = soop_status.write().await;
        let chzzk = chzzk_status.write().await;
        let kick = kick_status.write().await;
        let epoch = self.events.invalidate();
        if soop.running {
            soop_epoch.store(epoch, std::sync::atomic::Ordering::Release);
        }
        if kick.running {
            kick_epoch.store(epoch, std::sync::atomic::Ordering::Release);
        }
        if chzzk.running {
            chzzk_epoch.store(epoch, std::sync::atomic::Ordering::Release);
        }
    }

    pub(crate) fn download_event_epoch(&self) -> u64 {
        self.events.epoch()
    }

    pub(crate) fn report_start_failure(&self, item: &crate::model::VodQueueItem, epoch: u64) {
        self.events
            .start_failed(&item.id, item.attempts, item.platform, epoch);
    }

    async fn provider_status(&self, platform: PlatformId) -> VodJobStatus {
        match platform {
            PlatformId::Soop => self.soop.status().await,
            PlatformId::Chzzk => self.chzzk.status().await,
            PlatformId::Kick => self.kick.status().await,
        }
    }

    async fn running_provider(&self) -> Option<(PlatformId, VodJobStatus)> {
        let soop = self.soop.status().await;
        if soop.running {
            return Some((PlatformId::Soop, soop));
        }
        let chzzk = self.chzzk.status().await;
        if chzzk.running {
            return Some((PlatformId::Chzzk, chzzk));
        }
        let kick = self.kick.status().await;
        if kick.running {
            return Some((PlatformId::Kick, kick));
        }
        None
    }

    async fn ensure_idle(&self) -> Result<()> {
        if let Some((platform, _)) = self.running_provider().await {
            bail!("다른 {platform} VOD 작업이 이미 실행 중입니다.");
        }
        Ok(())
    }

    pub async fn status(&self) -> VodJobStatus {
        if let Some((platform, status)) = self.running_provider().await {
            *self.selected.lock().await = platform;
            return status;
        }
        let selected = *self.selected.lock().await;
        self.provider_status(selected).await
    }

    pub async fn terminal_status(&self, job_id: &str) -> Option<VodJobStatus> {
        let selected = *self.selected.lock().await;
        let primary = match selected {
            PlatformId::Soop => self.soop.terminal_status(job_id).await,
            PlatformId::Chzzk => self.chzzk.terminal_status(job_id).await,
            PlatformId::Kick => self.kick.terminal_status(job_id).await,
        };
        if primary.is_some() {
            return primary;
        }
        for platform in [PlatformId::Soop, PlatformId::Chzzk, PlatformId::Kick] {
            let result = match platform {
                PlatformId::Soop => self.soop.terminal_status(job_id).await,
                PlatformId::Chzzk => self.chzzk.terminal_status(job_id).await,
                PlatformId::Kick => self.kick.terminal_status(job_id).await,
            };
            if result.is_some() {
                return result;
            }
        }
        None
    }

    pub async fn analyze(&self, req: VodAnalyzeRequest) -> Result<VodJobStatus> {
        let platform = vod_platform(&req.vod_url)?;
        let _lifecycle = self.lifecycle.lock().await;
        self.ensure_idle().await?;
        *self.selected.lock().await = platform;
        match platform {
            PlatformId::Soop => self.soop.analyze(req).await,
            PlatformId::Chzzk => self.chzzk.analyze(req).await,
            PlatformId::Kick => self.kick.analyze(req).await,
        }
    }

    pub async fn download(&self, req: VodDownloadRequest) -> Result<VodJobStatus> {
        let platform = vod_platform(&req.vod_url)?;
        let _lifecycle = self.lifecycle.lock().await;
        self.ensure_idle().await?;
        *self.selected.lock().await = platform;
        match platform {
            PlatformId::Soop => self.soop.download(req).await,
            PlatformId::Chzzk => self.chzzk.download(req).await,
            PlatformId::Kick => self.kick.download(req).await,
        }
    }

    pub async fn cancel(&self) -> Result<VodJobStatus> {
        let _lifecycle = self.lifecycle.lock().await;
        let platform = self
            .running_provider()
            .await
            .map(|(platform, _)| platform)
            .unwrap_or(*self.selected.lock().await);
        *self.selected.lock().await = platform;
        match platform {
            PlatformId::Soop => self.soop.cancel().await,
            PlatformId::Chzzk => self.chzzk.cancel().await,
            PlatformId::Kick => self.kick.cancel().await,
        }
    }
}

pub(crate) fn validate_download_request(req: &VodDownloadRequest) -> Result<()> {
    match vod_platform(&req.vod_url)? {
        PlatformId::Soop => soop::vod::validate_download_request(req),
        PlatformId::Chzzk => chzzk::vod::validate_download_request(req),
        PlatformId::Kick => kick::vod::validate_download_request(req),
    }
}

fn vod_platform(raw_url: &str) -> Result<PlatformId> {
    let platform = detect_vod_platform(raw_url)?;
    if !provider(platform).capabilities().vod {
        bail!("{platform} VOD is not supported");
    }
    Ok(platform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_vod_facade_routes_soop_and_chzzk() {
        assert_eq!(
            vod_platform("https://vod.sooplive.com/player/123456789").unwrap(),
            PlatformId::Soop
        );
        assert_eq!(
            vod_platform("https://chzzk.naver.com/video/123456").unwrap(),
            PlatformId::Chzzk
        );
        assert!(vod_platform("https://chzzk.naver.com/live/123456").is_err());
        assert!(vod_platform("https://example.com/player/123456789").is_err());
    }
    #[tokio::test]
    async fn settings_transition_excludes_idle_results_and_rearms_active_jobs() {
        for platform in [PlatformId::Soop, PlatformId::Chzzk, PlatformId::Kick] {
            let dir = tempfile::tempdir().unwrap();
            let manager = VodManager::new(dir.path().to_path_buf(), LogBuffer::new());
            let mut receiver = manager.subscribe_download_events();
            let (status, token) = match platform {
                PlatformId::Soop => manager.soop.notification_state(),
                PlatformId::Chzzk => manager.chzzk.notification_state(),
                PlatformId::Kick => manager.kick.notification_state(),
            };
            let finished = VodJobStatus {
                platform,
                state: "COMPLETED".into(),
                job_id: Some("finished-while-disabled".into()),
                ..Default::default()
            };
            *status.write().await = finished.clone();
            token.store(manager.events.epoch(), std::sync::atomic::Ordering::Release);
            manager.notification_settings_changed().await;
            manager.events.terminal(
                &finished,
                true,
                token.load(std::sync::atomic::Ordering::Acquire),
            );
            assert!(receiver.try_recv().is_err());

            let mut active = finished.clone();
            active.job_id = Some("still-active-at-enable".into());
            active.running = true;
            active.state = "DOWNLOADING".into();
            *status.write().await = active.clone();
            token.store(manager.events.epoch(), std::sync::atomic::Ordering::Release);
            manager.notification_settings_changed().await;
            active.running = false;
            active.state = "COMPLETED".into();
            *status.write().await = active.clone();
            manager.events.terminal(
                &active,
                true,
                token.load(std::sync::atomic::Ordering::Acquire),
            );
            assert_eq!(
                receiver.try_recv().unwrap().job_id,
                "still-active-at-enable"
            );
        }
    }

    #[tokio::test]
    async fn real_provider_task_failures_reach_the_shared_subscriber_offline() {
        for (platform, url) in [
            (
                PlatformId::Soop,
                "https://vod.sooplive.com/player/123456789",
            ),
            (PlatformId::Chzzk, "https://chzzk.naver.com/video/123456"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let manager = VodManager::new(dir.path().to_path_buf(), LogBuffer::new());
            let mut receiver = manager.subscribe_download_events();
            let missing = dir.path().join("missing-media-tool").display().to_string();
            let started = manager
                .download(VodDownloadRequest {
                    vod_url: url.into(),
                    output_directory: dir.path().join("out").display().to_string(),
                    parts: vec![],
                    quality: "best".into(),
                    merge: true,
                    cookie_mode: "SOOP_LOGIN".into(),
                    cookie_file: String::new(),
                    browser_name: "firefox".into(),
                    yt_dlp_path: missing.clone(),
                    ffmpeg_path: missing,
                    max_retries: 0,
                })
                .await
                .unwrap();
            let event = tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(event.platform, platform);
            assert_eq!(Some(event.job_id.as_str()), started.job_id.as_deref());
            assert!(!event.completed);
            assert!(receiver.try_recv().is_err());
            assert_eq!(
                manager.terminal_status(&event.job_id).await.unwrap().state,
                "FAILED"
            );
        }
    }
}
