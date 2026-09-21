use anyhow::{Context, Result, bail};
use std::{
    collections::{BTreeMap, VecDeque},
    env,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{RwLock, broadcast};

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

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new()
    }
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

// CHZZK VOD still passes a path-shaped settings handle. The value is deliberately
// opaque: runtime settings are read from SQLite, never from an INI/TXT file.
pub fn settings_path(_backend_dir: &Path) -> PathBuf {
    PathBuf::new()
}

pub fn read_safe_settings(_unused_path: &Path) -> Result<BTreeMap<String, String>> {
    crate::store::global()?.safe_settings()
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
    if let Ok(value) = env::var("STREAM_ARCHIVE_BACKEND_DIR") {
        let path = child_process_compatible_path(PathBuf::from(value))?;
        if path.is_dir() {
            return Ok(path);
        }
        bail!(
            "STREAM_ARCHIVE_BACKEND_DIR is not a directory: {}",
            path.display()
        );
    }

    // Prefer the backend beside the executable so a packaged GUI launched
    // from Explorer remains anchored to its own portable directory even when
    // the inherited working directory points somewhere else. Development runs
    // still fall back to ./backend when target/{debug,release} has no sibling
    // backend directory.
    let mut candidates = Vec::new();
    if let Ok(exe) = env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        candidates.push(exe_dir.join("backend"));
    }
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join("backend"));
    }

    for candidate in candidates {
        let candidate = child_process_compatible_path(candidate)?;
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }

    bail!(
        "backend directory not found. Expected ./backend or <exe-dir>/backend. Set STREAM_ARCHIVE_BACKEND_DIR for an explicit location."
    )
}
