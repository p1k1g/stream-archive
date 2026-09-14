use crate::{model::Channel, support::platform::PlatformId};
use anyhow::{Context, Result, bail};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{RwLock, broadcast};

const SETTINGS_FILE: &str = "SOOP_LIVE_SETTING.ini";
const SETTINGS_EXAMPLE_FILE: &str = "SOOP_LIVE_SETTING.example.ini";
const CHANNELS_FILE: &str = "SOOP_LIVE_CHANNELS.txt";
const CHANNELS_EXAMPLE_FILE: &str = "SOOP_LIVE_CHANNELS.example.txt";

pub const HIDDEN_SETTING_KEYS: &[&str] = &[
    "SOOP_PASSWORD",
    "CLOUDFLARE_API_KEY",
    "CHZZK_NID_AUT",
    "CHZZK_NID_SES",
];

pub const SAFE_SETTING_KEYS: &[&str] = &[
    "CHECK_INTERVAL",
    "CHANNEL_RELOAD_INTERVAL",
    "RECORD_RETRY_INTERVAL",
    "RECORD_STALL_TIMEOUT",
    "RECORD_MONITOR_INTERVAL",
    "WORKER_MAX_RETRY",
    "CONSOLE_REFRESH_INTERVAL",
    "CONSOLE_AUTO_FORMAT",
    "CHANNEL_NAME_WIDTH",
    "CONSOLE_COLOR",
    "CONSOLE_SHOW_PATH",
    "GUI_NOTIFY_RECORD_START",
    "GUI_NOTIFY_RECORD_FINISH",
    "GUI_NOTIFY_WARNING",
    "MIN_FREE_SPACE_GB",
    "OUTPUT_DIR",
    "QUALITY",
    "FILE_NAME_PATTERN",
    "STREAMLINK_PATH",
    "STREAMLINK_FALLBACK",
    "SOOP_USERNAME",
    "CLOUDFLARE_WORKER_URL",
    "MASTER_QUALITY",
    "LOG_ENABLED",
    "LOG_DIR",
    "LOG_RETENTION_DAYS",
    "BACKUP_ENABLED",
    "BACKUP_INTERVAL_HOURS",
    "BACKUP_KEEP_COUNT",
    "BACKUP_RETENTION_DAYS",
    "BACKUP_DIR",
];

const LOG_CAPACITY: usize = 400;

#[derive(Clone)]
pub struct LogBuffer {
    inner: Arc<RwLock<VecDeque<String>>>,
    events: broadcast::Sender<()>,
}

impl LogBuffer {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(128);
        Self {
            inner: Arc::new(RwLock::new(VecDeque::with_capacity(LOG_CAPACITY))),
            events,
        }
    }

    pub async fn push(&self, line: impl Into<String>) {
        let mut logs = self.inner.write().await;
        logs.push_back(line.into());
        while logs.len() > LOG_CAPACITY {
            logs.pop_front();
        }
        drop(logs);
        let _ = self.events.send(());
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.events.subscribe()
    }

    pub async fn tail(&self, max_lines: usize) -> Vec<String> {
        let logs = self.inner.read().await;
        let start = logs.len().saturating_sub(max_lines.max(1));
        logs.iter().skip(start).cloned().collect()
    }
}

#[cfg(windows)]
fn strip_windows_verbatim_prefix(value: &str) -> String {
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = value.strip_prefix(r"\\?\") {
        return rest.to_string();
    }
    value.to_string()
}

fn child_process_compatible_path(path: PathBuf) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .context("cannot resolve current directory")?
            .join(path)
    };

    #[cfg(windows)]
    {
        return Ok(PathBuf::from(strip_windows_verbatim_prefix(
            &absolute.to_string_lossy(),
        )));
    }

    #[cfg(not(windows))]
    {
        Ok(absolute)
    }
}

pub fn resolve_backend_dir() -> Result<PathBuf> {
    if let Ok(value) = env::var("SOOP_BACKEND_DIR") {
        let path = child_process_compatible_path(PathBuf::from(value))?;
        if path.join(SETTINGS_FILE).is_file() || path.join(SETTINGS_EXAMPLE_FILE).is_file() {
            return Ok(path);
        }
        bail!(
            "SOOP_BACKEND_DIR does not contain SOOP runtime files: {}",
            path.display()
        );
    }

    let mut candidates = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join("backend"));
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("backend"));
        }
    }

    for candidate in candidates {
        let candidate = child_process_compatible_path(candidate)?;
        if candidate.join(SETTINGS_FILE).is_file()
            || candidate.join(SETTINGS_EXAMPLE_FILE).is_file()
        {
            return Ok(candidate);
        }
    }

    bail!(
        "backend directory not found. Expected ./backend or <exe-dir>/backend. Set SOOP_BACKEND_DIR for an explicit location."
    )
}

pub fn ensure_runtime_files(backend_dir: &Path) -> Result<()> {
    ensure_from_example(
        &backend_dir.join(SETTINGS_FILE),
        &backend_dir.join(SETTINGS_EXAMPLE_FILE),
    )?;
    ensure_from_example(
        &backend_dir.join(CHANNELS_FILE),
        &backend_dir.join(CHANNELS_EXAMPLE_FILE),
    )?;
    Ok(())
}

fn ensure_from_example(target: &Path, example: &Path) -> Result<()> {
    if target.exists() {
        return Ok(());
    }
    if !example.is_file() {
        bail!(
            "runtime file missing and example not found: {}",
            example.display()
        );
    }
    fs::copy(example, target).with_context(|| {
        format!(
            "failed to initialize {} from {}",
            target.display(),
            example.display()
        )
    })?;
    Ok(())
}

pub fn settings_path(backend_dir: &Path) -> PathBuf {
    backend_dir.join(SETTINGS_FILE)
}

pub fn channels_path(backend_dir: &Path) -> PathBuf {
    backend_dir.join(CHANNELS_FILE)
}

pub fn read_safe_settings(path: &Path) -> Result<BTreeMap<String, String>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let safe: HashSet<&str> = SAFE_SETTING_KEYS.iter().copied().collect();
    let mut result = BTreeMap::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if safe.contains(key) {
            result.insert(key.to_string(), value.trim().to_string());
        }
    }
    Ok(result)
}

pub fn read_channels(path: &Path) -> Result<Vec<Channel>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    parse_channels(&content)
}

pub fn parse_channels(content: &str) -> Result<Vec<Channel>> {
    let normalized = content
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut channels = Vec::new();
    for (index, raw) in normalized.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 4 {
            bail!("invalid channel line {}: expected 4 fields", index + 1);
        }
        channels.push(Channel {
            platform: PlatformId::Soop,
            enabled: parts[0].trim().eq_ignore_ascii_case("Y"),
            name: parts[1].trim().to_string(),
            account: parts[2].trim().to_string(),
            outdir: parts[3].trim().to_string(),
        });
    }
    Ok(channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_channel_file() {
        let channels = parse_channels("Y|Alpha|alpha|C:\\A\nN|Beta|beta|C:\\B\n").unwrap();
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "Alpha");
        assert_eq!(channels[0].platform, PlatformId::Soop);
        assert!(channels[0].enabled);
        assert!(!channels[1].enabled);
    }

    #[test]
    fn parses_legacy_channel_line_endings() {
        for separator in ["\n", "\r\n", "\r"] {
            let content =
                format!("\u{feff}Y|Alpha|alpha|C:\\A{separator}N|Beta|beta|C:\\B{separator}");
            let channels = parse_channels(&content).unwrap();
            assert_eq!(channels.len(), 2, "separator={separator:?}");
            assert_eq!(channels[0].name, "Alpha");
            assert_eq!(channels[1].name, "Beta");
        }
    }

    #[test]
    fn parses_empty_outdir() {
        let channels = parse_channels("Y|Alpha|alpha|\n").unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].outdir, "");
    }
}
