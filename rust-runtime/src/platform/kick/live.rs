use super::{
    super::{PlatformProvider, live::StreamInput},
    KICK,
};
use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::time::Duration;
use url::Url;

#[derive(Debug, Clone)]
pub struct KickBroadcast {
    pub live_id: String,
    pub channel_name: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone)]
pub enum KickProbe {
    Offline,
    Live(KickBroadcast),
}

pub struct KickLiveSession {
    client: Client,
}
impl KickLiveSession {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
    pub async fn probe(&self, account: &str) -> Result<KickProbe> {
        KICK.validate_account(account)?;
        let response = KICK
            .channel_lookup_request(&self.client, account)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .context("KICK LIVE 정보 요청 실패")?;
        check_http_status(response.status())?;
        let mut response = response;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len().saturating_add(chunk.len()) > 1024 * 1024 {
                bail!("KICK LIVE 응답 크기 제한을 초과했습니다.");
            }
            body.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&body).context("KICK LIVE 응답 JSON을 읽지 못했습니다.")?;
        parse_probe(account, &value)
    }
    pub fn resolve_stream(&self, account: &str, _live: &KickBroadcast) -> Result<StreamInput> {
        KICK.validate_account(account)?;
        Ok(StreamInput::PluginUrl {
            url: format!("https://kick.com/{}", account.trim().to_ascii_lowercase()),
            cookies: Vec::new(),
            start_at_zero: false,
        })
    }
    pub fn recover_auth(&self) -> Result<String> {
        bail!(
            "KICK 공개 LIVE만 지원합니다. API 403/JS challenge는 오프라인이 아닙니다. Streamlink과 Chromium 계열 브라우저 설치 상태를 확인하세요."
        );
    }
}

fn check_http_status(status: StatusCode) -> Result<()> {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => bail!(
            "KICK API 접근이 거부되었습니다(HTTP {status}). 로그인 제한 또는 JS challenge일 수 있습니다. 오프라인으로 판단하지 않습니다."
        ),
        StatusCode::TOO_MANY_REQUESTS => {
            bail!("KICK API 요청 제한(HTTP 429). 감시 간격을 늘리고 잠시 후 다시 확인하세요.")
        }
        StatusCode::NOT_FOUND => bail!("KICK 채널을 찾지 못했습니다. 채널 ID(slug)를 확인하세요."),
        code if !code.is_success() => bail!("KICK LIVE API HTTP 오류: {code}"),
        _ => Ok(()),
    }
}

fn parse_probe(account: &str, value: &Value) -> Result<KickProbe> {
    let channel_name = KICK.parse_channel_name(account, value)?;
    let live = value
        .get("livestream")
        .context("KICK LIVE 응답에 livestream 상태가 없습니다.")?;
    if live.is_null() {
        return Ok(KickProbe::Offline);
    }
    let live_id = live
        .get("id")
        .and_then(|id| id.as_u64().filter(|id| *id > 0).map(|id| id.to_string()))
        .context("KICK LIVE 응답에 유효한 방송 ID가 없습니다.")?;
    let title = live
        .get("session_title")
        .and_then(Value::as_str)
        .context("KICK LIVE 응답에 방송 제목이 없습니다.")?
        .to_owned();
    let thumbnail_url = live
        .pointer("/thumbnail/url")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned);
    Ok(KickProbe::Live(KickBroadcast {
        live_id,
        channel_name,
        title,
        thumbnail_url,
    }))
}

pub(crate) fn valid_thumbnail_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && matches!(url.host_str(), Some("images.kick.com" | "files.kick.com"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn channel(live: Value) -> Value {
        serde_json::json!({"slug":"fixture","user":{"username":"방송자"},"livestream":live})
    }
    #[test]
    fn offline_requires_an_explicit_null_on_a_matching_channel() {
        assert!(matches!(
            parse_probe("fixture", &channel(Value::Null)).unwrap(),
            KickProbe::Offline
        ));
        assert!(parse_probe("other", &channel(Value::Null)).is_err());
        assert!(
            parse_probe(
                "fixture",
                &serde_json::json!({"slug":"fixture","user":{"username":"방송자"}})
            )
            .is_err()
        );
        assert!(parse_probe("fixture", &channel(serde_json::json!({}))).is_err());
    }
    #[test]
    fn broadcast_metadata_and_plugin_input_are_provider_owned() {
        let value = channel(
            serde_json::json!({"id":123,"session_title":"테스트 LIVE","thumbnail":{"url":"https://images.kick.com/video.jpg"}}),
        );
        let KickProbe::Live(live) = parse_probe("fixture", &value).unwrap() else {
            panic!("live");
        };
        assert_eq!(live.live_id, "123");
        assert_eq!(live.channel_name, "방송자");
        assert_eq!(live.title, "테스트 LIVE");
        assert_eq!(
            live.thumbnail_url.as_deref(),
            Some("https://images.kick.com/video.jpg")
        );
        let session = KickLiveSession::new(Client::new());
        let StreamInput::PluginUrl {
            url,
            cookies,
            start_at_zero,
        } = session.resolve_stream("FIXTURE", &live).unwrap()
        else {
            panic!("plugin URL");
        };
        assert_eq!(url, "https://kick.com/fixture");
        assert!(cookies.is_empty());
        assert!(!start_at_zero);
    }
    #[test]
    fn access_errors_are_not_offline_and_image_hosts_are_restricted() {
        for code in [401, 403, 404, 429, 500] {
            assert!(check_http_status(StatusCode::from_u16(code).unwrap()).is_err());
        }
        assert!(check_http_status(StatusCode::OK).is_ok());
        for url in [
            "https://images.kick.com/a.jpg",
            "https://files.kick.com/a.webp",
        ] {
            assert!(valid_thumbnail_url(&Url::parse(url).unwrap()));
        }
        for url in [
            "http://images.kick.com/a",
            "https://images.kick.com.evil.test/a",
            "https://user:secret@images.kick.com/a",
            "https://localhost/a",
            "https://files.kick.com:8443/a",
        ] {
            assert!(!valid_thumbnail_url(&Url::parse(url).unwrap()));
        }
    }
}
