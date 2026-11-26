//! Lossless post-recording MPEG-TS remux service (`ffmpeg -c copy`).
//!
//! Remuxes completed `.ts` LIVE recordings into `.mp4` or `.mkv` in the background
//! using an owned subprocess (`media_process::run_media_process`) that cancels
//! cleanly on application shutdown. The original `.ts` file is only removed after
//! atomic staging rename succeeds and `delete_source` is explicitly enabled.

use crate::{
    backend::LogBuffer,
    media_process::{MediaCancellation, MediaProcessOutcome, MediaProcessSpec, run_media_process},
    recorder::resolve_timestamp_rebase_ffmpeg,
    store::Store,
    tool_discovery::{ToolKind, resolve_tool},
};
use anyhow::{Context, Result, anyhow, bail};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex, Semaphore},
    task::JoinHandle,
};
use uuid::Uuid;

const MAX_CONCURRENT_REMUX_JOBS: usize = 2;
const SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RemuxFormat {
    #[default]
    Off,
    Mp4,
    Mkv,
}

impl RemuxFormat {
    pub fn from_setting(raw: &str) -> Self {
        match raw.trim().to_ascii_uppercase().as_str() {
            "MP4" => Self::Mp4,
            "MKV" => Self::Mkv,
            _ => Self::Off,
        }
    }

    pub fn extension(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Mp4 => Some("mp4"),
            Self::Mkv => Some("mkv"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Mp4 => "MP4",
            Self::Mkv => "MKV",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemuxJob {
    pub source: PathBuf,
    pub history_id: String,
    pub format: RemuxFormat,
    pub delete_source: bool,
    pub ffmpeg_path: String,
    pub streamlink_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemuxOutcome {
    pub output_path: PathBuf,
    pub size_bytes: u64,
    pub source_deleted: bool,
}

struct ActiveJobGuard(Arc<AtomicUsize>);

impl Drop for ActiveJobGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Clone)]
pub struct RemuxManager {
    backend_dir: Arc<PathBuf>,
    logs: LogBuffer,
    cancel: MediaCancellation,
    semaphore: Arc<Semaphore>,
    active: Arc<AtomicUsize>,
    tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl RemuxManager {
    pub fn new(backend_dir: PathBuf, logs: LogBuffer) -> Self {
        Self {
            backend_dir: Arc::new(backend_dir),
            logs,
            cancel: MediaCancellation::new(),
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_REMUX_JOBS)),
            active: Arc::new(AtomicUsize::new(0)),
            tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn active_count(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    /// Schedule a background lossless remux when enabled and source is a `.ts` file.
    pub fn enqueue(&self, job: RemuxJob, store: Option<Store>) {
        if job.format == RemuxFormat::Off || self.cancel.is_cancelled() || !is_ts_path(&job.source)
        {
            return;
        }
        let backend_dir = self.backend_dir.clone();
        let logs = self.logs.clone();
        let cancel = self.cancel.clone();
        let semaphore = self.semaphore.clone();
        let active = self.active.clone();
        let tasks = self.tasks.clone();
        active.fetch_add(1, Ordering::AcqRel);

        let handle = tokio::spawn(async move {
            let _active_guard = ActiveJobGuard(active);
            let Ok(_permit) = semaphore.acquire_owned().await else {
                return;
            };
            if cancel.is_cancelled() {
                return;
            }
            if let Err(err) =
                execute_remux(backend_dir.as_path(), &job, store.as_ref(), &logs, cancel).await
            {
                logs.push(format!("[REMUX:ERR] {}: {err:#}", file_label(&job.source)))
                    .await;
            }
        });

        let tasks_slot = tasks.clone();
        tokio::spawn(async move {
            let mut list = tasks_slot.lock().await;
            list.retain(|existing| !existing.is_finished());
            list.push(handle);
        });
    }

    /// Cancel all active remux processes and await clean termination.
    pub async fn shutdown(&self) {
        self.cancel.cancel();
        let handles = {
            let mut list = self.tasks.lock().await;
            std::mem::take(&mut *list)
        };
        for mut handle in handles {
            if tokio::time::timeout(SHUTDOWN_JOIN_TIMEOUT, &mut handle)
                .await
                .is_err()
            {
                handle.abort();
                let _ = handle.await;
            }
        }
    }
}

pub fn is_ts_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ts"))
}

pub fn build_remux_args(source: &Path, staging: &Path, format: RemuxFormat) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        source.as_os_str().to_os_string(),
        "-map".into(),
        "0:v?".into(),
        "-map".into(),
        "0:a?".into(),
        "-c".into(),
        "copy".into(),
    ];
    if format == RemuxFormat::Mp4 {
        args.push("-movflags".into());
        args.push("+faststart".into());
    }
    args.push(staging.as_os_str().to_os_string());
    args
}

pub fn unique_remux_target(source: &Path, extension: &str) -> Result<PathBuf> {
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    let stem = source
        .file_stem()
        .ok_or_else(|| anyhow!("remux source has no file stem: {}", source.display()))?
        .to_os_string();
    let mut first_name = stem.clone();
    first_name.push(format!(".{extension}"));
    let first = parent.join(first_name);
    if !first.exists() {
        return Ok(first);
    }
    for number in 2..=9999 {
        let mut candidate_name = stem.clone();
        candidate_name.push(format!("_{number:02}.{extension}"));
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!(
        "too many remux output filename collisions for {}",
        source.display()
    )
}

fn staging_remux_path(source: &Path, extension: &str) -> Result<PathBuf> {
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    let stem = source
        .file_stem()
        .ok_or_else(|| anyhow!("remux source has no file stem: {}", source.display()))?
        .to_os_string();
    let suffix = &Uuid::new_v4().simple().to_string()[..8];
    let mut name = stem;
    name.push(format!(".remuxing-{suffix}.{extension}"));
    Ok(parent.join(name))
}

fn resolve_remux_ffmpeg(
    backend_dir: &Path,
    configured_ffmpeg: &str,
    streamlink_path: &Path,
) -> Option<PathBuf> {
    resolve_tool(
        ToolKind::Ffmpeg,
        backend_dir,
        &[("FFMPEG_PATH", configured_ffmpeg)],
    )
    .path
    .or_else(|| resolve_timestamp_rebase_ffmpeg(streamlink_path).ok())
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

pub(crate) async fn execute_remux(
    backend_dir: &Path,
    job: &RemuxJob,
    store: Option<&Store>,
    logs: &LogBuffer,
    cancel: MediaCancellation,
) -> Result<Option<RemuxOutcome>> {
    let Some(extension) = job.format.extension() else {
        return Ok(None);
    };
    if !is_ts_path(&job.source) {
        return Ok(None);
    }
    let source_meta = match fs::metadata(&job.source) {
        Ok(meta) if meta.is_file() && meta.len() > 0 => meta,
        _ => return Ok(None),
    };
    let Some(ffmpeg) = resolve_remux_ffmpeg(backend_dir, &job.ffmpeg_path, &job.streamlink_path)
    else {
        logs.push(format!(
            "[REMUX:WARN] FFmpeg not found; keeping original .ts ({})",
            file_label(&job.source)
        ))
        .await;
        return Ok(None);
    };

    let staging = staging_remux_path(&job.source, extension)?;
    let args = build_remux_args(&job.source, &staging, job.format);
    logs.push(format!(
        "[REMUX] started format={} source={} ({} bytes)",
        job.format.as_str(),
        file_label(&job.source),
        source_meta.len()
    ))
    .await;

    let result = run_media_process(
        MediaProcessSpec::new(&ffmpeg).args(args).without_timeout(),
        Some(cancel),
    )
    .await;

    let result = match result {
        Ok(result) => result,
        Err(err) => {
            let _ = fs::remove_file(&staging);
            return Err(err).context("failed to execute FFmpeg remux");
        }
    };

    match result.outcome {
        MediaProcessOutcome::Success => {}
        MediaProcessOutcome::Cancelled => {
            let _ = fs::remove_file(&staging);
            logs.push(format!(
                "[REMUX:WARN] cancelled {}; original .ts preserved",
                file_label(&job.source)
            ))
            .await;
            return Ok(None);
        }
        MediaProcessOutcome::ProcessFailure => {
            let _ = fs::remove_file(&staging);
            bail!(
                "FFmpeg remux failed (exit={:?}): {}",
                result.exit_code,
                result.stderr.text.trim()
            );
        }
        MediaProcessOutcome::Timeout => {
            let _ = fs::remove_file(&staging);
            bail!("FFmpeg remux unexpectedly timed out");
        }
        MediaProcessOutcome::SpawnFailure => {
            let _ = fs::remove_file(&staging);
            bail!(
                "FFmpeg remux spawn failed: {}",
                result
                    .spawn_error
                    .unwrap_or_else(|| "unknown spawn failure".into())
            );
        }
    }

    let staged_size = fs::metadata(&staging)
        .map(|meta| if meta.is_file() { meta.len() } else { 0 })
        .unwrap_or(0);
    if staged_size == 0 {
        let _ = fs::remove_file(&staging);
        bail!("FFmpeg remux produced an empty output file; original .ts preserved");
    }

    let target = unique_remux_target(&job.source, extension)?;
    if let Err(err) = fs::rename(&staging, &target) {
        let _ = fs::remove_file(&staging);
        return Err(err).with_context(|| {
            format!(
                "failed to finalize remux file {} -> {}",
                staging.display(),
                target.display()
            )
        });
    }

    if let Some(db) = store
        && !job.history_id.is_empty()
        && let Err(err) =
            db.update_live_file(&job.history_id, &target.display().to_string(), staged_size)
    {
        logs.push(format!(
            "[REMUX:WARN] SQLite live history update failed id={}: {err:#}",
            job.history_id
        ))
        .await;
    }

    let mut source_deleted = false;
    if job.delete_source {
        match fs::remove_file(&job.source) {
            Ok(()) => source_deleted = true,
            Err(err) => {
                logs.push(format!(
                    "[REMUX:WARN] remux succeeded but failed to delete source {}: {err}",
                    file_label(&job.source)
                ))
                .await;
            }
        }
    }

    logs.push(format!(
        "[REMUX] completed {} -> {} ({staged_size} bytes, delete_source={source_deleted})",
        file_label(&job.source),
        file_label(&target)
    ))
    .await;

    Ok(Some(RemuxOutcome {
        output_path: target,
        size_bytes: staged_size,
        source_deleted,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LiveHistoryItem;
    use crate::support::platform::PlatformId;

    #[test]
    fn remux_format_parses_case_insensitively_and_defaults_off() {
        assert_eq!(RemuxFormat::from_setting("mp4"), RemuxFormat::Mp4);
        assert_eq!(RemuxFormat::from_setting(" MKV "), RemuxFormat::Mkv);
        assert_eq!(RemuxFormat::from_setting("OFF"), RemuxFormat::Off);
        assert_eq!(RemuxFormat::from_setting(""), RemuxFormat::Off);
        assert_eq!(RemuxFormat::from_setting("avi"), RemuxFormat::Off);
        assert_eq!(RemuxFormat::Mp4.extension(), Some("mp4"));
        assert_eq!(RemuxFormat::Mkv.extension(), Some("mkv"));
        assert_eq!(RemuxFormat::Off.extension(), None);
    }

    #[test]
    fn remux_args_use_stream_copy_and_faststart_for_mp4_only() {
        let src = Path::new("sample.ts");
        let dst_mp4 = Path::new("sample.remuxing.mp4");
        let mp4_args: Vec<String> = build_remux_args(src, dst_mp4, RemuxFormat::Mp4)
            .into_iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert!(mp4_args.windows(2).any(|w| w == ["-c", "copy"]));
        assert!(mp4_args.windows(2).any(|w| w == ["-map", "0:v?"]));
        assert!(mp4_args.windows(2).any(|w| w == ["-map", "0:a?"]));
        assert!(
            mp4_args
                .windows(2)
                .any(|w| w == ["-movflags", "+faststart"])
        );
        assert_eq!(
            mp4_args.last().map(String::as_str),
            Some("sample.remuxing.mp4")
        );

        let dst_mkv = Path::new("sample.remuxing.mkv");
        let mkv_args: Vec<String> = build_remux_args(src, dst_mkv, RemuxFormat::Mkv)
            .into_iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert!(mkv_args.windows(2).any(|w| w == ["-c", "copy"]));
        assert!(!mkv_args.iter().any(|arg| arg == "-movflags"));
        assert_eq!(
            mkv_args.last().map(String::as_str),
            Some("sample.remuxing.mkv")
        );
    }

    #[test]
    fn unique_remux_target_avoids_existing_files_with_numeric_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("261126_120000_channel.ts");
        fs::write(&src, b"ts-bytes").unwrap();

        let first = unique_remux_target(&src, "mp4").unwrap();
        assert_eq!(first, dir.path().join("261126_120000_channel.mp4"));
        fs::write(&first, b"existing").unwrap();

        let second = unique_remux_target(&src, "mp4").unwrap();
        assert_eq!(second, dir.path().join("261126_120000_channel_02.mp4"));
    }

    #[tokio::test]
    async fn execute_remux_skips_off_non_ts_and_empty_files() {
        let dir = tempfile::tempdir().unwrap();
        let logs = LogBuffer::new();
        let ts = dir.path().join("empty.ts");
        fs::write(&ts, b"").unwrap();
        let job = RemuxJob {
            source: ts,
            history_id: String::new(),
            format: RemuxFormat::Mp4,
            delete_source: true,
            ffmpeg_path: String::new(),
            streamlink_path: PathBuf::new(),
        };
        assert!(
            execute_remux(dir.path(), &job, None, &logs, MediaCancellation::new())
                .await
                .unwrap()
                .is_none()
        );

        let mp4 = dir.path().join("already.mp4");
        fs::write(&mp4, b"data").unwrap();
        let job_mp4 = RemuxJob {
            source: mp4,
            ..job.clone()
        };
        assert!(
            execute_remux(dir.path(), &job_mp4, None, &logs, MediaCancellation::new())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn execute_remux_success_updates_sqlite_history_and_respects_delete_source() {
        let dir = tempfile::tempdir().unwrap();
        let logs = LogBuffer::new();
        let db = Store::open(dir.path().join("stream-archive.db")).unwrap();

        let fake_ffmpeg = create_mock_ffmpeg(dir.path(), true);

        // Case 1: delete_source = false preserves original .ts and updates DB
        let ts1 = dir.path().join("stream_keep.ts");
        fs::write(&ts1, b"mock-mpeg-ts-payload").unwrap();
        db.start_live(&LiveHistoryItem {
            platform: PlatformId::Soop,
            id: "live-1".into(),
            account: "tester".into(),
            channel_name: "Tester".into(),
            file_path: Some(ts1.display().to_string()),
            started_at: "2026-11-26T00:00:00Z".into(),
            ..Default::default()
        })
        .unwrap();
        db.finish_live(
            "live-1",
            "2026-11-26T00:10:00Z",
            600,
            20,
            "BROADCAST ENDED",
            "COMPLETED",
        )
        .unwrap();

        let outcome1 = execute_remux(
            dir.path(),
            &RemuxJob {
                source: ts1.clone(),
                history_id: "live-1".into(),
                format: RemuxFormat::Mp4,
                delete_source: false,
                ffmpeg_path: fake_ffmpeg.display().to_string(),
                streamlink_path: PathBuf::new(),
            },
            Some(&db),
            &logs,
            MediaCancellation::new(),
        )
        .await
        .unwrap()
        .expect("remux outcome");
        assert!(ts1.exists());
        assert!(outcome1.output_path.is_file());
        assert!(!outcome1.source_deleted);
        assert_eq!(
            outcome1.output_path.extension().and_then(|e| e.to_str()),
            Some("mp4")
        );
        let history = crate::history_service::load_history(db.path(), &Default::default()).unwrap();
        assert_eq!(
            history.live[0].file_path.as_deref(),
            Some(outcome1.output_path.to_string_lossy().as_ref())
        );
        assert_eq!(history.live[0].size_bytes, outcome1.size_bytes);

        // Case 2: delete_source = true deletes original .ts only after success
        let ts2 = dir.path().join("stream_delete.ts");
        fs::write(&ts2, b"mock-mpeg-ts-payload-2").unwrap();
        let outcome2 = execute_remux(
            dir.path(),
            &RemuxJob {
                source: ts2.clone(),
                history_id: String::new(),
                format: RemuxFormat::Mkv,
                delete_source: true,
                ffmpeg_path: fake_ffmpeg.display().to_string(),
                streamlink_path: PathBuf::new(),
            },
            None,
            &logs,
            MediaCancellation::new(),
        )
        .await
        .unwrap()
        .expect("remux mkv outcome");
        assert!(!ts2.exists());
        assert!(outcome2.output_path.is_file());
        assert!(outcome2.source_deleted);
        assert_eq!(
            outcome2.output_path.extension().and_then(|e| e.to_str()),
            Some("mkv")
        );
    }

    #[tokio::test]
    async fn execute_remux_failure_cleans_staging_and_preserves_source() {
        let dir = tempfile::tempdir().unwrap();
        let logs = LogBuffer::new();
        let failing_ffmpeg = create_mock_ffmpeg(dir.path(), false);
        let ts = dir.path().join("stream_fail.ts");
        fs::write(&ts, b"keep-me-intact").unwrap();

        let err = execute_remux(
            dir.path(),
            &RemuxJob {
                source: ts.clone(),
                history_id: String::new(),
                format: RemuxFormat::Mp4,
                delete_source: true,
                ffmpeg_path: failing_ffmpeg.display().to_string(),
                streamlink_path: PathBuf::new(),
            },
            None,
            &logs,
            MediaCancellation::new(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("FFmpeg remux failed"));
        assert!(ts.exists(), "source .ts must stay intact on remux failure");
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".remuxing-"))
            .collect();
        assert!(leftovers.is_empty(), "staging file must be cleaned up");
    }

    #[cfg(windows)]
    fn create_mock_ffmpeg(dir: &Path, succeed: bool) -> PathBuf {
        let name = if succeed {
            "mock-ffmpeg-ok.cmd"
        } else {
            "mock-ffmpeg-fail.cmd"
        };
        let path = dir.join(name);
        let script = if succeed {
            "@echo off\r\nsetlocal\r\nset \"last=\"\r\n:loop\r\nif \"%~1\"==\"\" goto done\r\nset \"last=%~1\"\r\nshift\r\ngoto loop\r\n:done\r\necho remuxed-payload>\"%last%\"\r\nexit /b 0\r\n"
        } else {
            "@echo off\r\nsetlocal\r\nset \"last=\"\r\n:loop\r\nif \"%~1\"==\"\" goto done\r\nset \"last=%~1\"\r\nshift\r\ngoto loop\r\n:done\r\necho partial>\"%last%\"\r\necho simulated mux error 1>&2\r\nexit /b 1\r\n"
        };
        fs::write(&path, script).unwrap();
        path
    }

    #[cfg(unix)]
    fn create_mock_ffmpeg(dir: &Path, succeed: bool) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let name = if succeed {
            "mock-ffmpeg-ok.sh"
        } else {
            "mock-ffmpeg-fail.sh"
        };
        let path = dir.join(name);
        let script = if succeed {
            "#!/bin/sh\nfor last; do :; done\nprintf 'remuxed-payload' > \"$last\"\nexit 0\n"
        } else {
            "#!/bin/sh\nfor last; do :; done\nprintf 'partial' > \"$last\"\necho 'simulated mux error' >&2\nexit 1\n"
        };
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
}
