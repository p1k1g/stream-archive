//! External notification webhooks (Discord & Telegram) shared across GPUI and headless runtimes.
//!
//! Secret webhook URLs and bot tokens are decrypted only inside this service boundary
//! and are never written to runtime logs or surfaced in error messages.

use crate::{
    backend::LogBuffer, security::read_provider_settings, store::Store,
    support::platform::PlatformId,
};
use anyhow::{Result, bail};
use reqwest::Client;
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use url::Url;

const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONCURRENT_DELIVERIES: usize = 4;
const ALLOWED_DISCORD_HOSTS: &[&str] = &[
    "discord.com",
    "discordapp.com",
    "canary.discord.com",
    "ptb.discord.com",
];

#[derive(Debug, Clone)]
pub enum WebhookEvent {
    LiveStarted {
        platform: PlatformId,
        channel_name: String,
        account: String,
        title: String,
    },
    LiveFinished {
        platform: PlatformId,
        channel_name: String,
        account: String,
        title: String,
        duration_secs: i64,
        size_bytes: u64,
        reason: String,
    },
    LiveWarning {
        platform: PlatformId,
        channel_name: String,
        account: String,
        status: String,
        detail: String,
    },
    VodFinished {
        platform: PlatformId,
        title: String,
        completed: bool,
        detail: String,
    },
    Test,
}

impl WebhookEvent {
    fn tag(&self) -> &'static str {
        match self {
            Self::LiveStarted { .. } => "live_started",
            Self::LiveFinished { .. } => "live_finished",
            Self::LiveWarning { .. } => "live_warning",
            Self::VodFinished { .. } => "vod_finished",
            Self::Test => "test",
        }
    }

    fn setting_key(&self) -> Option<&'static str> {
        match self {
            Self::LiveStarted { .. } | Self::LiveFinished { .. } => Some("WEBHOOK_NOTIFY_LIVE"),
            Self::VodFinished { .. } => Some("WEBHOOK_NOTIFY_VOD"),
            Self::LiveWarning { .. } => Some("WEBHOOK_NOTIFY_WARNING"),
            Self::Test => None,
        }
    }

    fn format_message(&self) -> (String, String, u32) {
        match self {
            Self::LiveStarted {
                platform,
                channel_name,
                account,
                title,
            } => {
                let heading = format!("[LIVE 시작] {channel_name} ({platform})");
                let body = format!(
                    "채널: {channel_name} ({account})\n제목: {}",
                    if title.trim().is_empty() {
                        "-"
                    } else {
                        title.trim()
                    }
                );
                (heading, body, 0x22_C5_5E)
            }
            Self::LiveFinished {
                platform,
                channel_name,
                account,
                title,
                duration_secs,
                size_bytes,
                reason,
            } => {
                let heading = format!("[LIVE 종료] {channel_name} ({platform})");
                let body = format!(
                    "채널: {channel_name} ({account})\n제목: {}\n녹화 시간: {} · 크기: {}\n사유: {reason}",
                    if title.trim().is_empty() {
                        "-"
                    } else {
                        title.trim()
                    },
                    format_duration(*duration_secs),
                    format_size(*size_bytes),
                );
                (heading, body, 0x3B_82_F6)
            }
            Self::LiveWarning {
                platform,
                channel_name,
                account,
                status,
                detail,
            } => {
                let heading = format!("[LIVE 경고 · {status}] {channel_name} ({platform})");
                let body = format!(
                    "채널: {channel_name} ({account})\n상세: {}",
                    if detail.trim().is_empty() {
                        status.as_str()
                    } else {
                        detail.trim()
                    }
                );
                (heading, body, 0xEF_44_44)
            }
            Self::VodFinished {
                platform,
                title,
                completed,
                detail,
            } => {
                let state = if *completed { "완료" } else { "실패" };
                let color = if *completed { 0x10_B9_81 } else { 0xEF_44_44 };
                let heading = format!("[VOD {state}] ({platform})");
                let body = format!(
                    "제목: {}\n결과: {}",
                    if title.trim().is_empty() {
                        "-"
                    } else {
                        title.trim()
                    },
                    if detail.trim().is_empty() {
                        state
                    } else {
                        detail.trim()
                    }
                );
                (heading, body, color)
            }
            Self::Test => (
                "[Stream Archive] 웹훅 테스트 알림".into(),
                "Discord / Telegram 외부 알림 연동이 정상적으로 동작합니다.".into(),
                0x63_66_F1,
            ),
        }
    }
}

pub fn validate_discord_webhook_url(raw: &str) -> Result<()> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    if trimmed.len() > 512 || trimmed.contains(['\r', '\n', '\0']) {
        bail!("Discord Webhook URL 형식이 올바르지 않습니다.");
    }
    let url = Url::parse(trimmed)
        .map_err(|_| anyhow::anyhow!("Discord Webhook URL을 파싱할 수 없습니다."))?;
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let is_local_test = cfg!(test) && url.scheme() == "http" && host == "127.0.0.1";
    if !is_local_test
        && (url.scheme() != "https" || !ALLOWED_DISCORD_HOSTS.contains(&host.as_str()))
    {
        bail!("Discord Webhook URL은 https://discord.com/api/webhooks/... 형식이어야 합니다.");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("Discord Webhook URL에 자격증명 정보를 포함할 수 없습니다.");
    }
    let segments: Vec<&str> = url
        .path_segments()
        .map(|parts| parts.filter(|part| !part.is_empty()).collect())
        .unwrap_or_default();
    if segments.len() < 4
        || segments[0] != "api"
        || segments[1] != "webhooks"
        || !segments[2].chars().all(|c| c.is_ascii_digit())
        || segments[3].len() < 8
    {
        bail!("Discord Webhook URL 경로는 /api/webhooks/<ID>/<TOKEN> 형식이어야 합니다.");
    }
    Ok(())
}

pub fn validate_telegram_bot_token(raw: &str) -> Result<()> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    if trimmed.len() > 256 || trimmed.chars().any(char::is_whitespace) || trimmed.contains('\0') {
        bail!("Telegram Bot Token에 공백이나 제어 문자를 포함할 수 없습니다.");
    }
    let Some((bot_id, secret)) = trimmed.split_once(':') else {
        bail!("Telegram Bot Token은 <숫자ID>:<토큰> 형식이어야 합니다.");
    };
    if bot_id.is_empty()
        || !bot_id.chars().all(|c| c.is_ascii_digit())
        || secret.len() < 10
        || !secret
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("Telegram Bot Token 형식이 올바르지 않습니다.");
    }
    Ok(())
}

pub fn validate_telegram_chat_id(raw: &str) -> Result<()> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    if trimmed.len() > 128 || trimmed.chars().any(char::is_whitespace) || trimmed.contains('\0') {
        bail!("TELEGRAM_CHAT_ID에 공백이나 제어 문자를 포함할 수 없습니다.");
    }
    if let Some(username) = trimmed.strip_prefix('@') {
        if username.len() < 2
            || !username
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            bail!("TELEGRAM_CHAT_ID 채널명은 @영문/숫자/밑줄 형식이어야 합니다.");
        }
        return Ok(());
    }
    let digits = trimmed.strip_prefix('-').unwrap_or(trimmed);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        bail!("TELEGRAM_CHAT_ID는 숫자 ID(예: -1001234567890) 또는 @채널명이어야 합니다.");
    }
    Ok(())
}

#[derive(Clone)]
pub struct WebhookDispatcher {
    store: Store,
    logs: LogBuffer,
    semaphore: Arc<Semaphore>,
}

impl WebhookDispatcher {
    pub fn new(store: Store, logs: LogBuffer) -> Self {
        Self {
            store,
            logs,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_DELIVERIES)),
        }
    }

    /// Fire-and-forget notification. Fast-paths out without decrypting secrets when disabled.
    pub fn notify(&self, event: WebhookEvent) {
        let Ok(safe) = self.store.safe_settings() else {
            return;
        };
        if !safe
            .get("WEBHOOK_ENABLED")
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("Y"))
        {
            return;
        }
        if let Some(category_key) = event.setting_key()
            && safe
                .get(category_key)
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("N"))
        {
            return;
        }
        let Ok(permit) = self.semaphore.clone().try_acquire_owned() else {
            return;
        };
        let store = self.store.clone();
        let logs = self.logs.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = dispatch_with_store(&store, &logs, &event, false).await;
        });
    }

    /// Send a synchronous test notification to all configured webhook targets.
    pub async fn send_test(&self) -> Result<String> {
        let delivered =
            dispatch_with_store(&self.store, &self.logs, &WebhookEvent::Test, true).await?;
        if delivered.is_empty() {
            bail!(
                "설정된 웹훅 대상이 없습니다. Discord Webhook URL 또는 Telegram Bot Token + Chat ID를 먼저 저장하세요."
            );
        }
        Ok(format!(
            "외부 알림 웹훅 테스트 발송 성공 ({})",
            delivered.join(", ")
        ))
    }
}

fn webhook_client() -> Result<Client> {
    #[cfg(test)]
    {
        Ok(Client::builder()
            .timeout(WEBHOOK_TIMEOUT)
            .no_proxy()
            .http1_only()
            .pool_max_idle_per_host(0)
            .user_agent("Stream-Archive-Webhook/1.2")
            .build()?)
    }
    #[cfg(not(test))]
    {
        static CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();
        if let Some(client) = CLIENT.get() {
            return Ok(client.clone());
        }
        let built = Client::builder()
            .timeout(WEBHOOK_TIMEOUT)
            .no_proxy()
            .http1_only()
            .user_agent("Stream-Archive-Webhook/1.2")
            .build()?;
        let _ = CLIENT.set(built.clone());
        Ok(built)
    }
}

fn discord_payload(event: &WebhookEvent) -> serde_json::Value {
    let (title, description, color) = event.format_message();
    json!({
        "username": "Stream Archive",
        "embeds": [{
            "title": title,
            "description": description,
            "color": color,
        }]
    })
}

fn telegram_payload(chat_id: &str, event: &WebhookEvent) -> serde_json::Value {
    let (title, description, _) = event.format_message();
    json!({
        "chat_id": chat_id,
        "text": format!("{title}\n{description}"),
        "disable_web_page_preview": true
    })
}

async fn post_discord(client: &Client, webhook_url: &str, event: &WebhookEvent) -> Result<()> {
    let response = client
        .post(webhook_url)
        .json(&discord_payload(event))
        .send()
        .await
        .map_err(|err| anyhow::anyhow!("Discord 전송 오류: {}", sanitized_reqwest_error(&err)))?;
    let status = response.status();
    if !status.is_success() {
        bail!("Discord 응답 오류: HTTP {}", status.as_u16());
    }
    Ok(())
}

async fn post_telegram(
    client: &Client,
    api_base: &str,
    bot_token: &str,
    chat_id: &str,
    event: &WebhookEvent,
) -> Result<()> {
    let endpoint = format!(
        "{}/bot{bot_token}/sendMessage",
        api_base.trim_end_matches('/')
    );
    let response = client
        .post(&endpoint)
        .json(&telegram_payload(chat_id, event))
        .send()
        .await
        .map_err(|err| anyhow::anyhow!("Telegram 전송 오류: {}", sanitized_reqwest_error(&err)))?;
    let status = response.status();
    if !status.is_success() {
        bail!("Telegram 응답 오류: HTTP {}", status.as_u16());
    }
    Ok(())
}

fn sanitized_reqwest_error(err: &reqwest::Error) -> &'static str {
    if err.is_timeout() {
        "요청 시간 초과"
    } else if err.is_connect() {
        "서버 연결 실패"
    } else if err.is_status() {
        "HTTP 상태 오류"
    } else {
        "네트워크 오류"
    }
}

async fn dispatch_with_store(
    store: &Store,
    logs: &LogBuffer,
    event: &WebhookEvent,
    strict_errors: bool,
) -> Result<Vec<&'static str>> {
    let mut settings = read_provider_settings(
        store,
        &[
            "DISCORD_WEBHOOK_URL",
            "TELEGRAM_BOT_TOKEN",
            "TELEGRAM_CHAT_ID",
        ],
        &["DISCORD_WEBHOOK_URL", "TELEGRAM_BOT_TOKEN"],
    )?;
    let discord_url = settings
        .remove("DISCORD_WEBHOOK_URL")
        .unwrap_or_default()
        .trim()
        .to_string();
    let telegram_token = settings
        .remove("TELEGRAM_BOT_TOKEN")
        .unwrap_or_default()
        .trim()
        .to_string();
    let telegram_chat_id = settings
        .remove("TELEGRAM_CHAT_ID")
        .unwrap_or_default()
        .trim()
        .to_string();

    let client = webhook_client()?;
    let mut delivered = Vec::new();
    let mut errors = Vec::new();

    if !discord_url.is_empty() {
        match validate_discord_webhook_url(&discord_url) {
            Ok(()) => match post_discord(&client, &discord_url, event).await {
                Ok(()) => {
                    delivered.push("Discord");
                    logs.push(format!(
                        "[WEBHOOK] delivered target=discord event={}",
                        event.tag()
                    ))
                    .await;
                }
                Err(err) => {
                    logs.push(format!(
                        "[WEBHOOK:WARN] delivery failed target=discord event={} reason={err:#}",
                        event.tag()
                    ))
                    .await;
                    errors.push(format!("{err:#}"));
                }
            },
            Err(err) => {
                logs.push(format!(
                    "[WEBHOOK:WARN] invalid Discord webhook configuration: {err:#}"
                ))
                .await;
                errors.push(format!("{err:#}"));
            }
        }
    }

    if !telegram_token.is_empty() && !telegram_chat_id.is_empty() {
        match validate_telegram_bot_token(&telegram_token)
            .and_then(|()| validate_telegram_chat_id(&telegram_chat_id))
        {
            Ok(()) => {
                match post_telegram(
                    &client,
                    "https://api.telegram.org",
                    &telegram_token,
                    &telegram_chat_id,
                    event,
                )
                .await
                {
                    Ok(()) => {
                        delivered.push("Telegram");
                        logs.push(format!(
                            "[WEBHOOK] delivered target=telegram event={}",
                            event.tag()
                        ))
                        .await;
                    }
                    Err(err) => {
                        logs.push(format!(
                            "[WEBHOOK:WARN] delivery failed target=telegram event={} reason={err:#}",
                            event.tag()
                        ))
                        .await;
                        errors.push(format!("{err:#}"));
                    }
                }
            }
            Err(err) => {
                logs.push(format!(
                    "[WEBHOOK:WARN] invalid Telegram configuration: {err:#}"
                ))
                .await;
                errors.push(format!("{err:#}"));
            }
        }
    } else if strict_errors && (!telegram_token.is_empty() || !telegram_chat_id.is_empty()) {
        errors.push("Telegram 알림은 Bot Token과 Chat ID를 모두 입력해야 합니다.".into());
    }

    if strict_errors && !errors.is_empty() {
        bail!("{}", errors.join(" / "));
    }
    Ok(delivered)
}

fn format_duration(seconds: i64) -> String {
    let total = seconds.max(0) as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}시간 {m:02}분 {s:02}초")
    } else {
        format!("{m}분 {s:02}초")
    }
}

fn format_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    if bytes as f64 >= GB {
        format!("{:.2} GB", bytes as f64 / GB)
    } else {
        format!("{:.1} MB", bytes as f64 / MB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[test]
    fn validates_discord_and_telegram_inputs_strictly() {
        let valid_discord = format!(
            "https://{}/api/webhooks/{}/{}",
            "discord.com", "123456789012345678", "sample_token_value_1234"
        );
        validate_discord_webhook_url("").unwrap();
        validate_discord_webhook_url(&valid_discord).unwrap();
        assert!(
            validate_discord_webhook_url("https://example.com/api/webhooks/1/abcdefgh").is_err()
        );
        assert!(
            validate_discord_webhook_url("http://discord.com/api/webhooks/1/abcdefgh").is_err()
        );
        assert!(validate_discord_webhook_url("https://discord.com/channels/1/2").is_err());

        let valid_tg = format!("{}:{}", "123456789", "ABCdef1234567890_xyz");
        validate_telegram_bot_token("").unwrap();
        validate_telegram_bot_token(&valid_tg).unwrap();
        assert!(validate_telegram_bot_token("not-a-token").is_err());
        assert!(validate_telegram_bot_token("abc:short").is_err());

        validate_telegram_chat_id("").unwrap();
        validate_telegram_chat_id("123456789").unwrap();
        validate_telegram_chat_id("-1001234567890").unwrap();
        validate_telegram_chat_id("@my_stream_alerts").unwrap();
        assert!(validate_telegram_chat_id("invalid chat").is_err());
        assert!(validate_telegram_chat_id("@").is_err());
    }

    async fn spawn_capture_server(status: u16) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                let mut expected_total: Option<usize> = None;
                loop {
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..n]);
                    if expected_total.is_none()
                        && let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n")
                    {
                        let headers = String::from_utf8_lossy(&data[..pos]);
                        let content_len = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())
                                    .flatten()
                            })
                            .unwrap_or(0);
                        expected_total = Some(pos + 4 + content_len);
                    }
                    if let Some(total) = expected_total
                        && data.len() >= total
                    {
                        break;
                    }
                }
                let captured = String::from_utf8_lossy(&data).into_owned();
                let _ = tx.send(captured);
                let resp = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            }
        });
        (format!("http://{addr}"), rx)
    }

    #[tokio::test]
    async fn discord_and_telegram_posts_format_payloads_and_redact_errors() {
        let client = webhook_client().unwrap();

        let (base_ok, rx_discord) = spawn_capture_server(204).await;
        let url = format!("{base_ok}/api/webhooks/123456/secret_token_9999");
        post_discord(
            &client,
            &url,
            &WebhookEvent::LiveStarted {
                platform: PlatformId::Soop,
                channel_name: "테스트채널".into(),
                account: "tester".into(),
                title: "방송 제목".into(),
            },
        )
        .await
        .unwrap();
        let req = rx_discord.await.unwrap();
        assert!(req.contains("\"username\":\"Stream Archive\""));
        assert!(req.contains("[LIVE 시작]"));

        let (base_tg, rx_tg) = spawn_capture_server(200).await;
        let token = format!("{}:{}", "987654321", "SecretBotToken_12345");
        post_telegram(
            &client,
            &base_tg,
            &token,
            "-100999888",
            &WebhookEvent::VodFinished {
                platform: PlatformId::Chzzk,
                title: "다시보기".into(),
                completed: true,
                detail: "완료".into(),
            },
        )
        .await
        .unwrap();
        let req_tg = rx_tg.await.unwrap();
        assert!(req_tg.contains("\"-100999888\""));
        assert!(req_tg.contains("[VOD 완료]"));

        // Non-2xx error must never leak the bot token or webhook URL
        let (base_err, _rx_err) = spawn_capture_server(401).await;
        let err = post_telegram(
            &client,
            &base_err,
            &token,
            "-100999888",
            &WebhookEvent::Test,
        )
        .await
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("401"));
        assert!(!msg.contains(&token));
    }
}
