use crate::model::{Channel, WatcherStatus};
use anyhow::{bail, Context, Result};
use atomic_write_file::AtomicWriteFile;
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    env,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::{Child, Command},
    sync::{Mutex, RwLock},
};

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
}

impl LogBuffer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(VecDeque::with_capacity(LOG_CAPACITY))),
        }
    }

    pub async fn push(&self, line: impl Into<String>) {
        let mut logs = self.inner.write().await;
        logs.push_back(line.into());
        while logs.len() > LOG_CAPACITY {
            logs.pop_front();
        }
    }

    pub async fn tail(&self, max_lines: usize) -> Vec<String> {
        let logs = self.inner.read().await;
        let start = logs.len().saturating_sub(max_lines.max(1));
        logs.iter().skip(start).cloned().collect()
    }
}

struct WatcherRuntime {
    child: Option<Child>,
    pid: Option<u32>,
    last_exit_code: Option<i32>,
}

pub struct WatcherManager {
    backend_dir: PathBuf,
    runtime: Mutex<WatcherRuntime>,
    logs: LogBuffer,
}

impl WatcherManager {
    pub fn new(backend_dir: PathBuf, logs: LogBuffer) -> Self {
        Self {
            backend_dir,
            runtime: Mutex::new(WatcherRuntime {
                child: None,
                pid: None,
                last_exit_code: None,
            }),
            logs,
        }
    }

    pub async fn start(&self) -> Result<WatcherStatus> {
        let mut runtime = self.runtime.lock().await;
        refresh_runtime(&mut runtime)?;

        if runtime.child.is_some() {
            return Ok(snapshot_from_runtime(&runtime));
        }

        let ps1 = self.backend_dir.join("SOOP_LIVE.ps1");
        if !ps1.is_file() {
            bail!("SOOP_LIVE.ps1 not found: {}", ps1.display());
        }

        let shell = if cfg!(windows) { "powershell.exe" } else { "pwsh" };
        let escaped_ps1 = ps1.to_string_lossy().replace('\'', "''");
        let ps_command = format!(
            "[Console]::InputEncoding=[System.Text.UTF8Encoding]::new($false); \
             [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); \
             $OutputEncoding=[System.Text.UTF8Encoding]::new($false); \
             & '{escaped_ps1}'"
        );

        let mut command = Command::new(shell);
        command.arg("-NoLogo").arg("-NoProfile");
        if cfg!(windows) {
            command.arg("-ExecutionPolicy").arg("Bypass");
        }
        command
            .arg("-Command")
            .arg(ps_command)
            .current_dir(&self.backend_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false);

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to start {shell}"))?;

        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        runtime.pid = pid;
        runtime.last_exit_code = None;
        runtime.child = Some(child);

        if let Some(stdout) = stdout {
            spawn_reader(stdout, self.logs.clone(), "[WATCHER]");
        }
        if let Some(stderr) = stderr {
            spawn_reader(stderr, self.logs.clone(), "[WATCHER:ERR]");
        }

        self.logs
            .push(format!(
                "[SERVER] watcher started (pid={})",
                pid.map(|v| v.to_string()).unwrap_or_else(|| "?".into())
            ))
            .await;

        Ok(snapshot_from_runtime(&runtime))
    }

    pub async fn stop(&self) -> Result<WatcherStatus> {
        let mut runtime = self.runtime.lock().await;
        refresh_runtime(&mut runtime)?;

        let Some(pid) = runtime.pid else {
            return Ok(snapshot_from_runtime(&runtime));
        };

        let exit_code = {
            let Some(child) = runtime.child.as_mut() else {
                runtime.pid = None;
                return Ok(snapshot_from_runtime(&runtime));
            };

            #[cfg(windows)]
            {
                let taskkill_status = Command::new("taskkill.exe")
                    .arg("/PID")
                    .arg(pid.to_string())
                    .arg("/T")
                    .arg("/F")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await;

                let taskkill_ok = taskkill_status
                    .as_ref()
                    .map(|status| status.success())
                    .unwrap_or(false);

                if !taskkill_ok {
                    if let Some(status) = child
                        .try_wait()
                        .context("failed to verify watcher after taskkill failure")?
                    {
                        status.code()
                    } else {
                        bail!("taskkill /PID {pid} /T /F failed; watcher remains running");
                    }
                } else {
                    child.wait().await.ok().and_then(|status| status.code())
                }
            }

            #[cfg(not(windows))]
            {
                let _ = child.kill().await;
                child.wait().await.ok().and_then(|status| status.code())
            }
        };

        runtime.child = None;
        runtime.pid = None;
        runtime.last_exit_code = exit_code;

        self.logs
            .push(format!(
                "[SERVER] watcher stopped (exit={})",
                exit_code.map(|v| v.to_string()).unwrap_or_else(|| "unknown".into())
            ))
            .await;

        Ok(snapshot_from_runtime(&runtime))
    }

    pub async fn status(&self) -> Result<WatcherStatus> {
        let mut runtime = self.runtime.lock().await;
        let was_running = runtime.child.is_some();
        refresh_runtime(&mut runtime)?;
        if was_running && runtime.child.is_none() {
            self.logs
                .push(format!(
                    "[SERVER] watcher exited (exit={})",
                    runtime
                        .last_exit_code
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unknown".into())
                ))
                .await;
        }
        Ok(snapshot_from_runtime(&runtime))
    }
}

fn refresh_runtime(runtime: &mut WatcherRuntime) -> Result<()> {
    let exit = match runtime.child.as_mut() {
        Some(child) => child.try_wait().context("failed to query watcher status")?,
        None => None,
    };
    if let Some(status) = exit {
        runtime.last_exit_code = status.code();
        runtime.child = None;
        runtime.pid = None;
    }
    Ok(())
}

fn snapshot_from_runtime(runtime: &WatcherRuntime) -> WatcherStatus {
    WatcherStatus {
        running: runtime.child.is_some(),
        pid: runtime.pid,
        last_exit_code: runtime.last_exit_code,
    }
}

fn spawn_reader<R>(reader: R, logs: LogBuffer, prefix: &'static str)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => logs.push(format!("{prefix} {line}")).await,
                Ok(None) => break,
                Err(err) => {
                    logs.push(format!("{prefix} <read error: {err}>")).await;
                    break;
                }
            }
        }
    });
}

pub fn resolve_backend_dir() -> Result<PathBuf> {
    if let Ok(value) = env::var("SOOP_BACKEND_DIR") {
        let path = PathBuf::from(value);
        if path.join("SOOP_LIVE.ps1").is_file() {
            return fs::canonicalize(&path)
                .with_context(|| format!("cannot canonicalize {}", path.display()));
        }
        bail!(
            "SOOP_BACKEND_DIR does not contain SOOP_LIVE.ps1: {}",
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
        if candidate.join("SOOP_LIVE.ps1").is_file() {
            return fs::canonicalize(&candidate)
                .with_context(|| format!("cannot canonicalize {}", candidate.display()));
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
        bail!("runtime file missing and example not found: {}", example.display());
    }
    fs::copy(example, target).with_context(|| {
        format!("failed to initialize {} from {}", target.display(), example.display())
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
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
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

pub fn update_settings(path: &Path, updates: &BTreeMap<String, String>) -> Result<()> {
    validate_setting_updates(updates)?;
    let original = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
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

fn validate_setting_updates(updates: &BTreeMap<String, String>) -> Result<()> {
    let allowed: HashSet<&str> = SAFE_SETTING_KEYS.iter().copied().collect();
    for (key, value) in updates {
        if !allowed.contains(key.as_str()) {
            bail!("setting is not editable in Phase 1: {key}");
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

fn validate_int(value: &str, min: u64, max: u64, key: &str) -> Result<()> {
    let parsed: u64 = value
        .parse()
        .with_context(|| format!("{key} must be an integer"))?;
    if !(min..=max).contains(&parsed) {
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

pub fn read_channels(path: &Path) -> Result<Vec<Channel>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
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

fn preferred_newline(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else if content.contains('\r') && !content.contains('\n') {
        "\r"
    } else {
        "\n"
    }
}

fn backup_existing(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let mut backup_name = path.as_os_str().to_os_string();
    backup_name.push(".bak");
    let backup = PathBuf::from(backup_name);
    fs::copy(path, &backup).with_context(|| {
        format!("failed to create backup {} from {}", backup.display(), path.display())
    })?;
    Ok(())
}

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
            Channel { enabled: true, name: "A".into(), account: "same".into(), outdir: "".into() },
            Channel { enabled: true, name: "B".into(), account: "SAME".into(), outdir: "".into() },
        ];
        assert!(write_channels(&path, &dup).is_err());
    }
}
