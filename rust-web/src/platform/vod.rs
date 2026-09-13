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
}

impl VodManager {
    pub fn new(backend_dir: PathBuf, logs: LogBuffer) -> Self {
        Self {
            soop: soop::vod::VodManager::new(backend_dir.clone(), logs.clone()),
            chzzk: chzzk::vod::VodManager::new(backend_dir, logs),
            selected: Mutex::new(PlatformId::Soop),
        }
    }

    pub async fn status(&self) -> VodJobStatus {
        match *self.selected.lock().await {
            PlatformId::Soop => self.soop.status().await,
            PlatformId::Chzzk => self.chzzk.status().await,
        }
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
        *self.selected.lock().await = platform;
        match platform {
            PlatformId::Soop => self.soop.analyze(req).await,
            PlatformId::Chzzk => self.chzzk.analyze(req).await,
        }
    }

    pub async fn download(&self, req: VodDownloadRequest) -> Result<VodJobStatus> {
        let platform = vod_platform(&req.vod_url)?;
        *self.selected.lock().await = platform;
        match platform {
            PlatformId::Soop => self.soop.download(req).await,
            PlatformId::Chzzk => self.chzzk.download(req).await,
        }
    }

    pub async fn cancel(&self) -> Result<VodJobStatus> {
        match *self.selected.lock().await {
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
