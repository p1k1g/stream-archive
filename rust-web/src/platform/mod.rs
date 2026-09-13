use anyhow::{Result, bail};
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fmt, str::FromStr};
use url::Url;

pub mod chzzk;
pub mod live;
pub mod soop;
pub mod vod;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PlatformId {
    #[default]
    Soop,
    Chzzk,
}

impl PlatformId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Soop => "SOOP",
            Self::Chzzk => "CHZZK",
        }
    }

    pub const fn live_output_extension(self) -> &'static str {
        match self {
            Self::Soop => "ts",
            Self::Chzzk => "mp4",
        }
    }
}

impl fmt::Display for PlatformId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PlatformId {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "" | "SOOP" => Ok(Self::Soop),
            "CHZZK" => Ok(Self::Chzzk),
            other => bail!("지원하지 않는 플랫폼입니다: {other}"),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct PlatformCapabilities {
    pub channel_lookup: bool,
    pub live: bool,
    pub vod: bool,
}

pub trait PlatformProvider: Send + Sync {
    fn id(&self) -> PlatformId;
    fn display_name(&self) -> &'static str;
    fn capabilities(&self) -> PlatformCapabilities;
    fn validate_account(&self, account: &str) -> Result<()>;
    fn channel_lookup_request(&self, client: &Client, account: &str) -> RequestBuilder;
    fn parse_channel_name(&self, account: &str, value: &Value) -> Result<String>;
    fn accepts_vod_url(&self, url: &Url) -> bool;
}

pub fn provider(id: PlatformId) -> &'static dyn PlatformProvider {
    match id {
        PlatformId::Soop => &soop::SOOP,
        PlatformId::Chzzk => &chzzk::CHZZK,
    }
}

pub const fn default_platform() -> PlatformId {
    PlatformId::Soop
}

pub fn detect_vod_platform(raw_url: &str) -> Result<PlatformId> {
    let url = Url::parse(raw_url)?;
    for id in [PlatformId::Soop, PlatformId::Chzzk] {
        let provider = provider(id);
        if provider.capabilities().vod && provider.accepts_vod_url(&url) {
            return Ok(id);
        }
    }
    bail!("지원하지 않는 VOD URL입니다.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_id_defaults_to_soop_for_legacy_data() {
        assert_eq!(PlatformId::default(), PlatformId::Soop);
        assert_eq!("SOOP".parse::<PlatformId>().unwrap(), PlatformId::Soop);
        assert_eq!("soop".parse::<PlatformId>().unwrap(), PlatformId::Soop);
        assert_eq!("CHZZK".parse::<PlatformId>().unwrap(), PlatformId::Chzzk);
        assert_eq!("chzzk".parse::<PlatformId>().unwrap(), PlatformId::Chzzk);
    }

    #[test]
    fn live_output_extensions_match_platform_container() {
        assert_eq!(PlatformId::Soop.live_output_extension(), "ts");
        assert_eq!(PlatformId::Chzzk.live_output_extension(), "mp4");
    }

    #[test]
    fn detects_soop_and_chzzk_vod_urls() {
        assert_eq!(
            detect_vod_platform("https://vod.sooplive.com/player/123456789").unwrap(),
            PlatformId::Soop
        );
        assert_eq!(
            detect_vod_platform("https://chzzk.naver.com/video/123456").unwrap(),
            PlatformId::Chzzk
        );
        assert!(detect_vod_platform("https://chzzk.naver.com/live/123456").is_err());
        assert!(detect_vod_platform("https://example.com/player/123456789").is_err());
    }

    #[test]
    fn provider_capabilities_are_explicit() {
        let soop = provider(PlatformId::Soop).capabilities();
        assert!(soop.channel_lookup && soop.live && soop.vod);
        let chzzk = provider(PlatformId::Chzzk).capabilities();
        assert!(chzzk.channel_lookup && chzzk.live && chzzk.vod);
        assert_eq!(provider(PlatformId::Soop).display_name(), "SOOP");
        assert_eq!(provider(PlatformId::Chzzk).display_name(), "CHZZK");
    }
}
