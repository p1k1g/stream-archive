use super::{PlatformId, chzzk, detect_vod_platform, provider, soop};
use crate::{
    backend::LogBuffer,
    model::{VodAnalyzeRequest, VodDownloadRequest, VodJobStatus},
};
use anyhow::{Result, bail};
use std::path::PathBuf;
use tokio::sync::Mutex;

/// Platform-neutral VOD facade.
///
/// Queue/history/UI code talks only to this type. Platform-specific metadata,
/// authentication and download mechanics live under the selected provider.
pub struct VodManager {
    soop: soop::vod::VodManager,
    chzzk: chzzk::vod::VodManager,
    selected: Mutex<PlatformId>,
    lifecycle: Mutex<()>,
}

impl VodManager {
    pub fn new(backend_dir: PathBuf, logs: LogBuffer) -> Self {
        Self {
            soop: soop::vod::VodManager::new(backend_dir.clone(), logs.clone()),
            chzzk: chzzk::vod::VodManager::new(backend_dir, logs),
            selected: Mutex::new(PlatformId::Soop),
            lifecycle: Mutex::new(()),
        }
    }

    async fn provider_status(&self, platform: PlatformId) -> VodJobStatus {
        match platform {
            PlatformId::Soop => self.soop.status().await,
            PlatformId::Chzzk => self.chzzk.status().await,
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
        };
        if primary.is_some() {
            return primary;
        }
        match selected {
            PlatformId::Soop => self.chzzk.terminal_status(job_id).await,
            PlatformId::Chzzk => self.soop.terminal_status(job_id).await,
        }
    }

    pub async fn analyze(&self, req: VodAnalyzeRequest) -> Result<VodJobStatus> {
        let platform = vod_platform(&req.vod_url)?;
        let _lifecycle = self.lifecycle.lock().await;
        self.ensure_idle().await?;
        *self.selected.lock().await = platform;
        match platform {
            PlatformId::Soop => self.soop.analyze(req).await,
            PlatformId::Chzzk => self.chzzk.analyze(req).await,
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
        }
    }
}

pub(crate) fn validate_download_request(req: &VodDownloadRequest) -> Result<()> {
    match vod_platform(&req.vod_url)? {
        PlatformId::Soop => soop::vod::validate_download_request(req),
        PlatformId::Chzzk => chzzk::vod::validate_download_request(req),
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
}
