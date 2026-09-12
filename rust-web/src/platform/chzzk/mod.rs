use super::{PlatformCapabilities, PlatformId, PlatformProvider};
use anyhow::{Result, bail};
use reqwest::{Client, RequestBuilder};
use serde_json::Value;
use url::Url;

pub mod live;

pub(crate) static CHZZK: ChzzkProvider = ChzzkProvider;

pub(crate) struct ChzzkProvider;

impl PlatformProvider for ChzzkProvider {
    fn id(&self) -> PlatformId {
        PlatformId::Chzzk
    }

    fn display_name(&self) -> &'static str {
        "CHZZK"
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
        if account.is_empty() {
            bail!("CHZZK 채널 ID가 비어 있습니다.");
        }
        if account.len() != 32 || !account.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("CHZZK 채널 ID는 32자리 16진수 형식이어야 합니다.");
        }
        Ok(())
    }

    fn channel_lookup_request(&self, client: &Client, account: &str) -> RequestBuilder {
        client.get(format!(
            "https://api.chzzk.naver.com/service/v1/channels/{}",
            account.trim()
        ))
    }

    fn parse_channel_name(&self, account: &str, value: &Value) -> Result<String> {
        if value.get("code").and_then(Value::as_i64) != Some(200) {
            bail!("유효한 CHZZK 채널을 찾지 못했습니다: {account}");
        }
        let content = value
            .get("content")
            .ok_or_else(|| anyhow::anyhow!("CHZZK 채널 정보가 없습니다: {account}"))?;
        let returned_id = content
            .get("channelId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !returned_id.is_empty() && !returned_id.eq_ignore_ascii_case(account.trim()) {
            bail!("CHZZK 응답 채널 ID가 요청과 다릅니다.");
        }
        let name = content
            .get("channelName")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            bail!("CHZZK 채널 이름을 찾지 못했습니다: {account}");
        }
        Ok(name.to_string())
    }

    fn accepts_vod_url(&self, _url: &Url) -> bool {
        // Phase 17 deliberately exposes CHZZK LIVE only. VOD routing is enabled
        // in Phase 18 after the common CHZZK authentication path is stabilized.
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHANNEL: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn validates_channel_id_shape() {
        assert!(CHZZK.validate_account(CHANNEL).is_ok());
        assert!(CHZZK.validate_account("not-a-channel-id").is_err());
    }

    #[test]
    fn parses_channel_name() {
        let value = serde_json::json!({
            "code": 200,
            "content": {
                "channelId": CHANNEL,
                "channelName": "테스트 채널"
            }
        });
        assert_eq!(
            CHZZK.parse_channel_name(CHANNEL, &value).unwrap(),
            "테스트 채널"
        );
    }
}
