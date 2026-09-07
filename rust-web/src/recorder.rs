use crate::{
    backend::LogBuffer,
    model::LiveHistoryItem,
    store,
};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{io::{AsyncBufReadExt, BufReader}, process::{Child, Command}};
use uuid::Uuid;

const GB: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct RecorderConfig {
    pub streamlink: PathBuf,
    pub quality: String,
    pub stall_timeout: u64,
    pub monitor_interval: u64,
    pub min_free_space_gb: f64,
}

pub struct Recording {
    pub pid: u32,
    pub bno: String,
    pub title: String,
    pub file: PathBuf,
    pub started_at: DateTime<Utc>,
    pub history_id: String,
    child: Child,
    output_dir: PathBuf,
    last_size: u64,
    last_growth: Instant,
    last_monitor: Instant,
}

pub enum RecordingPoll {
    Running,
    Exited(Option<i32>),
    LowDisk(f64),
    Stalled,
}

#[derive(Clone)]
pub struct RecorderManager {
    logs: LogBuffer,
}

impl RecorderManager {
    pub fn new(logs: LogBuffer) -> Self {
        Self { logs }
    }

    pub async fn start(
        &self,
        config: &RecorderConfig,
        stream_url: &str,
        output_file: PathBuf,
        bno: String,
        title: String,
        channel: &str,
        account: &str,
    ) -> Result<Recording> {
        let output_dir = output_file.parent().ok_or_else(|| anyhow!("output file has no parent"))?.to_path_buf();
        fs::create_dir_all(&output_dir)?;
        let free = free_gb(&output_dir)?;
        if free < config.min_free_space_gb {
            bail!("LOW DISK SPACE - free={free:.2}GB limit={:.2}GB", config.min_free_space_gb);
        }

        let mut command = Command::new(&config.streamlink);
        command
            .arg(stream_url)
            .arg(&config.quality)
            .arg("--output").arg(&output_file)
            .arg("--force")
            .arg("--hls-live-edge").arg("3")
            .arg("--stream-segment-threads").arg("3")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(false);
        if let Some(parent) = config.streamlink.parent() {
            if parent.is_dir() { command.current_dir(parent); }
        }

        let mut child = command.spawn().with_context(|| format!("failed to start Streamlink: {}", config.streamlink.display()))?;
        let pid = child.id().ok_or_else(|| anyhow!("Streamlink PID unavailable"))?;
        if let Some(stderr) = child.stderr.take() {
            let logs = self.logs.clone();
            let account = account.to_string();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line.trim();
                    if !line.is_empty() {
                        logs.push(format!("[RUST:STREAMLINK:{account}] {line}")).await;
                    }
                }
            });
        }

        let started_at = Utc::now();
        let history_id = Uuid::new_v4().to_string();
        if let Ok(db) = store::global() {
            if let Err(err) = db.start_live(&LiveHistoryItem {
                id: history_id.clone(),
                account: account.to_string(),
                channel_name: channel.to_string(),
                bno: Some(bno.clone()),
                title: Some(title.clone()),
                file_path: Some(output_file.display().to_string()),
                started_at: started_at.to_rfc3339(),
                ended_at: None,
                duration_seconds: 0,
                size_bytes: 0,
                reason: None,
                status: "RECORDING".into(),
            }) {
                self.logs.push(format!("[DB:WARN] LIVE history start failed: {err:#}")).await;
            }
        }

        self.logs.push(format!(
            "[RUST] RECORD START channel={channel} account={account} bno={bno} pid={pid} file={}",
            output_file.display()
        )).await;

        Ok(Recording {
            pid,
            bno,
            title,
            file: output_file,
            started_at,
            history_id,
            child,
            output_dir,
            last_size: 0,
            last_growth: Instant::now(),
            last_monitor: Instant::now(),
        })
    }

    pub fn poll(&self, rec: &mut Recording, config: &RecorderConfig) -> Result<RecordingPoll> {
        if let Some(status) = rec.child.try_wait().context("failed to query Streamlink status")? {
            return Ok(RecordingPoll::Exited(status.code()));
        }
        if rec.last_monitor.elapsed() < Duration::from_secs(config.monitor_interval.max(1)) {
            return Ok(RecordingPoll::Running);
        }
        rec.last_monitor = Instant::now();

        let free = free_gb(&rec.output_dir)?;
        if free < config.min_free_space_gb {
            return Ok(RecordingPoll::LowDisk(free));
        }

        let size = fs::metadata(&rec.file).map(|m| m.len()).unwrap_or(0);
        if size > rec.last_size {
            rec.last_size = size;
            rec.last_growth = Instant::now();
            return Ok(RecordingPoll::Running);
        }
        if rec.last_growth.elapsed() >= Duration::from_secs(config.stall_timeout.max(10)) {
            return Ok(RecordingPoll::Stalled);
        }
        Ok(RecordingPoll::Running)
    }

    pub async fn stop(&self, rec: &mut Recording) -> Result<Option<i32>> {
        if rec.child.try_wait()?.is_none() {
            #[cfg(windows)]
            {
                let status = Command::new("taskkill.exe")
                    .arg("/PID").arg(rec.pid.to_string()).arg("/T").arg("/F")
                    .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
                    .status().await?;
                if !status.success() && rec.child.try_wait()?.is_none() {
                    bail!("taskkill failed for recorder pid={}", rec.pid);
                }
            }
            #[cfg(not(windows))]
            {
                let _ = rec.child.kill().await;
            }
        }
        Ok(rec.child.wait().await.ok().and_then(|s| s.code()))
    }

    pub async fn log_finished(&self, channel: &str, account: &str, rec: &Recording, reason: &str) {
        let size = fs::metadata(&rec.file).map(|m| m.len()).unwrap_or(rec.last_size);
        let ended_at = Utc::now();
        let secs = (ended_at - rec.started_at).num_seconds().max(0);
        let status = if reason == "NORMAL" { "COMPLETED" } else if reason.contains("STALLED") || reason.contains("EXIT CODE") { "FAILED" } else { "STOPPED" };
        if let Ok(db) = store::global() {
            if let Err(err) = db.finish_live(
                &rec.history_id,
                &ended_at.to_rfc3339(),
                secs,
                size,
                reason,
                status,
            ) {
                self.logs.push(format!("[DB:WARN] LIVE history finish failed: {err:#}")).await;
            }
        }
        self.logs.push(format!(
            "[RUST] RECORD FINISHED channel={channel} account={account} duration={secs}s size={size} reason={reason} file={}",
            rec.file.display()
        )).await;
    }
}

pub fn free_gb(path: &Path) -> Result<f64> {
    Ok(fs2::available_space(path)? as f64 / GB as f64)
}
