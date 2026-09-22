use anyhow::{Result, bail};
use regex::Regex;
use reqwest::{
    Client, Response,
    header::{COOKIE, SET_COOKIE},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};

#[derive(Debug, Clone)]
pub struct SoopBroadcast {
    pub bno: String,
    pub bj_nick: String,
    pub title: String,
    pub rmd: String,
    pub bpwd: String,
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
        if channel.get("RESULT").and_then(Value::as_i64) == Some(-6) {
            return Ok(SoopProbe::AuthRequired);
        }
        if channel.get("RESULT").and_then(Value::as_i64) != Some(1) {
            return Ok(SoopProbe::Offline);
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
                "soop_cookie_header": self.worker_cookie_header()
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
                            return Ok(SoopResolvedStream {
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
                            });
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
