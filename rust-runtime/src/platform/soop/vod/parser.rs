use super::{VodEntry, VodMetadata};
use crate::model::{VodAnalysisView, VodPartInfo, VodQualityOption};
use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use regex::Regex;
use reqwest::StatusCode;
use serde_json::Value;
use std::path::{Path, PathBuf};
use url::Url;

pub(super) fn parse_metadata(info: &Value) -> Result<VodMetadata> {
    // Current yt-dlp returns a video object for one PART, a playlist for many.
    let entries = match info.get("entries") {
        Some(Value::Array(entries)) => entries.iter().collect::<Vec<_>>(),
        Some(_) => bail!("yt-dlp VOD PART 목록 형식이 올바르지 않습니다."),
        None if !entry_url(info).is_empty() => vec![info],
        None => bail!("yt-dlp 결과에 재생 가능한 VOD PART가 없습니다."),
    };
    if entries.is_empty() {
        bail!("VOD PART를 찾지 못했습니다.");
    }
    let streamer_id = str_field(info, "uploader_id")
        .or_else(|| entries.first().and_then(|v| str_field(v, "uploader_id")))
        .ok_or_else(|| anyhow!("VOD BJ ID를 찾지 못했습니다."))?;
    let streamer = str_field(info, "uploader")
        .or_else(|| entries.first().and_then(|v| str_field(v, "uploader")))
        .unwrap_or_else(|| streamer_id.clone());
    let title = str_field(info, "title")
        .or_else(|| entries.first().and_then(|v| str_field(v, "title")))
        .unwrap_or_else(|| "UNKNOWN".into());
    let raw_date = str_field(info, "upload_date")
        .or_else(|| entries.first().and_then(|v| str_field(v, "upload_date")))
        .unwrap_or_else(|| Utc::now().format("%Y%m%d").to_string());
    let date = if raw_date.len() == 8 && raw_date.chars().all(|c| c.is_ascii_digit()) {
        raw_date[2..].to_string()
    } else {
        Utc::now().format("%y%m%d").to_string()
    };
    let parsed = entries
        .iter()
        .map(|entry| VodEntry {
            manifest_url: entry_url(entry),
            duration_seconds: entry_duration(entry),
        })
        .collect();
    Ok(VodMetadata {
        title,
        thumbnail_url: extractor_thumbnail(info)
            .or_else(|| entries.iter().find_map(|entry| extractor_thumbnail(entry))),
        streamer,
        streamer_id,
        date,
        entries: parsed,
        requires_private_auth: extractor_requires_private_auth(info)
            || entries
                .iter()
                .any(|entry| extractor_requires_private_auth(entry)),
    })
}

pub(super) fn extractor_requires_private_auth(value: &Value) -> bool {
    value.get("protocol").and_then(Value::as_str) == Some("soopvod")
        || value.get("_cookie_refresh_params").is_some()
        || ["formats", "requested_formats"].iter().any(|key| {
            value
                .get(*key)
                .and_then(Value::as_array)
                .is_some_and(|formats| formats.iter().any(extractor_requires_private_auth))
        })
}

pub(super) fn extractor_thumbnail(value: &Value) -> Option<String> {
    str_field(value, "thumbnail").or_else(|| {
        value
            .get("thumbnails")?
            .as_array()?
            .iter()
            .rev()
            .find_map(|item| str_field(item, "url"))
    })
}

pub(super) fn complete_api_result(
    mut metadata: VodMetadata,
    response: Result<Value>,
) -> Result<VodMetadata> {
    let usable_urls = metadata
        .entries
        .iter()
        .all(|entry| !entry.manifest_url.is_empty());
    match response {
        Ok(value) => {
            metadata.requires_private_auth |= value.get("data").is_some_and(|data| {
                str_field(data, "sub_upload_type").is_some_and(|kind| !kind.is_empty())
            });
            match parse_api_metadata(&value) {
                Ok(api) => Ok(complete_api_metadata(metadata, api)),
                Err(_) if usable_urls && !api_access_denied(&value) => Ok(metadata),
                Err(error) => Err(error),
            }
        }
        Err(error)
            if usable_urls
                && !error.chain().any(|cause| {
                    cause.downcast_ref::<reqwest::Error>().is_some_and(|error| {
                        error.status().is_some_and(|status| {
                            status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN
                        })
                    })
                }) =>
        {
            // Duration enrichment is optional. Manifest probing and signed-cookie
            // validation still run before analysis or download can proceed.
            Ok(metadata)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn api_access_denied(value: &Value) -> bool {
    let Some(data) = value.get("data") else {
        return false;
    };
    let no_files = data
        .get("files")
        .and_then(Value::as_array)
        .is_none_or(|files| files.is_empty());
    matches!(api_integer(data.get("code")), Some(-6221 | -6205))
        || api_integer(data.get("subscribed_view")).is_some_and(|code| code < 0)
        || (no_files
            && (data.get("adult_status").and_then(Value::as_str) == Some("notLogin")
                || str_field(data, "sub_upload_type").is_some_and(|kind| !kind.is_empty())))
}

pub(super) fn complete_api_metadata(mut metadata: VodMetadata, api: VodMetadata) -> VodMetadata {
    // An authorized API response is authoritative if the PART list changed.
    // Otherwise retain fresh extractor URLs and fill only absent fields.
    if metadata.entries.len() != api.entries.len() || metadata.streamer_id != api.streamer_id {
        return api;
    }
    for (entry, api_entry) in metadata.entries.iter_mut().zip(api.entries) {
        if entry.manifest_url.is_empty() {
            entry.manifest_url = api_entry.manifest_url;
        }
        if entry.duration_seconds == 0 {
            entry.duration_seconds = api_entry.duration_seconds;
        }
    }
    metadata.requires_private_auth |= api.requires_private_auth;
    if metadata.thumbnail_url.is_none() {
        metadata.thumbnail_url = api.thumbnail_url;
    }
    metadata
}

pub(super) fn parse_api_metadata(value: &Value) -> Result<VodMetadata> {
    let data = value
        .get("data")
        .ok_or_else(|| anyhow!("SOOP VOD API 응답에 data가 없습니다."))?;
    match api_integer(data.get("code")) {
        Some(-6221) => bail!("삭제되었거나 존재하지 않는 SOOP VOD입니다."),
        Some(-6205) => bail!("비공개 SOOP VOD입니다. 현재 계정의 접근 권한을 확인해 주세요."),
        Some(code) if code < 0 => bail!("SOOP VOD 접근 실패 code={code}"),
        _ => {}
    }
    if api_integer(data.get("subscribed_view")).is_some_and(|code| code < 0) {
        bail!("구독 VOD 접근 권한이 없습니다. 앱의 SOOP 계정이 구독한 계정인지 확인해 주세요.");
    }
    if api_integer(value.get("result")) != Some(1) {
        bail!(
            "SOOP VOD API 요청 실패: {}",
            redact(&data.get("message").map(value_as_string).unwrap_or_default())
        );
    }
    let files = data
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("SOOP VOD API 응답에 PART 목록이 없습니다."))?;
    if files.is_empty() {
        if data.get("adult_status").and_then(Value::as_str) == Some("notLogin") {
            bail!("성인 인증된 SOOP 계정으로 로그인해야 볼 수 있는 VOD입니다.");
        }
        if str_field(data, "sub_upload_type").is_some_and(|value| !value.is_empty()) {
            bail!("구독 VOD 접근 권한이 없습니다. 앱의 SOOP 로그인 계정을 확인해 주세요.");
        }
        bail!("SOOP VOD API에서 재생 가능한 PART를 찾지 못했습니다.");
    }
    let entries = files
        .iter()
        .map(|file| {
            let manifest_url = api_file_url(file);
            if !valid_media_url(&manifest_url) {
                bail!("SOOP VOD PART 영상 주소가 없거나 올바르지 않습니다.");
            }
            Ok(VodEntry {
                manifest_url,
                // SOOP API durations are milliseconds; yt-dlp durations are seconds.
                duration_seconds: (entry_duration(file) as f64 / 1000.0).round() as u64,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let streamer_id = str_field(data, "bj_id")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("SOOP VOD API 응답에 BJ ID가 없습니다."))?;
    let date = api_integer(data.get("write_timestamp"))
        .and_then(|stamp| chrono::DateTime::from_timestamp(stamp, 0))
        .map(|date| {
            date.with_timezone(&chrono::FixedOffset::east_opt(9 * 3600).unwrap())
                .format("%y%m%d")
                .to_string()
        })
        .unwrap_or_else(|| Utc::now().format("%y%m%d").to_string());
    Ok(VodMetadata {
        title: str_field(data, "title").unwrap_or_else(|| "UNKNOWN".into()),
        thumbnail_url: str_field(data, "thumb"),
        streamer: str_field(data, "writer_nick").unwrap_or_else(|| streamer_id.clone()),
        streamer_id,
        date,
        entries,
        requires_private_auth: str_field(data, "sub_upload_type").is_some_and(|s| !s.is_empty()),
    })
}

pub(super) fn api_integer(value: Option<&Value>) -> Option<i64> {
    value.and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
}

pub(super) fn analysis_view(
    url: &str,
    metadata: &VodMetadata,
    qualities: Vec<VodQualityOption>,
) -> VodAnalysisView {
    VodAnalysisView {
        vod_url: url.into(),
        thumbnail_url: metadata.thumbnail_url.clone(),
        title: metadata.title.clone(),
        streamer: metadata.streamer.clone(),
        streamer_id: metadata.streamer_id.clone(),
        part_count: metadata.entries.len(),
        qualities,
        parts: metadata
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| VodPartInfo {
                part: i + 1,
                duration_seconds: e.duration_seconds,
            })
            .collect(),
    }
}

pub(super) fn resolve_parts(requested: &[usize], count: usize) -> Result<Vec<usize>> {
    if count == 0 {
        bail!("VOD PART를 찾지 못했습니다.");
    }
    let mut result = if requested.is_empty() {
        (1..=count).collect()
    } else {
        requested.to_vec()
    };
    result.sort_unstable();
    result.dedup();
    if result.iter().any(|part| *part < 1 || *part > count) {
        bail!("PART는 1~{count} 범위여야 합니다.");
    }
    Ok(result)
}

pub(super) fn validate_url(value: &str) -> Result<()> {
    static PLAYER_PATH_RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"/player/\d+").unwrap());
    let url = Url::parse(value).context("올바른 SOOP VOD URL이 아닙니다.")?;
    if url.scheme() != "https"
        || !url.host_str().unwrap_or("").ends_with("sooplive.com")
        || !PLAYER_PATH_RE.is_match(url.path())
    {
        bail!("올바른 SOOP VOD HTTPS URL이 아닙니다.");
    }
    Ok(())
}

pub(super) fn validate_retries(value: u32) -> Result<()> {
    if !(1..=20).contains(&value) {
        bail!("MAX_RETRY must be between 1 and 20.");
    }
    Ok(())
}

pub(super) fn title_no(url: &str) -> Result<String> {
    static TITLE_NO_RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"/player/(\d+)").unwrap());
    TITLE_NO_RE
        .captures(url)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| anyhow!("VOD title_no를 찾지 못했습니다."))
}

pub(super) fn entry_url(value: &Value) -> String {
    let formats = ["formats", "requested_formats"]
        .into_iter()
        .filter_map(|key| value.get(key).and_then(Value::as_array))
        .flatten()
        .collect::<Vec<_>>();
    // A selected top-level rendition can coexist with a nested master. Search
    // every manifest field before falling back to any selected/direct URL.
    for candidate in std::iter::once(value).chain(formats.iter().copied()) {
        for key in ["manifest_url", "manifestUrl", "hls_url", "hlsUrl"] {
            if let Some(s) = candidate.get(key).and_then(Value::as_str)
                && valid_media_url(s)
            {
                return s.into();
            }
        }
    }
    for candidate in std::iter::once(value).chain(formats) {
        if let Some(s) = candidate.get("url").and_then(Value::as_str)
            && valid_media_url(s)
        {
            return s.into();
        }
    }
    String::new()
}

pub(super) fn valid_media_url(value: &str) -> bool {
    Url::parse(value).ok().is_some_and(|url| {
        (url.scheme() == "https"
            || (cfg!(test) && url.scheme() == "http" && url.host_str() == Some("127.0.0.1")))
            && !url.path().starts_with("/player/")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

pub(super) fn api_file_url(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        return s.into();
    }
    for key in ["file", "url", "file_url", "fileUrl"] {
        if let Some(s) = value.get(key).and_then(Value::as_str) {
            return s.into();
        }
    }
    String::new()
}

pub(super) fn entry_duration(value: &Value) -> u64 {
    for key in ["duration", "file_duration", "play_time"] {
        if let Some(number) = value.get(key).and_then(Value::as_f64)
            && number > 0.0
        {
            return number.round() as u64;
        }
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            if let Ok(number) = text.parse::<f64>()
                && number > 0.0
            {
                return number.round() as u64;
            }
            if let Some(seconds) = parse_duration_string(text) {
                return seconds;
            }
        }
    }
    for key in ["duration_string", "durationString"] {
        if let Some(text) = value.get(key).and_then(Value::as_str)
            && let Some(seconds) = parse_duration_string(text)
        {
            return seconds;
        }
    }
    0
}

pub(super) fn parse_duration_string(value: &str) -> Option<u64> {
    let parts = value.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [m, s] => Some(m.parse::<u64>().ok()? * 60 + s.parse::<u64>().ok()?),
        [h, m, s] => Some(
            h.parse::<u64>().ok()? * 3600 + m.parse::<u64>().ok()? * 60 + s.parse::<u64>().ok()?,
        ),
        _ => None,
    }
}

pub(super) fn str_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
}

pub(super) fn value_as_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

pub(super) fn safe_name(value: &str) -> String {
    crate::support::sanitize_filename_segment(value, 100, Some("UNKNOWN"))
}

pub(super) fn collision_path(dir: &Path, base: &str, extension: &str) -> Result<PathBuf> {
    for i in 0..=9999 {
        let name = if i == 0 {
            format!("{base}{extension}")
        } else {
            format!("{base}_{i:03}{extension}")
        };
        let path = dir.join(name);
        if !path.exists() {
            if path.to_string_lossy().chars().count() > 240 {
                bail!("VOD 전체 출력 경로가 안전 제한을 초과했습니다.");
            }
            return Ok(path);
        }
    }
    bail!("충돌 없는 출력 파일명을 만들지 못했습니다.")
}

pub(super) fn redact(text: &str) -> String {
    static SECRET_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(
            r"(?i)(cookie|token|signature|policy|key-pair-id|authorization)(\s*[:=]\s*)[^\s;&]+",
        )
        .unwrap()
    });
    static QUERY_RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"(?i)(https?://[^?\s]+)\?[^\s]+").unwrap());
    let safe = SECRET_RE.replace_all(text, "$1$2***");
    QUERY_RE.replace_all(&safe, "$1?[REDACTED]").to_string()
}

pub(super) fn ffconcat_line(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    format!("file '{}'", normalized.replace('\'', "'\\''"))
}
