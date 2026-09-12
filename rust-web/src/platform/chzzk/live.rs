use super::{
    super::live::StreamInput,
    auth::{ChzzkAuth, ChzzkAuthState},
};
use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode, header::COOKIE};
use serde_json::Value;

const LIVE_DETAIL_URL: &str =
    "https://api.chzzk.naver.com/service/v2/channels/{channel_id}/live-detail";

#[derive(Debug, Clone)]
pub struct ChzzkBroadcast {
    pub live_id: String,
    pub channel_name: String,
    pub title: String,
    pub adult: bool,
    pub requires_auth: bool,
}

#[derive(Debug, Clone)]
pub enum ChzzkProbe {
    Offline,
    AuthRequired,
    Live(ChzzkBroadcast),
}

#[derive(Debug, Clone)]
pub struct ChzzkResolvedStream {
    pub quality: String,
    pub input: StreamInput,
}

pub struct ChzzkLiveSession {
    client: Client,
}

impl ChzzkLiveSession {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn probe(&self, channel_id: &str) -> Result<ChzzkProbe> {
        let auth = ChzzkAuth::load()?;
        let url = LIVE_DETAIL_URL.replace("{channel_id}", channel_id.trim());
        let mut request = self.client.get(url);
        if let Some(cookie) = auth.cookie_header() {
            request = request.header(COOKIE, cookie);
        }
        let response = request.send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(ChzzkProbe::Offline);
        }
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return auth_failure(&auth);
        }
        let value: Value = response
            .error_for_status()
            .context("CHZZK live-detail HTTP 오류")?
            .json()
            .await?;
        parse_probe_content(channel_id, &value, &auth)
    }

    pub async fn resolve_stream(
        &self,
        channel_id: &str,
        live: &ChzzkBroadcast,
    ) -> Result<ChzzkResolvedStream> {
        let auth = ChzzkAuth::load()?;
        resolve_stream_with_auth(channel_id, live, &auth)
    }
}

fn parse_probe_content(channel_id: &str, value: &Value, auth: &ChzzkAuth) -> Result<ChzzkProbe> {
    if value.get("code").and_then(Value::as_i64) != Some(200) {
        let code = value
            .get("code")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown response");
        bail!("CHZZK live-detail API 오류: code={code} message={message}");
    }
    let Some(content) = value.get("content").filter(|value| !value.is_null()) else {
        return Ok(ChzzkProbe::Offline);
    };
    if content.get("status").and_then(Value::as_str) != Some("OPEN") {
        return Ok(ChzzkProbe::Offline);
    }

    let live_id = content
        .get("liveId")
        .and_then(|value| {
            value
                .as_i64()
                .map(|number| number.to_string())
                .or_else(|| value.as_str().map(str::to_string))
        })
        .filter(|value| !value.is_empty())
        .context("CHZZK LIVE response has no liveId")?;
    let channel_name = content
        .pointer("/channel/channelName")
        .and_then(Value::as_str)
        .unwrap_or(channel_id)
        .trim()
        .to_string();
    let title = content
        .get("liveTitle")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let adult = content
        .get("adult")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let membership_only = content
        .get("membershipBenefitType")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("MEMBER_ONLY"));
    let requires_auth = adult || membership_only;
    let playback_available = content.get("livePlaybackJson").is_some_and(|value| {
        !value.is_null() && value.as_str().is_some_and(|text| !text.is_empty())
    });

    if !playback_available {
        if requires_auth {
            return auth_failure(auth);
        }
        bail!("CHZZK 방송은 OPEN 상태이지만 재생 정보를 받지 못했습니다.");
    }

    Ok(ChzzkProbe::Live(ChzzkBroadcast {
        live_id,
        channel_name,
        title,
        adult,
        requires_auth,
    }))
}

fn resolve_stream_with_auth(
    channel_id: &str,
    live: &ChzzkBroadcast,
    auth: &ChzzkAuth,
) -> Result<ChzzkResolvedStream> {
    if live.requires_auth && !auth.configured() {
        match auth.state() {
            ChzzkAuthState::Partial => {
                bail!("CHZZK NID_AUT/NID_SES 중 하나만 설정되어 있습니다.")
            }
            ChzzkAuthState::Missing => {
                bail!("이 CHZZK 제한 방송에는 NID_AUT/NID_SES 인증이 필요합니다.")
            }
            ChzzkAuthState::Configured => unreachable!(),
        }
    }
    Ok(ChzzkResolvedStream {
        quality: "best".into(),
        input: StreamInput::PluginUrl {
            url: format!("https://chzzk.naver.com/live/{}", channel_id.trim()),
            cookies: if live.requires_auth {
                auth.streamlink_cookies()
            } else {
                Vec::new()
            },
        },
    })
}

fn auth_failure(auth: &ChzzkAuth) -> Result<ChzzkProbe> {
    match auth.state() {
        ChzzkAuthState::Missing => Ok(ChzzkProbe::AuthRequired),
        ChzzkAuthState::Partial => {
            bail!(
                "CHZZK NID_AUT/NID_SES 중 하나만 설정되어 있습니다. 두 값을 모두 다시 저장하세요."
            )
        }
        ChzzkAuthState::Configured => {
            bail!(
                "CHZZK 인증 쿠키가 만료되었거나 이 제한 방송을 재생할 권한이 없습니다. NID_AUT/NID_SES를 확인하세요."
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHANNEL: &str = "0123456789abcdef0123456789abcdef";

    fn open_live(adult: bool, membership: Option<&str>, playback: Option<&str>) -> Value {
        serde_json::json!({
            "code": 200,
            "content": {
                "status": "OPEN",
                "liveId": 123,
                "liveTitle": "테스트 방송",
                "adult": adult,
                "membershipBenefitType": membership,
                "livePlaybackJson": playback,
                "channel": {"channelName": "테스트 채널"}
            }
        })
    }

    #[test]
    fn auth_failure_distinguishes_missing_partial_and_invalid() {
        assert!(matches!(
            auth_failure(&ChzzkAuth::from_plain("", "")).unwrap(),
            ChzzkProbe::AuthRequired
        ));
        let partial = auth_failure(&ChzzkAuth::from_plain("aut", ""))
            .unwrap_err()
            .to_string();
        assert!(partial.contains("하나만 설정"));
        let invalid = auth_failure(&ChzzkAuth::from_plain("aut", "ses"))
            .unwrap_err()
            .to_string();
        assert!(invalid.contains("만료되었거나"));
    }

    #[test]
    fn public_live_does_not_require_auth() {
        let probe = parse_probe_content(
            CHANNEL,
            &open_live(false, None, Some("{}")),
            &ChzzkAuth::from_plain("", ""),
        )
        .unwrap();
        let ChzzkProbe::Live(live) = probe else {
            panic!("expected live probe");
        };
        assert!(!live.requires_auth);
        assert!(!live.adult);
    }

    #[test]
    fn adult_and_membership_live_require_auth_when_playback_is_hidden() {
        for value in [
            open_live(true, None, None),
            open_live(false, Some("MEMBER_ONLY"), None),
        ] {
            assert!(matches!(
                parse_probe_content(CHANNEL, &value, &ChzzkAuth::from_plain("", "")).unwrap(),
                ChzzkProbe::AuthRequired
            ));
        }
    }

    #[test]
    fn configured_restricted_live_passes_cookies_to_streamlink() {
        let auth = ChzzkAuth::from_plain("aut", "ses");
        let probe =
            parse_probe_content(CHANNEL, &open_live(true, None, Some("{}")), &auth).unwrap();
        let ChzzkProbe::Live(live) = probe else {
            panic!("expected live probe");
        };
        let resolved = resolve_stream_with_auth(CHANNEL, &live, &auth).unwrap();
        let StreamInput::PluginUrl { cookies, .. } = resolved.input else {
            panic!("expected plugin URL");
        };
        assert_eq!(cookies.len(), 2);
    }

    #[test]
    fn public_live_omits_auth_cookies_from_streamlink() {
        let auth = ChzzkAuth::from_plain("aut", "ses");
        let probe =
            parse_probe_content(CHANNEL, &open_live(false, None, Some("{}")), &auth).unwrap();
        let ChzzkProbe::Live(live) = probe else {
            panic!("expected live probe");
        };
        let resolved = resolve_stream_with_auth(CHANNEL, &live, &auth).unwrap();
        let StreamInput::PluginUrl { cookies, .. } = resolved.input else {
            panic!("expected plugin URL");
        };
        assert!(cookies.is_empty());
    }

    #[test]
    fn open_live_without_playback_is_not_misreported_as_offline() {
        let err = parse_probe_content(
            CHANNEL,
            &open_live(false, None, None),
            &ChzzkAuth::from_plain("", ""),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("재생 정보를 받지 못했습니다"));
    }
}
