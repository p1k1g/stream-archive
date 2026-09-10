use super::{PlatformId, detect_vod_platform, provider, soop};
use crate::{
    backend::LogBuffer,
    model::{VodAnalyzeRequest, VodDownloadRequest, VodJobStatus},
};
use anyhow::{Result, bail};
use std::path::PathBuf;

/// Platform-neutral VOD facade.
///
/// Queue/history/UI code talks only to this type. Platform-specific metadata,
/// authentication and download mechanics live under the selected provider.
pub struct VodManager {
    soop: soop::vod::VodManager,
}

impl VodManager {
    pub fn new(backend_dir: PathBuf, logs: LogBuffer) -> Self {
        Self {
            soop: soop::vod::VodManager::new(backend_dir, logs),
        }
    }

    pub async fn status(&self) -> VodJobStatus {
        self.soop.status().await
    }

    pub async fn terminal_status(&self, job_id: &str) -> Option<VodJobStatus> {
        self.soop.terminal_status(job_id).await
    }

    pub async fn analyze(&self, req: VodAnalyzeRequest) -> Result<VodJobStatus> {
        match vod_platform(&req.vod_url)? {
            PlatformId::Soop => self.soop.analyze(req).await,
        }
    }

    pub async fn download(&self, req: VodDownloadRequest) -> Result<VodJobStatus> {
        match vod_platform(&req.vod_url)? {
            PlatformId::Soop => self.soop.download(req).await,
        }
    }

    pub async fn cancel(&self) -> Result<VodJobStatus> {
        // Phase 16 currently has one registered VOD provider. When CHZZK is
        // added, the common facade will track the active provider here rather
        // than teaching queue/history code about provider-specific managers.
        self.soop.cancel().await
    }
}

pub(crate) fn validate_download_request(req: &VodDownloadRequest) -> Result<()> {
    match vod_platform(&req.vod_url)? {
        PlatformId::Soop => soop::vod::validate_download_request(req),
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
    fn common_vod_facade_routes_soop_and_rejects_unknown_hosts() {
        assert_eq!(
            vod_platform("https://vod.sooplive.com/player/123456789").unwrap(),
            PlatformId::Soop
        );
        assert!(vod_platform("https://example.com/player/123456789").is_err());
    }
}
