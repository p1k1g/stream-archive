use anyhow::{Context, Result};
use reqwest::Client;
use std::{sync::OnceLock, time::Duration};

pub(crate) const PROVIDER_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36";

fn channel_lookup_client() -> Result<Client> {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let client = Client::builder()
        .user_agent(PROVIDER_USER_AGENT)
        .timeout(Duration::from_secs(10))
        .no_proxy()
        .http1_only()
        .build()
        .context("채널 조회용 HTTP client 생성 실패")?;
    let _ = CLIENT.set(client.clone());
    Ok(client)
}

#[path = "platform/mod.rs"]
pub mod platform;

use platform::{PlatformId, default_platform, provider};

pub(crate) fn sanitize_filename_segment(
    value: &str,
    max_chars: usize,
    fallback: Option<&str>,
) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() {
            if ch.is_whitespace() {
                out.push(' ');
            }
            continue;
        }
        if r#"\/:*?"<>|"#.contains(ch) {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let mut out = out
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches([' ', '.'])
        .to_string();
    if out.chars().count() > max_chars {
        out = out
            .chars()
            .take(max_chars)
            .collect::<String>()
            .trim_matches([' ', '.'])
            .to_string();
    }
    if out.is_empty() {
        return fallback.unwrap_or_default().to_string();
    }
    if is_windows_reserved_device_name(&out) {
        out.insert(0, '_');
    }
    out
}

fn is_windows_reserved_device_name(name: &str) -> bool {
    matches!(
        name.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

pub async fn resolve_channel_name(account: &str) -> Result<String> {
    let account = account.trim();
    if let Some((prefix, value)) = account.split_once(':')
        && let Ok(platform) = prefix.parse::<PlatformId>()
        && !prefix.trim().is_empty()
    {
        return resolve_channel_name_for(platform, value).await;
    }
    resolve_channel_name_for(default_platform(), account).await
}

pub async fn resolve_channel_name_for(platform: PlatformId, account: &str) -> Result<String> {
    let account = account.trim();
    let provider = provider(platform);
    provider.validate_account(account)?;

    let client = channel_lookup_client()
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

    #[test]
    fn platform_prefixes_are_unambiguous() {
        assert!("CHZZK:0123456789abcdef0123456789abcdef".starts_with("CHZZK:"));
        assert!("SOOP:example".starts_with("SOOP:"));
    }

    #[test]
    fn sanitize_filename_segment_replaces_invalid_chars_truncates_and_guards_reserved() {
        assert_eq!(
            sanitize_filename_segment("a:b/c", 100, Some("UNKNOWN")),
            "a_b_c"
        );
        assert_eq!(
            sanitize_filename_segment("CON", 100, Some("UNKNOWN")),
            "_CON"
        );
        assert_eq!(
            sanitize_filename_segment("  ...  ", 100, Some("UNKNOWN")),
            "UNKNOWN"
        );
        assert_eq!(sanitize_filename_segment("  ...  ", 60, None), "");
        assert_eq!(
            sanitize_filename_segment("a   b\tc", 100, Some("UNKNOWN")),
            "a b c"
        );
        assert_eq!(
            sanitize_filename_segment("abcdef.", 4, Some("UNKNOWN")),
            "abcd"
        );
    }
}
