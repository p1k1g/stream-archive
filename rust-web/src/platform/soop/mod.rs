use super::{PlatformCapabilities, PlatformId, PlatformProvider};
use anyhow::{Result, bail};
use reqwest::{Client, RequestBuilder};
use serde_json::Value;
use url::Url;

pub mod live;
pub mod vod;

pub(crate) static SOOP: SoopProvider = SoopProvider;

pub(crate) struct SoopProvider;

impl PlatformProvider for SoopProvider {
    fn id(&self) -> PlatformId {
        PlatformId::Soop
    }

    fn display_name(&self) -> &'static str {
        "SOOP"
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities {
            channel_lookup: true,
            live: true,
            vod: true,
        }
    }

    fn validate_account(&self, account: &str) -> Result<()> {
        if account.is_empty() {
            bail!("계정 ID가 비어 있습니다.");
        }
        if !account
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        {
            bail!("계정 ID 형식이 올바르지 않습니다.");
        }
        Ok(())
    }

    fn channel_lookup_request(&self, client: &Client, account: &str) -> RequestBuilder {
        client
            .get("https://st.sooplive.com/api/get_station_status.php")
            .query(&[("szBjId", account)])
    }

    fn parse_channel_name(&self, account: &str, value: &Value) -> Result<String> {
        let result = value.get("RESULT").and_then(Value::as_i64).unwrap_or(0);
        if result == 0 {
            bail!("유효한 SOOP 계정을 찾지 못했습니다: {account}");
        }

        let returned_id = value
            .pointer("/DATA/user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !returned_id.is_empty() && !returned_id.eq_ignore_ascii_case(account) {
            bail!("SOOP 응답 계정이 요청과 다릅니다: 요청={account}, 응답={returned_id}");
        }

        let name = value
            .pointer("/DATA/user_nick")
            .and_then(Value::as_str)
            .or_else(|| value.get("station_name").and_then(Value::as_str))
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            bail!("채널 닉네임을 찾지 못했습니다: {account}");
        }
        Ok(name.to_string())
    }

    fn accepts_vod_url(&self, url: &Url) -> bool {
        matches!(
            url.host_str(),
            Some("vod.sooplive.com" | "www.sooplive.com" | "sooplive.com")
        ) && (url.path().contains("/player/") || url.path().contains("/station/video/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_nickname_shape() {
        let value = serde_json::json!({
            "RESULT": 1,
            "DATA": {"user_id": "1004ysus", "user_nick": "테스트닉"}
        });
        assert_eq!(
            SOOP.parse_channel_name("1004ysus", &value).unwrap(),
            "테스트닉"
        );
    }

    #[test]
    fn keeps_legacy_nickname_fallback() {
        let value = serde_json::json!({"RESULT": 1, "station_name": "구형닉"});
        assert_eq!(SOOP.parse_channel_name("legacy", &value).unwrap(), "구형닉");
    }
}
