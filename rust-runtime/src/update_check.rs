//! Lightweight GitHub Releases version check (read-only, no auto-download/install).

use crate::support::platform::read_bounded_json;
use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{sync::OnceLock, time::Duration};

pub const RELEASES_PAGE_URL: &str = "https://github.com/p1k1g/stream-archive/releases";
const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/p1k1g/stream-archive/releases/latest";
const MAX_RELEASE_BODY_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseUpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_url: String,
}

fn update_http_client() -> Result<Client> {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let client = Client::builder()
        .user_agent(format!("StreamArchive/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(8))
        .build()
        .context("업데이트 확인 HTTP client 생성 실패")?;
    let _ = CLIENT.set(client.clone());
    Ok(client)
}

pub async fn check_latest_release() -> Result<ReleaseUpdateInfo> {
    let client = update_http_client()?;
    let response = client
        .get(LATEST_RELEASE_API_URL)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .context("GitHub 최신 릴리스 조회 실패")?
        .error_for_status()
        .context("GitHub 최신 릴리스 HTTP 오류")?;
    let payload: Value =
        read_bounded_json(response, MAX_RELEASE_BODY_BYTES, "GitHub latest release").await?;
    parse_latest_release(env!("CARGO_PKG_VERSION"), &payload)
}

pub fn parse_latest_release(current_version: &str, payload: &Value) -> Result<ReleaseUpdateInfo> {
    let raw_tag = payload
        .get("tag_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("GitHub 릴리스 응답에 tag_name이 없습니다.")?;
    let latest_version = normalize_version(raw_tag)
        .with_context(|| format!("유효한 릴리스 버전 형식이 아닙니다: {raw_tag}"))?;
    let current = normalize_version(current_version)
        .with_context(|| format!("현재 버전 형식이 올바르지 않습니다: {current_version}"))?;
    let release_url = payload
        .get("html_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| is_allowed_release_url(url))
        .unwrap_or(RELEASES_PAGE_URL)
        .to_string();
    let update_available = is_newer_version(&current, &latest_version)?;
    Ok(ReleaseUpdateInfo {
        current_version: current,
        latest_version,
        update_available,
        release_url,
    })
}

pub fn is_allowed_release_url(url: &str) -> bool {
    let trimmed = url.trim();
    trimmed == RELEASES_PAGE_URL
        || trimmed.starts_with("https://github.com/p1k1g/stream-archive/releases/")
}

fn normalize_version(raw: &str) -> Result<String> {
    let stripped = raw.trim().trim_start_matches(['v', 'V']);
    let _ = parse_semver_triple(stripped)?;
    Ok(stripped.to_string())
}

fn parse_semver_triple(version: &str) -> Result<(u64, u64, u64)> {
    let core = version.split(['-', '+']).next().unwrap_or("");
    let mut parts = core.split('.');
    let major = parts
        .next()
        .and_then(|p| p.parse::<u64>().ok())
        .ok_or_else(|| anyhow::anyhow!("invalid major version: {version}"))?;
    let minor = parts
        .next()
        .and_then(|p| p.parse::<u64>().ok())
        .ok_or_else(|| anyhow::anyhow!("invalid minor version: {version}"))?;
    let patch = parts
        .next()
        .and_then(|p| p.parse::<u64>().ok())
        .ok_or_else(|| anyhow::anyhow!("invalid patch version: {version}"))?;
    if parts.next().is_some() {
        bail!("unexpected extra version segment: {version}");
    }
    Ok((major, minor, patch))
}

pub fn is_newer_version(current: &str, latest: &str) -> Result<bool> {
    let cur = parse_semver_triple(current.trim_start_matches(['v', 'V']))?;
    let lat = parse_semver_triple(latest.trim_start_matches(['v', 'V']))?;
    Ok(lat > cur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compares_semantic_versions_numerically() {
        assert!(is_newer_version("1.0.9", "1.1.0").unwrap());
        assert!(is_newer_version("1.9.0", "1.10.0").unwrap());
        assert!(is_newer_version("1.0.9", "2.0.0").unwrap());
        assert!(!is_newer_version("1.1.0", "1.1.0").unwrap());
        assert!(!is_newer_version("1.1.0", "1.0.9").unwrap());
        assert!(is_newer_version("1.0.9", "v1.1.0").unwrap());
        assert!(is_newer_version("1.0.9", "invalid").is_err());
    }

    #[test]
    fn parses_release_payload_and_restricts_url_origin() {
        let info = parse_latest_release(
            "1.0.9",
            &json!({
                "tag_name": "v1.1.0",
                "html_url": "https://github.com/p1k1g/stream-archive/releases/tag/v1.1.0"
            }),
        )
        .unwrap();
        assert_eq!(info.current_version, "1.0.9");
        assert_eq!(info.latest_version, "1.1.0");
        assert!(info.update_available);
        assert_eq!(
            info.release_url,
            "https://github.com/p1k1g/stream-archive/releases/tag/v1.1.0"
        );

        // Untrusted URLs fall back to the canonical releases page.
        let spoofed = parse_latest_release(
            "1.1.0",
            &json!({
                "tag_name": "v1.1.0",
                "html_url": "https://evil.example/download"
            }),
        )
        .unwrap();
        assert!(!spoofed.update_available);
        assert_eq!(spoofed.release_url, RELEASES_PAGE_URL);
    }
}
