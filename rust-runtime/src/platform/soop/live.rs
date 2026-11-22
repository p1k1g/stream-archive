use super::signed_proxy::{
    PrivateAuthError, SOOP_PRIVATE_AUTH_URL, SoopSignedAuth, request_subscription_signed_cookies,
    validate_soop_cdn_url,
};
use anyhow::{Result, anyhow, bail};
use regex::Regex;
use reqwest::{
    Client, Response,
    header::{COOKIE, SET_COOKIE},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use url::Url;

#[derive(Debug, Clone)]
pub struct SoopBroadcast {
    pub bno: String,
    pub bj_nick: String,
    pub title: String,
    pub rmd: String,
    pub bpwd: String,
    pub cdn: String,
    pub permittable_subscription_tier: u8,
    pub my_subscription_tier: u8,
}

#[derive(Debug, Clone)]
pub enum SoopProbe {
    Offline,
    AuthRequired,
    Live(SoopBroadcast),
}

#[derive(Debug, Clone)]
pub struct SoopResolvedStream {
    pub quality: String,
    pub cdn: String,
    pub host: String,
    pub playlist_url: String,
    pub signed_auth: Option<SoopSignedAuth>,
}

pub struct SoopLiveSession {
    client: Client,
    cookies: BTreeMap<String, String>,
    bno_regex: Regex,
}

impl SoopLiveSession {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            cookies: BTreeMap::new(),
            bno_regex: Regex::new(r"window\.nBroadNo\s*=\s*(\d+);").unwrap(),
        }
    }

    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn worker_cookie_header(&self) -> String {
        const ALLOW: &[&str] = &[
            "AuthTicket",
            "BbsTicket",
            "UserTicket",
            "BbsSaveTicket",
            "RDB",
            "PdboxTicket",
            "PdboxBbs",
            "PdboxUser",
            "PdboxSaveTicket",
        ];
        self.cookies
            .iter()
            .filter(|(key, _)| ALLOW.iter().any(|item| key.eq_ignore_ascii_case(item)))
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub async fn login(&mut self, username: &str, password: &str) -> Result<String> {
        if username.trim().is_empty() || password.trim().is_empty() {
            bail!("SOOP username/password is empty");
        }
        self.cookies.clear();
        let response = self
            .client
            .post("https://login.sooplive.com/app/LoginAction.php")
            .header("Referer", "https://www.sooplive.com/")
            .form(&[
                ("szWork", "login"),
                ("szType", "json"),
                ("szUid", username),
                ("szPassword", password),
                ("isSaveId", "true"),
                ("isSavePw", "false"),
                ("isSaveJoin", "false"),
                ("isLoginRetain", "Y"),
            ])
            .send()
            .await?;
        collect_cookies(&mut self.cookies, &response);
        let value: Value = response.error_for_status()?.json().await?;
        if value.get("RESULT").and_then(Value::as_i64) != Some(1) {
            bail!("SOOP login failed RESULT={:?}", value.get("RESULT"));
        }

        let response = self
            .client
            .get("https://afevent2.sooplive.com/api/get_private_info.php")
            .header("Referer", "https://www.sooplive.com/")
            .header(COOKIE, self.cookie_header())
            .send()
            .await?;
        collect_cookies(&mut self.cookies, &response);
        let auth: Value = response.error_for_status()?.json().await?;
        let login_id = auth
            .pointer("/CHANNEL/LOGIN_ID")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if login_id.is_empty() {
            bail!("SOOP login verification failed");
        }
        Ok(login_id)
    }

    pub async fn probe(&self, account: &str) -> Result<SoopProbe> {
        let channel_url = format!("https://play.sooplive.com/{account}");
        let cookie = self.cookie_header();
        let mut request = self
            .client
            .get(&channel_url)
            .header("Referer", "https://play.sooplive.com/");
        if !cookie.is_empty() {
            request = request.header(COOKIE, cookie.clone());
        }
        let html = request.send().await?.error_for_status()?.text().await?;
        let Some(captures) = self.bno_regex.captures(&html) else {
            return Ok(SoopProbe::Offline);
        };
        let bno = captures.get(1).unwrap().as_str().to_string();

        let mut request = self
            .client
            .post("https://live.sooplive.com/afreeca/player_live_api.php")
            .header("Referer", &channel_url)
            .form(&[
                ("from_api", "0"),
                ("mode", "landing"),
                ("player_type", "html5"),
                ("stream_type", "common"),
                ("type", "live"),
                ("bid", account),
                ("bno", bno.as_str()),
                ("pwd", ""),
            ]);
        if !cookie.is_empty() {
            request = request.header(COOKIE, cookie);
        }
        let value: Value = request.send().await?.error_for_status()?.json().await?;
        let Some(channel) = value.get("CHANNEL") else {
            return Ok(SoopProbe::Offline);
        };
        match channel.get("RESULT").and_then(Value::as_i64) {
            Some(-6) => return Ok(SoopProbe::AuthRequired),
            Some(-14) => {
                bail!(
                    "구독(또는 구독플러스) 전용 방송입니다. SOOP 계정의 구독 상태를 확인하세요 (RESULT=-14)."
                )
            }
            Some(1) => {}
            _ => return Ok(SoopProbe::Offline),
        }

        let get = |key: &str| {
            channel
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let rmd = get("RMD");
        let api_bno = get("BNO");
        if rmd.is_empty() || api_bno.is_empty() {
            return Ok(SoopProbe::Offline);
        }
        Ok(SoopProbe::Live(SoopBroadcast {
            bno: api_bno,
            bj_nick: get("BJNICK"),
            title: get("TITLE"),
            rmd,
            bpwd: get("BPWD"),
            cdn: get("CDN"),
            permittable_subscription_tier: json_u8(channel.get("P_MIN_TIER")),
            my_subscription_tier: json_u8(channel.get("TIER_TYPE")),
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn resolve_stream(
        &self,
        account: &str,
        live: &SoopBroadcast,
        worker_url: &str,
        worker_api_key: &str,
        max_retries: usize,
        stream_password: &str,
    ) -> Result<SoopResolvedStream> {
        let attempts = max_retries.max(1);
        let mut last_error = String::new();
        let is_subscription = live.permittable_subscription_tier > 0;
        for attempt in 1..=attempts {
            let cookies = self.worker_cookie_header();
            let body = json!({
                "account": account,
                "bno": live.bno,
                "rmd": live.rmd,
                "quality": "master",
                "cq": "sd",
                "password": stream_password,
                "cookie": cookies,
                "bid": account,
                "bpwd": stream_password,
                "channel_url": format!("https://play.sooplive.com/{account}"),
                "soop_cookie_header": self.worker_cookie_header(),
                "subscribe": is_subscription,
                "permittable_subscription_tier": live.permittable_subscription_tier,
            });
            match self
                .client
                .post(worker_url)
                .header("X-API-Key", worker_api_key)
                .json(&body)
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    let text = response.text().await.unwrap_or_default();
                    if status.is_success()
                        && let Ok(value) = serde_json::from_str::<Value>(&text)
                        && value.get("success").and_then(Value::as_bool) == Some(true)
                    {
                        let playlist_url = value
                            .get("playlist_url")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        if !playlist_url.is_empty() {
                            let worker_stream = SoopResolvedStream {
                                quality: value
                                    .get("quality")
                                    .and_then(Value::as_str)
                                    .unwrap_or("master")
                                    .to_string(),
                                cdn: value
                                    .get("cdn")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                host: value
                                    .get("host")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                playlist_url,
                                signed_auth: None,
                            };
                            if !is_subscription {
                                return Ok(worker_stream);
                            }
                            let aid = extract_query_param(&worker_stream.playlist_url, "aid")
                                .ok_or_else(|| {
                                    anyhow!("Worker playlist URL에 SOOP aid 토큰이 없습니다.")
                                })?;
                            return self
                                .resolve_subscription_stream(account, live, &worker_stream, &aid)
                                .await;
                        }
                    }
                    last_error = format!("Worker HTTP {status}: {}", compact(&text, 240));
                }
                Err(err) => last_error = format!("Worker transport error: {err}"),
            }
            if attempt < attempts {
                let delay = [2, 5, 10][(attempt - 1).min(2)];
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        }
        bail!("{last_error}")
    }

    async fn resolve_subscription_stream(
        &self,
        account: &str,
        live: &SoopBroadcast,
        worker_stream: &SoopResolvedStream,
        aid: &str,
    ) -> Result<SoopResolvedStream> {
        let login_cookie_header = self.cookie_header();
        if login_cookie_header.is_empty() {
            bail!("SOOP 구독(또는 구독플러스) 방송은 로그인 세션이 필요합니다.");
        }
        let rmd_url = validate_soop_cdn_url(&live.rmd)?;
        let quality = if worker_stream.quality.trim().is_empty() {
            "master"
        } else {
            worker_stream.quality.as_str()
        };
        let referer = format!("https://play.sooplive.com/{account}");
        let candidates = subscribe_cdn_candidates(&live.cdn);
        let mut errors = Vec::new();

        for cdn in candidates {
            let playlist_url = if worker_stream.cdn.eq_ignore_ascii_case(&cdn)
                && is_subscribe_cdn_host(&worker_stream.host)
                && validate_soop_cdn_url(&worker_stream.playlist_url).is_ok()
            {
                worker_stream.playlist_url.clone()
            } else {
                match self
                    .assign_subscribe_playlist_url(
                        &rmd_url,
                        account,
                        &live.bno,
                        quality,
                        &cdn,
                        aid,
                        &login_cookie_header,
                    )
                    .await
                {
                    Ok(url) => url,
                    Err(err) => {
                        errors.push(format!("{cdn}: assign failed ({err})"));
                        continue;
                    }
                }
            };

            let host = match validate_soop_cdn_url(&playlist_url) {
                Ok(parsed) => parsed.host_str().unwrap_or("").to_string(),
                Err(err) => {
                    errors.push(format!("{cdn}: invalid CDN URL ({err})"));
                    continue;
                }
            };

            let signed = match request_subscription_signed_cookies(
                &self.client,
                SOOP_PRIVATE_AUTH_URL,
                &login_cookie_header,
                account,
                &live.bno,
                &playlist_url,
            )
            .await
            {
                Ok(result) => result,
                Err(err @ PrivateAuthError::Denied { .. }) => {
                    return Err(anyhow!("{err}"));
                }
                Err(PrivateAuthError::Other(err)) => {
                    errors.push(format!("{cdn} ({host}): private_auth failed ({err})"));
                    continue;
                }
            };

            match self
                .client
                .get(&playlist_url)
                .header("Referer", &referer)
                .header("Origin", "https://play.sooplive.com")
                .header(COOKIE, &signed.cookie_header)
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let body = resp.text().await.unwrap_or_default();
                        if body.contains("#EXTM3U") {
                            return Ok(SoopResolvedStream {
                                quality: quality.to_string(),
                                cdn,
                                host,
                                playlist_url: playlist_url.clone(),
                                signed_auth: Some(SoopSignedAuth {
                                    account: account.to_string(),
                                    bno: live.bno.clone(),
                                    playlist_url,
                                    login_cookie_header,
                                    initial_signed_cookies: signed.cookie_header,
                                    refresh_interval: signed.refresh_interval,
                                }),
                            });
                        }
                        errors.push(format!("{cdn} ({host}): non-M3U8 HTTP {status}"));
                    } else {
                        errors.push(format!("{cdn} ({host}): HTTP {status}"));
                    }
                }
                Err(err) => {
                    errors.push(format!("{cdn} ({host}): playlist probe error ({err})"));
                }
            }
        }

        bail!(
            "SOOP 구독(티어 {}) 스트림을 확인할 수 없습니다: {}",
            live.permittable_subscription_tier,
            errors.join(" | ")
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn assign_subscribe_playlist_url(
        &self,
        rmd_url: &Url,
        account: &str,
        bno: &str,
        quality: &str,
        cdn: &str,
        aid: &str,
        login_cookie_header: &str,
    ) -> Result<String> {
        let mut assign_url = rmd_url.clone();
        assign_url.set_path("/broad_stream_assign.html");
        assign_url
            .query_pairs_mut()
            .clear()
            .append_pair("return_type", cdn)
            .append_pair("broad_key", &format!("{bno}-common-{quality}-hls"))
            .append_pair("use_cors", "true")
            .append_pair("cors_origin_url", "play.sooplive.com")
            .append_pair("player_mode", "landing");

        let resp = self
            .client
            .get(assign_url)
            .header("Referer", format!("https://play.sooplive.com/{account}"))
            .header("Origin", "https://play.sooplive.com")
            .header(COOKIE, login_cookie_header)
            .send()
            .await?
            .error_for_status()?;
        let value: Value = resp.json().await?;
        let view_url = value
            .get("view_url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if view_url.is_empty() {
            bail!("assign response missing view_url");
        }
        let mut playlist_url = validate_soop_cdn_url(view_url)?;
        playlist_url.query_pairs_mut().append_pair("aid", aid);
        Ok(playlist_url.to_string())
    }
}

fn json_u8(value: Option<&Value>) -> u8 {
    match value {
        Some(Value::Number(num)) => num.as_u64().and_then(|v| u8::try_from(v).ok()).unwrap_or(0),
        Some(Value::String(text)) => text.trim().parse::<u8>().unwrap_or(0),
        _ => 0,
    }
}

fn extract_query_param(raw_url: &str, key: &str) -> Option<String> {
    let parsed = Url::parse(raw_url).ok()?;
    parsed
        .query_pairs()
        .find_map(|(k, v)| (k == key && !v.trim().is_empty()).then(|| v.into_owned()))
}

fn is_subscribe_cdn_host(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();
    lower.contains("-sub-") || lower.contains("pc-web-sub")
}

fn map_cdn_to_subscribe(raw_cdn: &str) -> Option<String> {
    let normalized = raw_cdn.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.ends_with("_subscribe") {
        return Some(normalized);
    }
    if normalized.contains("lg_cdn") {
        return Some("lg_cdn_pc_web_subscribe".to_string());
    }
    if normalized.contains("gs_cdn") {
        return Some("gs_cdn_pc_web_subscribe".to_string());
    }
    if normalized.contains("gcp_cdn") {
        return Some("gcp_cdn_subscribe".to_string());
    }
    if normalized.contains("azure_cdn") {
        return Some("azure_cdn_subscribe".to_string());
    }
    if normalized.contains("aws_cf") {
        return Some("aws_cf_subscribe".to_string());
    }
    None
}

fn subscribe_cdn_candidates(preferred_cdn: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut push_unique = |item: String| {
        if !out
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&item))
        {
            out.push(item);
        }
    };
    if let Some(mapped) = map_cdn_to_subscribe(preferred_cdn) {
        push_unique(mapped);
    }
    for fallback in [
        "lg_cdn_pc_web_subscribe",
        "gcp_cdn_subscribe",
        "azure_cdn_subscribe",
        "aws_cf_subscribe",
        "gs_cdn_pc_web_subscribe",
    ] {
        push_unique(fallback.to_string());
    }
    out
}

pub fn is_password_protected(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_uppercase().as_str(),
        "Y" | "1" | "TRUE"
    )
}

fn collect_cookies(target: &mut BTreeMap<String, String>, response: &Response) {
    for value in response.headers().get_all(SET_COOKIE).iter() {
        if let Ok(text) = value.to_str()
            && let Some(pair) = text.split(';').next()
            && let Some((name, value)) = pair.split_once('=')
        {
            target.insert(name.trim().to_string(), value.trim().to_string());
        }
    }
}

fn compact(text: &str, max: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() > max {
        format!("{}...", compact.chars().take(max).collect::<String>())
    } else {
        compact
    }
}

/// The public player uses this broadcast-scoped snapshot path.
pub(crate) fn thumbnail_url(bno: &str) -> Option<String> {
    (!bno.is_empty() && bno.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| format!("https://liveimg.sooplive.com/m/{bno}"))
}

pub(crate) fn valid_thumbnail_url(url: &url::Url) -> bool {
    url.host_str() == Some("liveimg.sooplive.com")
        && url
            .path()
            .strip_prefix("/m/")
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod thumbnail_tests {
    use super::*;
    #[test]
    fn snapshot_is_broadcast_scoped_and_rejects_path_injection() {
        assert_eq!(
            thumbnail_url("123").as_deref(),
            Some("https://liveimg.sooplive.com/m/123")
        );
        for id in ["", "../123", "123?a=b", "https://evil.test"] {
            assert!(thumbnail_url(id).is_none());
        }
    }

    #[test]
    fn parses_subscription_tier_from_string_or_number() {
        assert_eq!(json_u8(Some(&json!("2"))), 2);
        assert_eq!(json_u8(Some(&json!(2))), 2);
        assert_eq!(json_u8(Some(&json!("0"))), 0);
        assert_eq!(json_u8(Some(&json!(0))), 0);
        assert_eq!(json_u8(Some(&json!(null))), 0);
        assert_eq!(json_u8(Some(&json!("invalid"))), 0);
        assert_eq!(json_u8(None), 0);
    }

    #[test]
    fn maps_and_orders_subscribe_cdn_candidates_without_duplicates() {
        let kr_lg = subscribe_cdn_candidates("lg_cdn");
        assert_eq!(kr_lg[0], "lg_cdn_pc_web_subscribe");
        assert_eq!(kr_lg.len(), 5);

        let overseas_gcp = subscribe_cdn_candidates("gcp_cdn");
        assert_eq!(overseas_gcp[0], "gcp_cdn_subscribe");
        assert_eq!(overseas_gcp[1], "lg_cdn_pc_web_subscribe");
        assert_eq!(overseas_gcp.len(), 5);

        let kr_gs = subscribe_cdn_candidates("gs_cdn");
        assert_eq!(kr_gs[0], "gs_cdn_pc_web_subscribe");
        assert_eq!(kr_gs[1], "lg_cdn_pc_web_subscribe");
        assert_eq!(kr_gs.len(), 5);

        let already_sub = subscribe_cdn_candidates("gcp_cdn_subscribe");
        assert_eq!(already_sub[0], "gcp_cdn_subscribe");
        assert_eq!(already_sub.len(), 5);

        let empty = subscribe_cdn_candidates("");
        assert_eq!(empty[0], "lg_cdn_pc_web_subscribe");
        assert_eq!(empty.len(), 5);
    }

    #[test]
    fn extracts_aid_query_param_and_detects_subscribe_hosts() {
        assert_eq!(
            extract_query_param(
                "https://live-global-cdn-v02.sooplive.com/live-stm-13/auth_master_playlist.m3u8?aid=.A32.abc&x=1",
                "aid"
            )
            .as_deref(),
            Some(".A32.abc")
        );
        assert!(extract_query_param("https://example.test/a.m3u8", "aid").is_none());
        assert!(is_subscribe_cdn_host(
            "live-sub-global-cdn-v02.sooplive.com"
        ));
        assert!(is_subscribe_cdn_host(
            "live-pcweb-sub-kr-cdn-z02.sooplive.com"
        ));
        assert!(is_subscribe_cdn_host("pc-web-sub.stream.sooplive.com"));
        assert!(!is_subscribe_cdn_host("live-global-cdn-v02.sooplive.com"));
    }
}
