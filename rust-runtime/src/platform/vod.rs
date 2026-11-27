use super::{PlatformId, chzzk, detect_vod_platform, kick, provider, soop};
use crate::{
    backend::LogBuffer,
    model::{VodAnalyzeRequest, VodDownloadRequest, VodJobStatus},
};
use anyhow::{Result, bail};
use chrono::Utc;
use std::{
    collections::VecDeque,
    future::Future,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
};
use uuid::Uuid;

const TERMINAL_CACHE_LIMIT: usize = 32;

pub(crate) struct ProviderVodConfig {
    pub platform: PlatformId,
    pub busy_message: &'static str,
    pub starting_message: &'static str,
    pub cancelling_message: &'static str,
    pub cancelled_message: &'static str,
    pub cancel_log: &'static str,
    pub error_log_prefix: &'static str,
    pub log_job_id_on_error: bool,
    pub redact_error: fn(&str) -> String,
}

struct JobRuntime {
    task: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
}

pub(crate) fn should_mark_cancelled(cancel: &AtomicBool, state: &str) -> bool {
    cancel.load(Ordering::SeqCst) && state != "COMPLETED"
}

/// Shared single-active-job lifecycle, cancellation, terminal cache, and
/// notification epoch coordination reused by every VOD provider.
pub(crate) struct ProviderVodRunner {
    backend_dir: PathBuf,
    logs: LogBuffer,
    config: ProviderVodConfig,
    runtime: Mutex<JobRuntime>,
    status: Arc<RwLock<VodJobStatus>>,
    pub(crate) terminal: Arc<Mutex<VecDeque<(String, VodJobStatus)>>>,
    events: crate::download_events::DownloadEvents,
    notification_epoch: Arc<AtomicU64>,
}

impl ProviderVodRunner {
    pub(crate) fn new(
        backend_dir: PathBuf,
        logs: LogBuffer,
        events: crate::download_events::DownloadEvents,
        config: ProviderVodConfig,
    ) -> Self {
        let initial_status = VodJobStatus {
            platform: config.platform,
            ..Default::default()
        };
        Self {
            backend_dir,
            logs,
            config,
            runtime: Mutex::new(JobRuntime {
                task: None,
                cancel: Arc::new(AtomicBool::new(false)),
            }),
            status: Arc::new(RwLock::new(initial_status)),
            terminal: Arc::new(Mutex::new(VecDeque::new())),
            events,
            notification_epoch: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn notification_state(&self) -> (&RwLock<VodJobStatus>, &AtomicU64) {
        (&self.status, &self.notification_epoch)
    }

    #[cfg(test)]
    pub(crate) async fn take_task(&self) -> Option<JoinHandle<()>> {
        self.runtime.lock().await.task.take()
    }

    pub(crate) async fn status(&self) -> VodJobStatus {
        {
            let mut runtime = self.runtime.lock().await;
            if runtime.task.as_ref().is_some_and(|task| task.is_finished()) {
                runtime.task.take();
            }
        }
        self.status.read().await.clone()
    }

    pub(crate) async fn terminal_status(&self, job_id: &str) -> Option<VodJobStatus> {
        self.terminal
            .lock()
            .await
            .iter()
            .rev()
            .find(|(id, _)| id == job_id)
            .map(|(_, status)| status.clone())
    }

    pub(crate) async fn start_job<F, Fut>(&self, is_download: bool, work: F) -> Result<VodJobStatus>
    where
        F: FnOnce(PathBuf, LogBuffer, Arc<RwLock<VodJobStatus>>, Arc<AtomicBool>) -> Fut,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let mut runtime = self.runtime.lock().await;
        if runtime
            .task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            bail!("{}", self.config.busy_message);
        }
        runtime.task.take();

        let cancel = Arc::new(AtomicBool::new(false));
        runtime.cancel = cancel.clone();
        let backend = self.backend_dir.clone();
        let logs = self.logs.clone();
        let status = self.status.clone();
        let terminal = self.terminal.clone();
        let job_id = Uuid::new_v4().to_string();
        let terminal_job_id = job_id.clone();
        {
            let mut current = status.write().await;
            *current = VodJobStatus {
                platform: self.config.platform,
                state: "STARTING".into(),
                running: true,
                job_id: Some(job_id),
                message: self.config.starting_message.into(),
                started_at: Some(Utc::now().to_rfc3339()),
                ..Default::default()
            };
            self.notification_epoch
                .store(self.events.epoch(), Ordering::Release);
        }

        let events = self.events.clone();
        let event_epoch = self.notification_epoch.clone();
        let cancelled_message = self.config.cancelled_message;
        let cancel_log = self.config.cancel_log;
        let error_log_prefix = self.config.error_log_prefix;
        let log_job_id_on_error = self.config.log_job_id_on_error;
        let redact_error = self.config.redact_error;
        let future = work(backend, logs.clone(), status.clone(), cancel.clone());
        let task = tokio::spawn(async move {
            let result = future.await;
            let final_status = {
                let mut current = status.write().await;
                current.running = false;
                current.finished_at = Some(Utc::now().to_rfc3339());
                if should_mark_cancelled(&cancel, &current.state) {
                    current.state = "CANCELLED".into();
                    current.message = cancelled_message.into();
                    logs.push(cancel_log).await;
                } else if let Err(err) = result {
                    current.state = "FAILED".into();
                    current.message = redact_error(&format!("{err:#}"));
                    if log_job_id_on_error {
                        logs.push(format!(
                            "{error_log_prefix} job={terminal_job_id} {}",
                            current.message
                        ))
                        .await;
                    } else {
                        logs.push(format!("{error_log_prefix} {}", current.message))
                            .await;
                    }
                }
                current.clone()
            };
            let mut terminal = terminal.lock().await;
            if terminal.len() >= TERMINAL_CACHE_LIMIT {
                terminal.pop_front();
            }
            events.terminal(
                &final_status,
                is_download,
                event_epoch.load(Ordering::Acquire),
            );
            terminal.push_back((terminal_job_id, final_status));
        });
        runtime.task = Some(task);
        Ok(self.status.read().await.clone())
    }

    pub(crate) async fn cancel(&self) -> Result<VodJobStatus> {
        let task = {
            let mut runtime = self.runtime.lock().await;
            if runtime
                .task
                .as_ref()
                .is_some_and(|task| !task.is_finished())
            {
                runtime.cancel.store(true, Ordering::SeqCst);
            }
            runtime.task.take()
        };
        {
            let mut current = self.status.write().await;
            if current.running {
                current.state = "CANCELLING".into();
                current.message = self.config.cancelling_message.into();
            }
        }
        if let Some(task) = task {
            let _ = task.await;
        }
        Ok(self.status.read().await.clone())
    }
}

pub(crate) fn validate_thumbnail_url(platform: PlatformId, url: &url::Url) -> bool {
    match platform {
        PlatformId::Soop => soop::vod::valid_thumbnail_url(url),
        PlatformId::Chzzk => chzzk::vod::valid_thumbnail_url(url),
        PlatformId::Kick => kick::live::valid_thumbnail_url(url),
    }
}

pub(crate) async fn thumbnail_frame_source(
    platform: PlatformId,
    vod_url: &str,
    handle: &str,
) -> Result<String> {
    match platform {
        PlatformId::Kick => kick::vod::thumbnail_frame_source(vod_url, handle).await,
        _ => bail!("이 플랫폼은 VOD 첫 프레임 미리보기를 제공하지 않습니다."),
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
