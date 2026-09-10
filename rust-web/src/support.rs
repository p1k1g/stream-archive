use anyhow::{Context, Result};
use reqwest::Client;
use std::time::Duration;

#[path = "platform.rs"]
pub mod platform;

use platform::{PlatformId, default_platform, provider};

pub async fn resolve_channel_name(account: &str) -> Result<String> {
    resolve_channel_name_for(default_platform(), account).await
}

pub async fn resolve_channel_name_for(platform: PlatformId, account: &str) -> Result<String> {
    let account = account.trim();
    let provider = provider(platform);
    provider.validate_account(account)?;

    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36")
        .timeout(Duration::from_secs(10))
        .no_proxy()
        .http1_only()
        .build()
        .with_context(|| format!("{} 조회용 HTTP client 생성 실패", provider.display_name()))?;

    let response = provider
        .channel_lookup_request(&client, account)
        .send()
        .await
        .with_context(|| format!("{} 채널 정보 요청 실패", provider.display_name()))?
        .error_for_status()
        .with_context(|| format!("{} 채널 정보 HTTP 오류", provider.display_name()))?;

    let value = response
        .json()
        .await
        .with_context(|| format!("{} 채널 정보 JSON 파싱 실패", provider.display_name()))?;

    provider.parse_channel_name(account, &value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_channel_provider_is_soop() {
        assert_eq!(default_platform(), PlatformId::Soop);
    }
}
