use crate::{
    backend::{HIDDEN_SETTING_KEYS, SAFE_SETTING_KEYS},
    model::Channel,
};
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, HashSet};

pub const VOD_TOOL_KEYS: &[&str] = &["YT_DLP_PATH", "FFMPEG_PATH"];

pub fn validate_setting_updates(updates: &BTreeMap<String, String>) -> Result<()> {
    let allowed: HashSet<&str> = SAFE_SETTING_KEYS.iter().copied().collect();
    for (key, value) in updates {
        if !allowed.contains(key.as_str()) {
            bail!("setting is not editable in Rust web: {key}");
        }
        validate_single_line(value, 2048, &format!("setting {key}"))?;
        match key.as_str() {
            "CHECK_INTERVAL" => validate_int(value, 1, 86_400, key)?,
            "CHANNEL_RELOAD_INTERVAL" => validate_int(value, 1, 3_600, key)?,
            "RECORD_RETRY_INTERVAL" => validate_int(value, 1, 3_600, key)?,
            "RECORD_STALL_TIMEOUT" => validate_int(value, 10, 86_400, key)?,
            "RECORD_MONITOR_INTERVAL" => validate_int(value, 1, 3_600, key)?,
            "WORKER_MAX_RETRY" => validate_int(value, 0, 100, key)?,
            "CONSOLE_REFRESH_INTERVAL" => validate_int(value, 1, 3_600, key)?,
            "MIN_FREE_SPACE_GB" => validate_number(value, 0.0, 1_000_000.0, key)?,
            "LOG_RETENTION_DAYS" => validate_int(value, 0, 36_500, key)?,
            "CONSOLE_AUTO_FORMAT"
            | "CONSOLE_COLOR"
            | "CONSOLE_SHOW_PATH"
            | "GUI_NOTIFY_RECORD_START"
            | "GUI_NOTIFY_RECORD_FINISH"
            | "GUI_NOTIFY_WARNING"
            | "LOG_ENABLED" => validate_yes_no(value, key)?,
            "CLOUDFLARE_WORKER_URL"
                if !value.trim().is_empty() && !value.starts_with("https://") =>
            {
                bail!("CLOUDFLARE_WORKER_URL must be https://");
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn validate_secret_updates(updates: &BTreeMap<String, String>) -> Result<()> {
    for (key, value) in updates {
        if !HIDDEN_SETTING_KEYS.contains(&key.as_str()) {
            bail!("unsupported secret key: {key}");
        }
        validate_single_line(value, 16_384, &format!("secret {key}"))?;
    }
    Ok(())
}

pub fn validate_channels(channels: &[Channel]) -> Result<()> {
    let mut accounts = HashSet::new();
    for channel in channels {
        validate_single_line(&channel.name, 200, "channel name")?;
        validate_single_line(&channel.account, 200, "channel account")?;
        validate_single_line(&channel.outdir, 2048, "channel output directory")?;
        if channel.name.trim().is_empty() {
            bail!("channel name cannot be empty");
        }
        if channel.account.trim().is_empty() {
            bail!("channel account cannot be empty");
        }
        for (label, value) in [
            ("channel name", channel.name.as_str()),
            ("channel account", channel.account.as_str()),
            ("channel output directory", channel.outdir.as_str()),
        ] {
            if value.contains('|') {
                bail!("{label} cannot contain '|'");
            }
        }
        if !accounts.insert(channel.account.trim().to_ascii_lowercase()) {
            bail!("duplicate channel account: {}", channel.account);
        }
    }
    Ok(())
}

pub fn validate_vod_tool_updates(updates: &BTreeMap<String, String>) -> Result<()> {
    for (key, value) in updates {
        if !VOD_TOOL_KEYS.contains(&key.as_str()) {
            bail!("unsupported VOD tool setting: {key}");
        }
        validate_single_line(value, 2048, &format!("VOD tool path {key}"))?;
    }
    Ok(())
}

pub fn apply_vod_tool_defaults(
    values: &BTreeMap<String, String>,
    yt_dlp: &mut String,
    ffmpeg: &mut String,
) {
    if yt_dlp.trim().is_empty() {
        *yt_dlp = values.get("YT_DLP_PATH").cloned().unwrap_or_default();
    }
    if ffmpeg.trim().is_empty() {
        *ffmpeg = values.get("FFMPEG_PATH").cloned().unwrap_or_default();
    }
}

fn validate_single_line(value: &str, max_len: usize, label: &str) -> Result<()> {
    if value.len() > max_len {
        bail!("{label} is too long");
    }
    if value.contains('\r') || value.contains('\n') || value.contains('\0') {
        bail!("{label} must be a single line");
    }
    Ok(())
}

fn validate_int(value: &str, min: u64, max: u64, key: &str) -> Result<()> {
    let parsed: u64 = value
        .parse()
        .with_context(|| format!("{key} must be an integer"))?;
    if !(min..=max).contains(&parsed) {
        bail!("{key} must be between {min} and {max}");
    }
    Ok(())
}

fn validate_number(value: &str, min: f64, max: f64, key: &str) -> Result<()> {
    let parsed: f64 = value
        .parse()
        .with_context(|| format!("{key} must be numeric"))?;
    if !parsed.is_finite() || parsed < min || parsed > max {
        bail!("{key} must be between {min} and {max}");
    }
    Ok(())
}

fn validate_yes_no(value: &str, key: &str) -> Result<()> {
    if !matches!(value.to_ascii_uppercase().as_str(), "Y" | "N") {
        bail!("{key} must be Y or N");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_channel_accounts_case_insensitively() {
        let channels = vec![
            Channel {
                enabled: true,
                name: "A".into(),
                account: "User".into(),
                outdir: String::new(),
            },
            Channel {
                enabled: true,
                name: "B".into(),
                account: "user".into(),
                outdir: String::new(),
            },
        ];
        assert!(validate_channels(&channels).is_err());
    }

    #[test]
    fn accepts_fractional_min_free_space() {
        let updates = BTreeMap::from([("MIN_FREE_SPACE_GB".into(), "1.5".into())]);
        assert!(validate_setting_updates(&updates).is_ok());
    }
}
