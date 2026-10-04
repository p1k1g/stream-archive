use super::{PlatformCapabilities, PlatformProvider};
use anyhow::{Result, bail};
use reqwest::{Client, RequestBuilder};
use serde_json::Value;
use url::Url;

pub mod live;
pub(crate) static KICK: KickProvider = KickProvider;
pub(crate) struct KickProvider;

impl PlatformProvider for KickProvider {
    fn display_name(&self) -> &'static str {
        "KICK"
    }
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities {
            channel_lookup: true,
            live: true,
            vod: false,
        }
    }
    fn validate_account(&self, account: &str) -> Result<()> {
        let account = account.trim();
        if account.is_empty()
            || account.len() > 100
            || !account
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            bail!(
                "KICK 채널 ID에는 채널 URL의 마지막 이름(slug)을 입력하세요. 영문·숫자·_·-만 사용할 수 있습니다."
            );
        }
        Ok(())
    }
    fn channel_lookup_request(&self, client: &Client, account: &str) -> RequestBuilder {
        client.get(format!(
            "https://kick.com/api/v2/channels/{}",
            account.trim().to_ascii_lowercase()
        ))
    }
    fn parse_channel_name(&self, account: &str, value: &Value) -> Result<String> {
        self.validate_account(account)?;
        if !value
            .get("slug")
            .and_then(Value::as_str)
            .is_some_and(|slug| slug.eq_ignore_ascii_case(account.trim()))
        {
            bail!("KICK 채널 응답이 요청한 채널과 일치하지 않습니다.");
        }
        if value.get("is_banned").and_then(Value::as_bool) == Some(true) {
            bail!("이 KICK 채널은 이용할 수 없습니다.");
        }
        value
            .pointer("/user/username")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("KICK 채널 이름을 받지 못했습니다."))
    }
    fn profile_image_url(&self, account: &str, metadata: Option<&Value>) -> Result<Url> {
        let value = metadata.ok_or_else(|| anyhow::anyhow!("KICK 채널 정보가 없습니다."))?;
        self.parse_channel_name(account, value)?;
        let url = Url::parse(
            value
                .pointer("/user/profile_pic")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )?;
        if !live::valid_thumbnail_url(&url) {
            bail!("허용되지 않은 KICK 이미지 주소입니다.");
        }
        Ok(url)
    }
    fn accepts_vod_url(&self, _url: &Url) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_slug_identity_and_live_only_capabilities() {
        for id in ["xqc", "Some_Channel-1", " example "] {
            assert!(KICK.validate_account(id).is_ok());
        }
        for id in ["", "../xqc", "https://kick.com/xqc", "xqc?foo=bar", "채널"] {
            assert!(KICK.validate_account(id).is_err());
        }
        let mut value = serde_json::json!({"slug":"xqc","user":{"username":"xQc"}});
        assert_eq!(KICK.parse_channel_name("XQC", &value).unwrap(), "xQc");
        assert!(KICK.parse_channel_name("other", &value).is_err());
        value["is_banned"] = true.into();
        assert!(KICK.parse_channel_name("xqc", &value).is_err());
        assert!(KICK.capabilities().live);
        assert!(!KICK.capabilities().vod);
        assert!(!KICK.accepts_vod_url(&Url::parse("https://kick.com/xqc/videos/123").unwrap()));
    }
}
