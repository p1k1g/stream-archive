use anyhow::{Context, Result, bail};
use reqwest::{Client, RequestBuilder, Response};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{fmt, str::FromStr};
use url::Url;

pub(crate) const DEFAULT_API_BODY_LIMIT: usize = 2 * 1024 * 1024;
pub(crate) const DEFAULT_HTML_BODY_LIMIT: usize = 4 * 1024 * 1024;

pub(crate) async fn read_bounded_bytes(
    mut response: Response,
    max_bytes: usize,
    label: &str,
) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|len| len > max_bytes as u64)
    {
        bail!("{label} 응답 크기 제한({max_bytes} bytes)을 초과했습니다.");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("{label} 응답 본문 수신 실패"))?
    {
        if chunk.len() > max_bytes.saturating_sub(body.len()) {
            bail!("{label} 응답 크기 제한({max_bytes} bytes)을 초과했습니다.");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub(crate) async fn read_bounded_text(
    response: Response,
    max_bytes: usize,
    label: &str,
) -> Result<String> {
    let bytes = read_bounded_bytes(response, max_bytes, label).await?;
    Ok(match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
    })
}

pub(crate) async fn read_bounded_json<T: DeserializeOwned>(
    response: Response,
    max_bytes: usize,
    label: &str,
) -> Result<T> {
    let bytes = read_bounded_bytes(response, max_bytes, label).await?;
    serde_json::from_slice(&bytes).with_context(|| format!("{label} JSON 파싱 실패"))
}

pub mod chzzk;
pub mod kick;
pub mod live;
pub mod soop;
pub mod vod;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PlatformId {
    #[default]
    Soop,
    Chzzk,
    Kick,
}

impl PlatformId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Soop => "SOOP",
            Self::Chzzk => "CHZZK",
            Self::Kick => "KICK",
        }
    }

    pub const fn live_output_extension(self) -> &'static str {
        match self {
            Self::Soop => "ts",
            Self::Chzzk => "ts",
            Self::Kick => "ts",
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
            "KICK" => Ok(Self::Kick),
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
    fn display_name(&self) -> &'static str;
    fn capabilities(&self) -> PlatformCapabilities;
    fn validate_account(&self, account: &str) -> Result<()>;
    fn channel_lookup_request(&self, client: &Client, account: &str) -> RequestBuilder;
    fn parse_channel_name(&self, account: &str, value: &Value) -> Result<String>;
    fn profile_image_url(&self, account: &str, metadata: Option<&Value>) -> Result<Url>;
    fn accepts_vod_url(&self, url: &Url) -> bool;
}

pub fn provider(id: PlatformId) -> &'static dyn PlatformProvider {
    match id {
        PlatformId::Soop => &soop::SOOP,
        PlatformId::Chzzk => &chzzk::CHZZK,
        PlatformId::Kick => &kick::KICK,
    }
}

pub const fn default_platform() -> PlatformId {
    PlatformId::Soop
}

pub fn detect_vod_platform(raw_url: &str) -> Result<PlatformId> {
    let url = Url::parse(raw_url)?;
    for id in [PlatformId::Soop, PlatformId::Chzzk, PlatformId::Kick] {
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
        assert_eq!(PlatformId::Chzzk.live_output_extension(), "ts");
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
        assert_eq!(
            detect_vod_platform(
                "https://kick.com/example/videos/01a106d1-f328-750c-a31b-16a5df570460"
            )
            .unwrap(),
            PlatformId::Kick
        );
        assert!(detect_vod_platform("https://kick.com/example").is_err());
        assert!(detect_vod_platform("https://chzzk.naver.com/live/123456").is_err());
        assert!(detect_vod_platform("https://example.com/player/123456789").is_err());
    }

    #[tokio::test]
    async fn bounded_http_readers_enforce_body_size_limits() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        async fn serve_once(raw_response: String) -> String {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(raw_response.as_bytes()).await;
            });
            format!("http://{addr}")
        }

        let client = Client::builder().no_proxy().http1_only().build().unwrap();

        let ok_url = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}"
                .into(),
        )
        .await;
        let resp = client.get(&ok_url).send().await.unwrap();
        let parsed: Value = read_bounded_json(resp, 64, "test").await.unwrap();
        assert_eq!(parsed["ok"], Value::Bool(true));

        // Rejects oversized Content-Length fast.
        let big_cl_url = serve_once(format!(
            "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{}",
            "a".repeat(100)
        ))
        .await;
        let resp = client.get(&big_cl_url).send().await.unwrap();
        assert!(read_bounded_text(resp, 32, "test").await.is_err());

        // Rejects oversized streamed body even without Content-Length.
        let big_stream_url = serve_once(format!(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{}",
            "b".repeat(80)
        ))
        .await;
        let resp = client.get(&big_stream_url).send().await.unwrap();
        assert!(read_bounded_bytes(resp, 32, "test").await.is_err());
    }
}
