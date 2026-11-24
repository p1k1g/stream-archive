use anyhow::{Context, Result, bail};
use chrono::{Local, NaiveDate};
use std::{
    collections::{BTreeMap, VecDeque},
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::sync::{RwLock, broadcast};

pub const HIDDEN_SETTING_KEYS: &[&str] = &[
    "SOOP_PASSWORD",
    "CLOUDFLARE_API_KEY",
    "CHZZK_NID_AUT",
    "CHZZK_NID_SES",
    "KICK_SESSION_TOKEN",
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
    "GUI_AUTO_START_WATCHER",
    "GUI_START_MINIMIZED_TRAY",
    "STREAM_ARCHIVE_CLOSE_ACTION",
    "STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS",
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
const LOG_LINE_BYTES: usize = 8 * 1024;
const LOG_BUFFER_BYTES: usize = 1024 * 1024;
const TRUNCATED: &str = " … [log truncated]";

struct BufferedLogs {
    lines: VecDeque<String>,
    bytes: usize,
}

fn bounded_log_line(mut line: String) -> String {
    if line.len() <= LOG_LINE_BYTES {
        if line.capacity() > LOG_LINE_BYTES {
            line.shrink_to_fit();
        }
        return line;
    }
    let mut end = LOG_LINE_BYTES - TRUNCATED.len();
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    // A new allocation avoids retaining the original oversized capacity.
    let mut bounded = line[..end].to_owned();
    bounded.push_str(TRUNCATED);
    bounded
}

const LOG_FILE_MAX_BYTES: u64 = 20 * 1024 * 1024;
const LOG_FILE_MAX_ROTATIONS: usize = 5;

enum LogSinkCommand {
    Line(String),
    Clear(std::sync::mpsc::SyncSender<()>),
    #[cfg(test)]
    Flush(std::sync::mpsc::SyncSender<()>),
}

struct LogFileSink {
    dir: PathBuf,
    tx: std::sync::mpsc::SyncSender<LogSinkCommand>,
}

impl LogFileSink {
    fn new(dir: PathBuf, retention_days: u64) -> Self {
        let _ = fs::create_dir_all(&dir);
        prune_expired_logs(&dir, retention_days);
        let (tx, rx) = std::sync::mpsc::sync_channel::<LogSinkCommand>(1024);
        let worker_dir = dir.clone();
        let _ = std::thread::Builder::new()
            .name("runtime-log-writer".into())
            .spawn(move || run_log_writer(worker_dir, retention_days, rx));
        Self { dir, tx }
    }

    fn append_line(&self, line: &str) {
        let now = Local::now();
        let formatted = format!("[{}] {line}", now.format("%Y-%m-%d %H:%M:%S%.3f"));
        let _ = self.tx.try_send(LogSinkCommand::Line(formatted));
    }

    fn clear_files(&self) {
        let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
        if self.tx.send(LogSinkCommand::Clear(done_tx)).is_ok() {
            let _ = done_rx.recv_timeout(Duration::from_secs(2));
        }
    }

    #[cfg(test)]
    fn flush(&self) {
        let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
        if self.tx.send(LogSinkCommand::Flush(done_tx)).is_ok() {
            let _ = done_rx.recv_timeout(Duration::from_secs(2));
        }
    }
}

fn rotate_daily_log(dir: &Path, date_str: &str) {
    let base = dir.join(format!("runtime-{date_str}.log"));
    let oldest = dir.join(format!("runtime-{date_str}.{LOG_FILE_MAX_ROTATIONS}.log"));
    let _ = fs::remove_file(&oldest);
    for idx in (1..LOG_FILE_MAX_ROTATIONS).rev() {
        let src = dir.join(format!("runtime-{date_str}.{idx}.log"));
        let dst = dir.join(format!("runtime-{date_str}.{}.log", idx + 1));
        if src.exists() {
            let _ = fs::rename(&src, &dst);
        }
    }
    if base.exists() {
        let _ = fs::rename(&base, dir.join(format!("runtime-{date_str}.1.log")));
    }
}

fn run_log_writer(
    dir: PathBuf,
    retention_days: u64,
    rx: std::sync::mpsc::Receiver<LogSinkCommand>,
) {
    use std::io::BufWriter;
    let mut current_date: Option<NaiveDate> = Some(Local::now().date_naive());
    let mut current_file: Option<BufWriter<fs::File>> = None;
    let mut current_bytes: u64 = 0;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            LogSinkCommand::Line(formatted) => {
                let today = Local::now().date_naive();
                let date_str = today.format("%Y-%m-%d").to_string();
                if current_date != Some(today) {
                    if let Some(mut w) = current_file.take() {
                        let _ = w.flush();
                    }
                    prune_expired_logs(&dir, retention_days);
                    current_date = Some(today);
                    current_bytes = 0;
                }
                if current_file.is_some() && current_bytes >= LOG_FILE_MAX_BYTES {
                    if let Some(mut w) = current_file.take() {
                        let _ = w.flush();
                    }
                    rotate_daily_log(&dir, &date_str);
                    current_bytes = 0;
                }
                if current_file.is_none() {
                    let _ = fs::create_dir_all(&dir);
                    let path = dir.join(format!("runtime-{date_str}.log"));
                    if let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) {
                        current_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
                        current_file = Some(BufWriter::new(file));
                    }
                }
                if let Some(writer) = current_file.as_mut() {
                    if writeln!(writer, "{formatted}").is_ok() {
                        current_bytes = current_bytes.saturating_add(formatted.len() as u64 + 1);
                        let _ = writer.flush();
                    } else {
                        current_file = None;
                    }
                }
            }
            LogSinkCommand::Clear(done) => {
                if let Some(mut w) = current_file.take() {
                    let _ = w.flush();
                }
                current_bytes = 0;
                if let Ok(entries) = fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                            continue;
                        };
                        if (name.starts_with("runtime-") && name.ends_with(".log"))
                            || name == "panic.log"
                        {
                            let _ = fs::remove_file(path);
                        }
                    }
                }
                let _ = done.send(());
            }
            #[cfg(test)]
            LogSinkCommand::Flush(done) => {
                if let Some(writer) = current_file.as_mut() {
                    let _ = writer.flush();
                }
                let _ = done.send(());
            }
        }
    }
    if let Some(mut w) = current_file.take() {
        let _ = w.flush();
    }
}

fn prune_expired_logs(dir: &Path, retention_days: u64) {
    if retention_days == 0 {
        return;
    }
    let Some(cutoff) =
        SystemTime::now().checked_sub(Duration::from_secs(retention_days.saturating_mul(86_400)))
    else {
        return;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !(name.starts_with("runtime-") && name.ends_with(".log")) {
            continue;
        }
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
            && let Ok(modified) = meta.modified()
            && modified < cutoff
        {
            let _ = fs::remove_file(path);
        }
    }
}

#[derive(Clone)]
pub struct LogBuffer {
    inner: Arc<RwLock<BufferedLogs>>,
    events: broadcast::Sender<()>,
    file_sink: Option<Arc<LogFileSink>>,
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl LogBuffer {
    pub fn new() -> Self {
        Self::with_sink(None)
    }

    pub fn with_file_sink(dir: PathBuf, retention_days: u64) -> Self {
        Self::with_sink(Some(Arc::new(LogFileSink::new(dir, retention_days))))
    }

    fn with_sink(file_sink: Option<Arc<LogFileSink>>) -> Self {
        let (events, _) = broadcast::channel(128);
        Self {
            inner: Arc::new(RwLock::new(BufferedLogs {
                lines: VecDeque::with_capacity(LOG_CAPACITY),
                bytes: 0,
            })),
            events,
            file_sink,
        }
    }

    pub fn log_dir(&self) -> Option<&Path> {
        self.file_sink.as_deref().map(|sink| sink.dir.as_path())
    }

    pub async fn push(&self, line: impl Into<String>) {
        let line = bounded_log_line(line.into());
        {
            let mut logs = self.inner.write().await;
            logs.bytes += line.len();
            logs.lines.push_back(line.clone());
            while logs.lines.len() > LOG_CAPACITY || logs.bytes > LOG_BUFFER_BYTES {
                if let Some(line) = logs.lines.pop_front() {
                    logs.bytes -= line.len();
                }
            }
        }
        if let Some(sink) = &self.file_sink {
            sink.append_line(&line);
        }
        let _ = self.events.send(());
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.events.subscribe()
    }

    pub async fn tail(&self, max_lines: usize) -> Vec<String> {
        let logs = self.inner.read().await;
        let start = logs.lines.len().saturating_sub(max_lines.max(1));
        logs.lines.iter().skip(start).cloned().collect()
    }

    pub async fn clear(&self) {
        {
            let mut logs = self.inner.write().await;
            logs.lines.clear();
            logs.bytes = 0;
        }
        if let Some(sink) = &self.file_sink {
            sink.clear_files();
        }
        let _ = self.events.send(());
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
        Ok(PathBuf::from(strip_windows_verbatim_prefix(
            &absolute.to_string_lossy(),
        )))
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

#[cfg(test)]
mod bounded_log_tests {
    use super::*;
    #[tokio::test]
    async fn byte_budget_and_line_count_evict_oldest_entries() {
        let logs = LogBuffer::new();
        for i in 0..1000 {
            logs.push(format!("{i}:{}", "x".repeat(LOG_LINE_BYTES)))
                .await;
        }
        let snapshot = logs.tail(usize::MAX).await;
        assert!(snapshot.len() <= LOG_CAPACITY);
        assert!(snapshot.iter().all(|s| s.len() <= LOG_LINE_BYTES));
        assert!(snapshot.iter().map(|s| s.len()).sum::<usize>() <= LOG_BUFFER_BYTES);
        assert!(snapshot.last().unwrap().starts_with("999:"));
        let state = logs.inner.read().await;
        assert_eq!(state.bytes, snapshot.iter().map(|s| s.len()).sum::<usize>());
    }
    #[tokio::test]
    async fn normal_tail_and_unicode_truncation_preserve_notification() {
        let logs = LogBuffer::new();
        let mut events = logs.subscribe();
        logs.push("정상 로그").await;
        assert!(events.try_recv().is_ok());
        assert_eq!(logs.tail(1).await, vec!["정상 로그"]);
        let oversized = bounded_log_line("한글".repeat(LOG_LINE_BYTES));
        assert!(oversized.len() <= LOG_LINE_BYTES);
        assert!(oversized.ends_with(TRUNCATED));
        assert!(oversized.capacity() < LOG_LINE_BYTES * 2);
        let mut overallocated = String::with_capacity(4 * 1024 * 1024);
        overallocated.push_str("small");
        let small = bounded_log_line(overallocated);
        assert_eq!(small, "small");
        assert!(small.capacity() <= LOG_LINE_BYTES);
        for i in 0..500 {
            logs.push(i.to_string()).await;
        }
        assert_eq!(logs.tail(1000).await.len(), LOG_CAPACITY);
        assert_eq!(logs.tail(2).await, vec!["498", "499"]);
    }

    #[tokio::test]
    async fn file_sink_persists_timestamped_lines_to_daily_log() {
        let temp = tempfile::tempdir().unwrap();
        let logs = LogBuffer::with_file_sink(temp.path().to_path_buf(), 30);
        assert_eq!(logs.log_dir(), Some(temp.path()));
        logs.push("[RUST:ERR] sample error line").await;
        logs.file_sink.as_ref().unwrap().flush();
        let file_name = format!("runtime-{}.log", Local::now().format("%Y-%m-%d"));
        let log_path = temp.path().join(file_name);
        let contents = fs::read_to_string(&log_path).unwrap();
        assert!(contents.contains("[RUST:ERR] sample error line"));
        logs.clear().await;
        assert!(logs.tail(10).await.is_empty());
        assert!(!log_path.exists());
    }
}
