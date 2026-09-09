use crate::model::Channel;
use anyhow::{Context, Result, bail};
#[cfg(test)]
use atomic_write_file::AtomicWriteFile;
#[cfg(test)]
use std::io::Write;
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

pub const HIDDEN_SETTING_KEYS: &[&str] = &["SOOP_PASSWORD", "CLOUDFLARE_API_KEY"];

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

#[cfg(test)]
pub fn update_settings(path: &Path, updates: &BTreeMap<String, String>) -> Result<()> {
    validate_setting_updates(updates)?;
    let original =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let newline = preferred_newline(&original);
    let normalized = normalize_line_endings(&original);
    let mut seen = HashSet::new();
    let mut output = Vec::new();

    for raw in normalized.lines() {
        let trimmed = raw.trim();
        if !trimmed.starts_with('#') {
            if let Some((key, _)) = raw.split_once('=') {
                let key = key.trim();
                if let Some(value) = updates.get(key) {
                    output.push(format!("{key}={value}"));
                    seen.insert(key.to_string());
                    continue;
                }
            }
        }
        output.push(raw.to_string());
    }

    for (key, value) in updates {
        if !seen.contains(key) {
            output.push(format!("{key}={value}"));
        }
    }

    let mut content = output.join(newline);
    content.push_str(newline);
    backup_existing(path)?;
    write_atomic(path, content.as_bytes())
}

#[cfg(test)]
fn validate_setting_updates(updates: &BTreeMap<String, String>) -> Result<()> {
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
            "MIN_FREE_SPACE_GB" => validate_int(value, 0, 1_000_000, key)?,
            "LOG_RETENTION_DAYS" => validate_int(value, 0, 36_500, key)?,
            "CONSOLE_AUTO_FORMAT"
            | "CONSOLE_COLOR"
            | "CONSOLE_SHOW_PATH"
            | "GUI_NOTIFY_RECORD_START"
            | "GUI_NOTIFY_RECORD_FINISH"
            | "GUI_NOTIFY_WARNING"
            | "LOG_ENABLED" => validate_yes_no(value, key)?,
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_int(value: &str, min: u64, max: u64, key: &str) -> Result<()> {
    let parsed: u64 = value
        .parse()
        .with_context(|| format!("{key} must be an integer"))?;
    if !(min..=max).contains(&parsed) {
        bail!("{key} must be between {min} and {max}");
    }
    Ok(())
}

#[cfg(test)]
fn validate_yes_no(value: &str, key: &str) -> Result<()> {
    if !matches!(value.to_ascii_uppercase().as_str(), "Y" | "N") {
        bail!("{key} must be Y or N");
    }
    Ok(())
}

pub fn read_channels(path: &Path) -> Result<Vec<Channel>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let normalized = normalize_line_endings(&content);
    let mut channels = Vec::new();

    for (index, raw) in normalized.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = raw.splitn(4, '|').collect();
        if parts.len() != 4 {
            bail!("invalid channel line {} in {}", index + 1, path.display());
        }
        channels.push(Channel {
            enabled: parts[0].trim().eq_ignore_ascii_case("Y"),
            name: parts[1].trim().to_string(),
            account: parts[2].trim().to_string(),
            outdir: parts[3].trim().to_string(),
        });
    }
    Ok(channels)
}

#[cfg(test)]
pub fn write_channels(path: &Path, channels: &[Channel]) -> Result<()> {
    let mut lines = vec!["# ENABLED|NAME|ACCOUNT|OUTDIR".to_string()];
    let mut accounts = HashSet::new();
    for channel in channels {
        validate_channel(channel)?;
        let account_key = channel.account.to_ascii_lowercase();
        if !accounts.insert(account_key) {
            bail!("duplicate channel account: {}", channel.account);
        }
        lines.push(format!(
            "{}|{}|{}|{}",
            if channel.enabled { "Y" } else { "N" },
            channel.name.trim(),
            channel.account.trim(),
            channel.outdir.trim()
        ));
    }
    let content = format!("{}\r\n", lines.join("\r\n"));
    backup_existing(path)?;
    write_atomic(path, content.as_bytes())
}

#[cfg(test)]
fn validate_channel(channel: &Channel) -> Result<()> {
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
    Ok(())
}

#[cfg(test)]
fn validate_single_line(value: &str, max_len: usize, label: &str) -> Result<()> {
    if value.len() > max_len {
        bail!("{label} is too long");
    }
    if value.contains('\r') || value.contains('\n') || value.contains('\0') {
        bail!("{label} must be a single line");
    }
    Ok(())
}

fn normalize_line_endings(content: &str) -> String {
    content.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
fn preferred_newline(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else if content.contains('\r') && !content.contains('\n') {
        "\r"
    } else {
        "\n"
    }
}

#[cfg(test)]
fn backup_existing(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let mut backup_name = path.as_os_str().to_os_string();
    backup_name.push(".bak");
    let backup = PathBuf::from(backup_name);
    fs::copy(path, &backup).with_context(|| {
        format!(
            "failed to create backup {} from {}",
            backup.display(),
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = AtomicWriteFile::options()
        .open(path)
        .with_context(|| format!("failed to open atomic writer for {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.commit()
        .with_context(|| format!("failed to commit {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    #[cfg(windows)]
    #[test]
    fn strips_windows_verbatim_path_prefixes() {
        assert_eq!(
            strip_windows_verbatim_prefix(r"\\?\C:\Users\test\backend"),
            r"C:\Users\test\backend"
        );
        assert_eq!(
            strip_windows_verbatim_prefix(r"\\?\UNC\server\share\backend"),
            r"\\server\share\backend"
        );
    }

    #[test]
    fn channel_parser_accepts_crlf_lf_and_cr() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("channels.txt");
        for content in [
            "# header\r\nY|A|a|\r\nN|B|b|D:\\B\r\n",
            "# header\nY|A|a|\nN|B|b|D:\\B\n",
            "# header\rY|A|a|\rN|B|b|D:\\B\r",
        ] {
            fs::write(&path, content).unwrap();
            let channels = read_channels(&path).unwrap();
            assert_eq!(channels.len(), 2);
            assert_eq!(channels[0].account, "a");
            assert_eq!(channels[1].outdir, "D:\\B");
        }
    }

    #[test]
    fn settings_update_preserves_hidden_secret_and_creates_backup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("SOOP_LIVE_SETTING.ini");
        let original = "CHECK_INTERVAL=30\r\nSOOP_PASSWORD=dpapi:v1:secret\r\nQUALITY=best\r\n";
        fs::write(&path, original).unwrap();

        let mut updates = BTreeMap::new();
        updates.insert("CHECK_INTERVAL".into(), "15".into());
        updates.insert("QUALITY".into(), "1080p".into());
        update_settings(&path, &updates).unwrap();

        let new_content = fs::read_to_string(&path).unwrap();
        assert!(new_content.contains("CHECK_INTERVAL=15"));
        assert!(new_content.contains("QUALITY=1080p"));
        assert!(new_content.contains("SOOP_PASSWORD=dpapi:v1:secret"));
        let backup = dir.path().join("SOOP_LIVE_SETTING.ini.bak");
        assert_eq!(fs::read_to_string(backup).unwrap(), original);
    }

    #[test]
    fn channel_write_creates_backup_and_rejects_duplicate_accounts() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("SOOP_LIVE_CHANNELS.txt");
        fs::write(&path, "Y|Old|old|\r\n").unwrap();

        let channels = vec![Channel {
            enabled: true,
            name: "New".into(),
            account: "new".into(),
            outdir: "".into(),
        }];
        write_channels(&path, &channels).unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("Y|New|new|"));
        assert_eq!(
            fs::read_to_string(dir.path().join("SOOP_LIVE_CHANNELS.txt.bak")).unwrap(),
            "Y|Old|old|\r\n"
        );

        let dup = vec![
            Channel {
                enabled: true,
                name: "A".into(),
                account: "same".into(),
                outdir: "".into(),
            },
            Channel {
                enabled: true,
                name: "B".into(),
                account: "SAME".into(),
                outdir: "".into(),
            },
        ];
        assert!(write_channels(&path, &dup).is_err());
    }
}
