use super::{
    super::{DEFAULT_API_BODY_LIMIT, live::StreamInput, read_bounded_json},
    auth::{ChzzkAuth, ChzzkAuthState},
};
use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode, header::COOKIE};
use serde_json::Value;

const LIVE_STATUS_URL: &str =
    "https://api.chzzk.naver.com/polling/v2/channels/{channel_id}/live-status";
const LIVE_DETAIL_URL: &str =
    "https://api.chzzk.naver.com/service/v2/channels/{channel_id}/live-detail";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveStatusDecision {
    Offline,
    Open,
    FallbackToDetail,
}

#[derive(Debug, Clone)]
pub struct ChzzkBroadcast {
    pub live_id: String,
    pub channel_name: String,
    pub title: String,
    pub requires_auth: bool,
    pub thumbnail_url: Option<String>,
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
        self.probe_with_templates(channel_id, &auth, LIVE_STATUS_URL, LIVE_DETAIL_URL)
            .await
    }

    async fn probe_with_templates(
        &self,
        channel_id: &str,
        auth: &ChzzkAuth,
        status_url_template: &str,
        detail_url_template: &str,
    ) -> Result<ChzzkProbe> {
        let trimmed_id = channel_id.trim();
        let cookie = auth.cookie_header();

        // Stage 1: query the lightweight polling/live-status endpoint first so offline
        // channels do not repeatedly hit the heavier live-detail endpoint (which can
        // return spurious HTTP 500 errors for offline/closed channels).
        let status_url = status_url_template.replace("{channel_id}", trimmed_id);
        let mut status_req = self.client.get(status_url);
        if let Some(cookie) = cookie.as_deref() {
            status_req = status_req.header(COOKIE, cookie);
        }
        if let Ok(status_resp) = status_req.send().await {
            let http_status = status_resp.status();
            if http_status == StatusCode::NOT_FOUND {
                return Ok(ChzzkProbe::Offline);
            }
            if matches!(
                http_status,
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ) {
                return auth_failure(auth);
            }
            if http_status.is_success()
                && let Ok(value) = read_bounded_json::<Value>(
                    status_resp,
                    DEFAULT_API_BODY_LIMIT,
                    "CHZZK live-status",
                )
                .await
            {
                match parse_live_status_content(&value) {
                    LiveStatusDecision::Offline => return Ok(ChzzkProbe::Offline),
                    LiveStatusDecision::Open | LiveStatusDecision::FallbackToDetail => {}
                }
            }
        }

        // Stage 2: fetch full live-detail metadata when live-status reports OPEN
        // (or fallback if live-status returned 5xx / unexpected payload).
        let detail_url = detail_url_template.replace("{channel_id}", trimmed_id);
        let mut request = self.client.get(detail_url);
        if let Some(cookie) = cookie.as_deref() {
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
            return auth_failure(auth);
        }
        let value: Value = read_bounded_json(
            response
                .error_for_status()
                .context("CHZZK live-detail HTTP 오류")?,
            DEFAULT_API_BODY_LIMIT,
            "CHZZK live-detail",
        )
        .await?;
        parse_probe_content(channel_id, &value, auth)
    }

    pub fn recover_auth(&self) -> Result<String> {
        bail!("CHZZK 제한 방송 인증은 설정 > CHZZK 인증에서 NID_AUT/NID_SES를 모두 저장하세요.")
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

fn parse_live_status_content(value: &Value) -> LiveStatusDecision {
    if value.get("code").and_then(Value::as_i64) != Some(200) {
        return LiveStatusDecision::FallbackToDetail;
    }
    let Some(content) = value.get("content") else {
        return LiveStatusDecision::FallbackToDetail;
    };
    if content.is_null() {
        return LiveStatusDecision::Offline;
    }
    match content.get("status").and_then(Value::as_str) {
        Some("OPEN") => LiveStatusDecision::Open,
        Some(_) => LiveStatusDecision::Offline,
        None => LiveStatusDecision::FallbackToDetail,
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
        requires_auth,
        thumbnail_url: content
            .get("liveImageUrl")
            .and_then(Value::as_str)
            .filter(|url| !url.trim().is_empty())
            .or_else(|| {
                content
                    .get("defaultThumbnailImageUrl")
                    .and_then(Value::as_str)
                    .filter(|url| !url.trim().is_empty())
            })
            .map(|url| url.replace("{type}", "270")),
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
            // CHZZK's Streamlink plugin preserves source PTS with copyts. Reset only the
            // presentation origin so a recording started mid-broadcast begins at 00:00.
            start_at_zero: true,
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

pub(crate) fn valid_thumbnail_url(url: &url::Url) -> bool {
    match url.host_str() {
        Some("livecloud-thumb.akamaized.net") => url.path().starts_with("/chzzk/"),
        Some("nng-phinf.pstatic.net") => true,
        _ => false,
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
    fn auth_recovery_guidance_is_owned_by_chzzk_provider() {
        let session = ChzzkLiveSession::new(Client::new());
        let message = session.recover_auth().unwrap_err().to_string();
        assert!(message.contains("CHZZK 인증"));
        assert!(message.contains("NID_AUT/NID_SES"));
    }

    #[test]
    fn thumbnail_uses_existing_probe_metadata_and_handles_null() {
        let mut value = open_live(false, None, Some("{}"));
        let auth = ChzzkAuth::from_plain("", "");
        let ChzzkProbe::Live(live) = parse_probe_content(CHANNEL, &value, &auth).unwrap() else {
            panic!("live");
        };
        assert!(live.thumbnail_url.is_none());
        value["content"]["defaultThumbnailImageUrl"] =
            "https://nng-phinf.pstatic.net/broadcast.jpg".into();
        let ChzzkProbe::Live(live) = parse_probe_content(CHANNEL, &value, &auth).unwrap() else {
            panic!("live");
        };
        assert_eq!(
            live.thumbnail_url.as_deref(),
            Some("https://nng-phinf.pstatic.net/broadcast.jpg")
        );
        value["content"]["liveImageUrl"] =
            "https://livecloud-thumb.akamaized.net/chzzk/live/123/image_{type}.jpg".into();
        let ChzzkProbe::Live(live) = parse_probe_content(CHANNEL, &value, &auth).unwrap() else {
            panic!("live");
        };
        assert_eq!(
            live.thumbnail_url.as_deref(),
            Some("https://livecloud-thumb.akamaized.net/chzzk/live/123/image_270.jpg")
        );
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
        assert!(!live.requires_auth);
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
        let StreamInput::PluginUrl {
            cookies,
            start_at_zero,
            ..
        } = resolved.input
        else {
            panic!("expected plugin URL");
        };
        assert_eq!(cookies.len(), 2);
        assert!(start_at_zero);
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
        let StreamInput::PluginUrl {
            cookies,
            start_at_zero,
            ..
        } = resolved.input
        else {
            panic!("expected plugin URL");
        };
        assert!(cookies.is_empty());
        assert!(start_at_zero);
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

    #[test]
    fn parse_live_status_distinguishes_offline_open_and_fallback() {
        assert_eq!(
            parse_live_status_content(&serde_json::json!({
                "code": 200,
                "content": null
            })),
            LiveStatusDecision::Offline
        );
        assert_eq!(
            parse_live_status_content(&serde_json::json!({
                "code": 200,
                "content": { "status": "CLOSE" }
            })),
            LiveStatusDecision::Offline
        );
        assert_eq!(
            parse_live_status_content(&serde_json::json!({
                "code": 200,
                "content": { "status": "OPEN" }
            })),
            LiveStatusDecision::Open
        );
        assert_eq!(
            parse_live_status_content(&serde_json::json!({
                "code": 500,
                "message": "internal"
            })),
            LiveStatusDecision::FallbackToDetail
        );
        assert_eq!(
            parse_live_status_content(&serde_json::json!({
                "code": 200,
                "content": {}
            })),
            LiveStatusDecision::FallbackToDetail
        );
    }

    async fn spawn_mock_chzzk_server(
        status_code: u16,
        status_body: &'static str,
        detail_code: u16,
        detail_body: &'static str,
    ) -> (
        String,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let detail_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hits_clone = detail_hits.clone();
        let handle = tokio::spawn(async move {
            for _ in 0..4 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let mut req_bytes = Vec::new();
                let mut chunk = [0u8; 1024];
                while !req_bytes.windows(4).any(|w| w == b"\r\n\r\n") && req_bytes.len() < 8192 {
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    req_bytes.extend_from_slice(&chunk[..n]);
                }
                let req = String::from_utf8_lossy(&req_bytes);
                let first_line = req.lines().next().unwrap_or("");
                let (code, body) = if first_line.contains("/live-detail") {
                    hits_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    (detail_code, detail_body)
                } else {
                    (status_code, status_body)
                };
                let resp = format!(
                    "HTTP/1.1 {code} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        (format!("http://{addr}"), detail_hits, handle)
    }

    #[tokio::test]
    async fn two_stage_probe_skips_live_detail_when_live_status_is_closed() {
        let (base, detail_hits, handle) = spawn_mock_chzzk_server(
            200,
            r#"{"code":200,"content":{"status":"CLOSE"}}"#,
            500,
            r#"{"code":500}"#,
        )
        .await;
        let session = ChzzkLiveSession::new(Client::builder().no_proxy().build().unwrap());
        let probe = session
            .probe_with_templates(
                CHANNEL,
                &ChzzkAuth::from_plain("", ""),
                &format!("{base}/polling/v2/channels/{{channel_id}}/live-status"),
                &format!("{base}/service/v2/channels/{{channel_id}}/live-detail"),
            )
            .await
            .unwrap();
        assert!(matches!(probe, ChzzkProbe::Offline));
        assert_eq!(detail_hits.load(std::sync::atomic::Ordering::SeqCst), 0);
        handle.abort();
    }

    #[tokio::test]
    async fn two_stage_probe_fetches_detail_when_open_and_falls_back_on_status_500() {
        let detail_json = r#"{"code":200,"content":{"status":"OPEN","liveId":456,"liveTitle":"방송 중","adult":false,"livePlaybackJson":"{}","channel":{"channelName":"테스트"}}}"#;
        let (base, detail_hits, handle) = spawn_mock_chzzk_server(
            200,
            r#"{"code":200,"content":{"status":"OPEN"}}"#,
            200,
            detail_json,
        )
        .await;
        let session = ChzzkLiveSession::new(Client::builder().no_proxy().build().unwrap());
        let probe = session
            .probe_with_templates(
                CHANNEL,
                &ChzzkAuth::from_plain("", ""),
                &format!("{base}/polling/v2/channels/{{channel_id}}/live-status"),
                &format!("{base}/service/v2/channels/{{channel_id}}/live-detail"),
            )
            .await
            .unwrap();
        let ChzzkProbe::Live(live) = probe else {
            panic!("expected live");
        };
        assert_eq!(live.live_id, "456");
        assert_eq!(detail_hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        handle.abort();

        let (base2, detail_hits2, handle2) =
            spawn_mock_chzzk_server(500, r#"{"code":500}"#, 200, detail_json).await;
        let probe2 = session
            .probe_with_templates(
                CHANNEL,
                &ChzzkAuth::from_plain("", ""),
                &format!("{base2}/polling/v2/channels/{{channel_id}}/live-status"),
                &format!("{base2}/service/v2/channels/{{channel_id}}/live-detail"),
            )
            .await
            .unwrap();
        assert!(matches!(probe2, ChzzkProbe::Live(_)));
        assert_eq!(detail_hits2.load(std::sync::atomic::Ordering::SeqCst), 1);
        handle2.abort();
    }
}
