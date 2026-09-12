use super::super::live::{HttpCookie, StreamInput};
use anyhow::{Context, Result, bail};
use reqwest::{Client, header::COOKIE};
use serde_json::Value;

const LIVE_DETAIL_URL: &str =
    "https://api.chzzk.naver.com/service/v2/channels/{channel_id}/live-detail";

#[derive(Debug, Clone)]
pub struct ChzzkBroadcast {
    pub live_id: String,
    pub channel_name: String,
    pub title: String,
    pub adult: bool,
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
    nid_aut: String,
    nid_ses: String,
}

impl ChzzkLiveSession {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            nid_aut: String::new(),
            nid_ses: String::new(),
        }
    }

    pub fn set_auth(&mut self, nid_aut: &str, nid_ses: &str) {
        self.nid_aut = nid_aut.trim().to_string();
        self.nid_ses = nid_ses.trim().to_string();
    }

    pub fn auth_configured(&self) -> bool {
        !self.nid_aut.is_empty() && !self.nid_ses.is_empty()
    }

    fn cookie_header(&self) -> Option<String> {
        if self.nid_aut.is_empty() && self.nid_ses.is_empty() {
            return None;
        }
        let mut values = Vec::new();
        if !self.nid_aut.is_empty() {
            values.push(format!("NID_AUT={}", self.nid_aut));
        }
        if !self.nid_ses.is_empty() {
            values.push(format!("NID_SES={}", self.nid_ses));
        }
        Some(values.join("; "))
    }

    pub async fn probe(&self, channel_id: &str) -> Result<ChzzkProbe> {
        let url = LIVE_DETAIL_URL.replace("{channel_id}", channel_id.trim());
        let mut request = self.client.get(url);
        if let Some(cookie) = self.cookie_header() {
            request = request.header(COOKIE, cookie);
        }
        let response = request.send().await?;
        if response.status().as_u16() == 404 {
            return Ok(ChzzkProbe::Offline);
        }
        let value: Value = response.error_for_status()?.json().await?;
        if value.get("code").and_then(Value::as_i64) != Some(200) {
            return Ok(ChzzkProbe::Offline);
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
        let playback_available = content.get("livePlaybackJson").is_some_and(|value| {
            !value.is_null() && value.as_str().is_some_and(|text| !text.is_empty())
        });

        if adult && !playback_available {
            return Ok(ChzzkProbe::AuthRequired);
        }

        Ok(ChzzkProbe::Live(ChzzkBroadcast {
            live_id,
            channel_name,
            title,
            adult,
        }))
    }

    pub async fn resolve_stream(
        &self,
        channel_id: &str,
        live: &ChzzkBroadcast,
    ) -> Result<ChzzkResolvedStream> {
        if live.adult && !self.auth_configured() {
            bail!("CHZZK 연령 제한 방송에는 NID_AUT/NID_SES 인증이 필요합니다.");
        }
        let mut cookies = Vec::new();
        if !self.nid_aut.is_empty() {
            cookies.push(HttpCookie {
                domain: ".naver.com".into(),
                name: "NID_AUT".into(),
                value: self.nid_aut.clone(),
                secure: true,
            });
        }
        if !self.nid_ses.is_empty() {
            cookies.push(HttpCookie {
                domain: ".naver.com".into(),
                name: "NID_SES".into(),
                value: self.nid_ses.clone(),
                secure: true,
            });
        }
        Ok(ChzzkResolvedStream {
            quality: "best".into(),
            input: StreamInput::PluginUrl {
                url: format!("https://chzzk.naver.com/live/{}", channel_id.trim()),
                cookies,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_is_configured_only_when_both_cookies_exist() {
        let client = Client::new();
        let mut session = ChzzkLiveSession::new(client);
        assert!(!session.auth_configured());
        session.set_auth("aut", "");
        assert!(!session.auth_configured());
        session.set_auth("aut", "ses");
        assert!(session.auth_configured());
    }
}
