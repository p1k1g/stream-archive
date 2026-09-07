use crate::backend::LogBuffer;
use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde_json::Value;
use std::{
    collections::HashMap,
    env, fs,
    path::PathBuf,
    time::Duration,
};
use sysinfo::System;

pub async fn resolve_channel_name(account: &str) -> Result<String> {
    let account = account.trim();
    if account.is_empty() {
        bail!("계정 ID가 비어 있습니다.");
    }
    if !account
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    {
        bail!("계정 ID 형식이 올바르지 않습니다.");
    }

    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36")
        .timeout(Duration::from_secs(10))
        .no_proxy()
        .http1_only()
        .build()
        .context("SOOP 조회용 HTTP client 생성 실패")?;

    let response = client
        .get("https://st.sooplive.com/api/get_station_status.php")
        .query(&[("szBjId", account)])
        .send()
        .await
        .context("SOOP 채널 정보 요청 실패")?
        .error_for_status()
        .context("SOOP 채널 정보 HTTP 오류")?;

    let value: Value = response
        .json()
        .await
        .context("SOOP 채널 정보 JSON 파싱 실패")?;

    let result = value.get("RESULT").and_then(Value::as_i64).unwrap_or(0);
    if result == 0 {
        bail!("유효한 SOOP 계정을 찾지 못했습니다: {account}");
    }

    // Current SOOP response shape:
    // { "RESULT": 1, "DATA": { "user_id": "...", "user_nick": "..." } }
    // Older consumers have also observed top-level station_name, so keep that as a fallback.
    let returned_id = value
        .pointer("/DATA/user_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();

    if !returned_id.is_empty() && !returned_id.eq_ignore_ascii_case(account) {
        bail!("SOOP 응답 계정이 요청과 다릅니다: 요청={account}, 응답={returned_id}");
    }

    let name = value
        .pointer("/DATA/user_nick")
        .and_then(Value::as_str)
        .or_else(|| value.get("station_name").and_then(Value::as_str))
        .unwrap_or("")
        .trim();

    if name.is_empty() {
        bail!("채널 닉네임을 찾지 못했습니다: {account}");
    }

    Ok(name.to_string())
}

pub fn find_legacy_watcher() -> Option<String> {
    let mut system = System::new_all();
    system.refresh_all();

    for (pid, process) in system.processes() {
        let command = process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        if command.to_ascii_lowercase().contains("soop_live.ps1") {
            return Some(format!("pid={pid} {}", command));
        }
    }
    None
}

pub fn spawn_streamlink_log_bridge(logs: LogBuffer) {
    tokio::spawn(async move {
        let temp_dir = env::temp_dir();
        let mut offsets: HashMap<PathBuf, usize> = HashMap::new();

        loop {
            if let Ok(entries) = fs::read_dir(&temp_dir) {
                let mut present = Vec::new();
                for entry in entries.flatten() {
                    let path = entry.path();
                    let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
                        continue;
                    };
                    if !name.starts_with("soop_streamlink_rust_") || !name.ends_with("stderr.log") {
                        continue;
                    }
                    present.push(path.clone());

                    let Ok(bytes) = fs::read(&path) else {
                        continue;
                    };
                    let offset = offsets.entry(path.clone()).or_insert(0);
                    if bytes.len() <= *offset {
                        continue;
                    }

                    let chunk = String::from_utf8_lossy(&bytes[*offset..]).to_string();
                    *offset = bytes.len();
                    for line in chunk.lines().map(str::trim).filter(|line| !line.is_empty()) {
                        logs.push(format!("[RUST:STREAMLINK] {line}")).await;
                    }
                }
                offsets.retain(|path, _| present.contains(path));
            }

            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nickname_from_value(value: &Value) -> Option<&str> {
        value
            .pointer("/DATA/user_nick")
            .and_then(Value::as_str)
            .or_else(|| value.get("station_name").and_then(Value::as_str))
    }

    #[test]
    fn parses_current_station_status_nickname_shape() {
        let value = serde_json::json!({
            "RESULT": 1,
            "DATA": {
                "user_id": "1004ysus",
                "user_nick": "테스트닉"
            }
        });
        assert_eq!(nickname_from_value(&value), Some("테스트닉"));
    }

    #[test]
    fn parses_legacy_station_name_fallback() {
        let value = serde_json::json!({
            "RESULT": 1,
            "station_name": "구형닉"
        });
        assert_eq!(nickname_from_value(&value), Some("구형닉"));
    }
}
