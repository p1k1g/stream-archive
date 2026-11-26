use super::{Metadata, redact};
use crate::model::VodQualityOption;
use anyhow::{Context, Result, anyhow, bail};
use chrono::{Datelike, Local, Utc};
use regex::Regex;
use serde_json::Value;
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::ExitStatus,
};
use url::Url;

pub(super) fn quality_options_from_streamlink_json(value: &Value) -> Result<Vec<VodQualityOption>> {
    let streams = value
        .get("streams")
        .and_then(Value::as_object)
        .context("Streamlink CHZZK JSON has no streams")?;
    static QUALITY_NAME_RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"^(\d+)p(\d+)?$").unwrap());
    let mut names = streams
        .keys()
        .filter_map(|name| {
            let captures = QUALITY_NAME_RE.captures(name)?;
            let height = captures.get(1)?.as_str().parse::<u64>().ok()?;
            let fps = captures
                .get(2)
                .and_then(|value| value.as_str().parse::<u64>().ok())
                .unwrap_or(0);
            Some((height, fps, name.clone()))
        })
        .collect::<Vec<_>>();
    names.sort_by(|left, right| right.cmp(left));

    let mut options = vec![VodQualityOption {
        value: "best".into(),
        label: "최고 화질 (자동)".into(),
    }];
    for (_, _, name) in names {
        options.push(VodQualityOption {
            value: name.clone(),
            label: name,
        });
    }
    Ok(options)
}

pub(super) fn metadata_from_chzzk_content(content: &Value) -> Result<Metadata> {
    let title = content
        .get("videoTitle")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        bail!("CHZZK API did not return a VOD title");
    }
    let streamer = content
        .pointer("/channel/channelName")
        .and_then(Value::as_str)
        .unwrap_or("CHZZK")
        .trim()
        .to_string();
    let streamer_id = content
        .pointer("/channel/channelId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let duration_seconds = content
        .get("duration")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_f64().map(|n| n.max(0.0) as u64))
        })
        .unwrap_or(0);
    let date = content
        .get("publishDate")
        .and_then(Value::as_str)
        .and_then(short_date)
        .or_else(|| {
            content
                .get("publishDateAt")
                .and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|n| n as i64)))
                .and_then(chrono::DateTime::<Utc>::from_timestamp_millis)
                .map(|timestamp| {
                    let local = timestamp.with_timezone(&Local);
                    format!(
                        "{:02}{:02}{:02}",
                        local.year() % 100,
                        local.month(),
                        local.day()
                    )
                })
        })
        .unwrap_or_else(today_short_date);
    Ok(Metadata {
        title,
        thumbnail_url: content
            .get("thumbnailImageUrl")
            .or_else(|| content.get("videoImageUrl"))
            .and_then(Value::as_str)
            .filter(|url| !url.trim().is_empty())
            .map(str::to_string),
        streamer,
        streamer_id,
        date,
        duration_seconds,
        qualities: vec![VodQualityOption {
            value: "best".into(),
            label: "최고 화질 (자동)".into(),
        }],
    })
}

pub(super) fn streamlink_quality_args(quality: &str) -> (Vec<String>, String) {
    static MAX_HEIGHT_RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"^best\[height<=(\d+)\]$").unwrap());
    if let Some(captures) = MAX_HEIGHT_RE.captures(quality) {
        let height = captures.get(1).unwrap().as_str();
        return (
            vec!["--stream-sorting-excludes".into(), format!(">{height}p")],
            "best".into(),
        );
    }
    let stream = if quality.is_empty() { "best" } else { quality };
    (Vec::new(), stream.to_string())
}

pub(super) fn ffmpeg_progress_seconds(line: &str) -> Option<f64> {
    let raw = line.trim().strip_prefix("out_time_us=")?;
    let micros = raw.parse::<u64>().ok()?;
    Some(micros as f64 / 1_000_000.0)
}

pub(super) fn download_progress_percent(media_seconds: f64, duration_seconds: u64) -> f64 {
    if duration_seconds == 0 {
        return 0.0;
    }
    (media_seconds.max(0.0) / duration_seconds as f64 * 100.0).clamp(0.0, 99.0)
}

pub(super) fn format_media_time(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

pub(super) fn find_finished_output(expected: &Path) -> Result<PathBuf> {
    if expected.is_file() {
        return Ok(expected.to_path_buf());
    }
    let parent = expected.parent().unwrap_or_else(|| Path::new("."));
    let stem = expected
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let mut candidates = fs::read_dir(parent)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.starts_with(stem))
                && matches!(
                    path.extension()
                        .and_then(|value| value.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase()
                        .as_str(),
                    "ts" | "mp4" | "mkv" | "webm"
                )
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("CHZZK 완료 파일을 찾지 못했습니다: {}", expected.display()))
}

pub(super) fn validate_url(raw: &str) -> Result<()> {
    video_id(raw).map(|_| ())
}

pub(super) fn video_id(raw: &str) -> Result<String> {
    let url = Url::parse(raw.trim()).context("CHZZK VOD URL 형식이 올바르지 않습니다.")?;
    if url.scheme() != "https" && url.scheme() != "http" {
        bail!("CHZZK VOD URL은 http/https 주소여야 합니다.");
    }
    if !url
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("chzzk.naver.com"))
    {
        bail!("CHZZK VOD URL 호스트가 아닙니다.");
    }
    let parts = url
        .path_segments()
        .map(|segments| segments.filter(|part| !part.is_empty()).collect::<Vec<_>>())
        .unwrap_or_default();
    if parts.len() != 2 || parts[0] != "video" || !parts[1].chars().all(|c| c.is_ascii_digit()) {
        bail!("CHZZK VOD URL은 https://chzzk.naver.com/video/<숫자> 형식이어야 합니다.");
    }
    Ok(parts[1].to_string())
}

pub(super) fn short_date(value: &str) -> Option<String> {
    let digits = value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>();
    (digits.len() >= 8).then(|| digits[2..8].to_string())
}

pub(super) fn today_short_date() -> String {
    let today = Local::now().date_naive();
    format!(
        "{:02}{:02}{:02}",
        today.year() % 100,
        today.month(),
        today.day()
    )
}

pub(super) fn safe_name(value: &str, max: usize) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() {
            continue;
        }
        if r#"\/:*?"<>|"#.contains(ch) {
            output.push('_');
        } else {
            output.push(ch);
        }
    }
    let mut output = output
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .trim_end_matches('.')
        .to_string();
    if output.is_empty() {
        output = "UNKNOWN".into();
    }
    if output.chars().count() > max {
        output = output
            .chars()
            .take(max)
            .collect::<String>()
            .trim()
            .trim_end_matches('.')
            .to_string();
    }
    output
}

pub(super) fn validate_retries(retries: u32) -> Result<()> {
    if !(1..=20).contains(&retries) {
        bail!("VOD 재시도 횟수는 1~20 사이여야 합니다.");
    }
    Ok(())
}

pub(super) fn push_tail(tail: &mut VecDeque<String>, line: &str) {
    if tail.len() >= 20 {
        tail.pop_front();
    }
    let clean = redact(line.trim());
    if !clean.is_empty() {
        tail.push_back(clean);
    }
}

pub(super) fn exit_code(status: ExitStatus) -> String {
    status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unknown".into())
}
