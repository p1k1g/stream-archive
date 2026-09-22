//! Deterministic local media-tool process execution.
//!
//! This boundary owns only subprocess lifecycle. Provider network behavior and
//! media command construction remain with the existing LIVE/VOD implementations.

use crate::{
    platform_runtime::spawn_owned,
    tool_discovery::ToolKind,
};
use anyhow::{Context, Result};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    task::JoinHandle,
};

const POLL_INTERVAL: Duration = Duration::from_millis(25);
pub const DEFAULT_CAPTURE_LIMIT: usize = 64 * 1024;
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(4);

pub struct MediaProcessSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: Option<PathBuf>,
    pub environment: BTreeMap<OsString, OsString>,
    pub timeout: Duration,
    pub capture_limit: usize,
}

impl MediaProcessSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            current_dir: None,
            environment: BTreeMap::new(),
            timeout: Duration::from_secs(30),
            capture_limit: DEFAULT_CAPTURE_LIMIT,
        }
    }

    pub fn arg(mut self, value: impl Into<OsString>) -> Self {
        self.args.push(value.into());
        self
    }

    pub fn args<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(values.into_iter().map(Into::into));
        self
    }

    pub fn current_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(path.into());
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.environment.insert(key.into(), value.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn capture_limit(mut self, capture_limit: usize) -> Self {
        self.capture_limit = capture_limit.max(1);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaProcessOutcome {
    Success,
    ProcessFailure,
    Timeout,
    Cancelled,
    SpawnFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedOutput {
    pub text: String,
    pub truncated: bool,
}

impl CapturedOutput {
    fn empty() -> Self {
        Self {
            text: String::new(),
            truncated: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MediaProcessResult {
    pub outcome: MediaProcessOutcome,
    pub exit_code: Option<i32>,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
    pub duration: Duration,
    pub spawn_error: Option<String>,
}

impl MediaProcessResult {
    pub fn success(&self) -> bool {
        self.outcome == MediaProcessOutcome::Success
    }
}

#[derive(Clone, Default)]
pub struct MediaCancellation {
    cancelled: Arc<AtomicBool>,
}

impl MediaCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

pub async fn run_media_process(
    spec: MediaProcessSpec,
    cancellation: Option<MediaCancellation>,
) -> Result<MediaProcessResult> {
    let started = Instant::now();
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if let Some(current_dir) = &spec.current_dir {
        command.current_dir(current_dir);
    }
    command.envs(spec.environment.iter());

    let (mut child, mut owned_tree) = match spawn_owned(&mut command).await {
        Ok(value) => value,
        Err(error) => {
            return Ok(MediaProcessResult {
                outcome: MediaProcessOutcome::SpawnFailure,
                exit_code: None,
                stdout: CapturedOutput::empty(),
                stderr: CapturedOutput::empty(),
                duration: started.elapsed(),
                spawn_error: Some(format!(
                    "failed to start media tool {}: {error:#}",
                    spec.program.display()
                )),
            });
        }
    };

    let stdout = child
        .stdout
        .take()
        .context("owned media process stdout pipe is unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("owned media process stderr pipe is unavailable")?;
    let stdout_task = tokio::spawn(capture_bounded(stdout, spec.capture_limit));
    let stderr_task = tokio::spawn(capture_bounded(stderr, spec.capture_limit));

    let deadline = started + spec.timeout;
    let mut outcome = MediaProcessOutcome::Success;
    let exit_code = loop {
        if cancellation
            .as_ref()
            .is_some_and(MediaCancellation::is_cancelled)
        {
            outcome = MediaProcessOutcome::Cancelled;
            break owned_tree.terminate(&mut child).await?;
        }

        if let Some(status) = child.try_wait()? {
            if !status.success() {
                outcome = MediaProcessOutcome::ProcessFailure;
            }
            owned_tree.terminate_now()?;
            let _ = child.wait().await;
            break status.code();
        }

        if Instant::now() >= deadline {
            outcome = MediaProcessOutcome::Timeout;
            break owned_tree.terminate(&mut child).await?;
        }

        tokio::time::sleep(POLL_INTERVAL).await;
    };

    let stdout = join_capture(stdout_task, "stdout").await?;
    let stderr = join_capture(stderr_task, "stderr").await?;

    Ok(MediaProcessResult {
        outcome,
        exit_code,
        stdout,
        stderr,
        duration: started.elapsed(),
        spawn_error: None,
    })
}

async fn join_capture(
    task: JoinHandle<std::io::Result<CapturedOutput>>,
    stream: &str,
) -> Result<CapturedOutput> {
    task.await
        .with_context(|| format!("{stream} capture task did not complete"))?
        .with_context(|| format!("{stream} capture failed"))
}

async fn capture_bounded<R>(mut reader: R, limit: usize) -> std::io::Result<CapturedOutput>
where
    R: AsyncRead + Unpin,
{
    let mut tail = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0u8; 8192];
    let mut truncated = false;

    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        append_tail(&mut tail, &buffer[..count], limit, &mut truncated);
    }

    Ok(CapturedOutput {
        text: String::from_utf8_lossy(&tail).into_owned(),
        truncated,
    })
}

fn append_tail(tail: &mut Vec<u8>, input: &[u8], limit: usize, truncated: &mut bool) {
    if input.len() >= limit {
        tail.clear();
        tail.extend_from_slice(&input[input.len() - limit..]);
        *truncated = true;
        return;
    }

    let overflow = tail.len().saturating_add(input.len()).saturating_sub(limit);
    if overflow > 0 {
        tail.drain(..overflow);
        *truncated = true;
    }
    tail.extend_from_slice(input);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaToolProbeStatus {
    Ok,
    ProcessFailure,
    Timeout,
    Cancelled,
    SpawnFailure,
    InvalidVersion,
}

#[derive(Debug, Clone)]
pub struct MediaToolProbe {
    pub kind: ToolKind,
    pub status: MediaToolProbeStatus,
    pub version: Option<String>,
    pub exit_code: Option<i32>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub detail: String,
}

pub async fn probe_tool_version(
    kind: ToolKind,
    executable: &Path,
    timeout: Duration,
) -> Result<MediaToolProbe> {
    probe_tool_version_with_environment(kind, executable, timeout, BTreeMap::new()).await
}

async fn probe_tool_version_with_environment(
    kind: ToolKind,
    executable: &Path,
    timeout: Duration,
    environment: BTreeMap<OsString, OsString>,
) -> Result<MediaToolProbe> {
    let version_arg: &OsStr = match kind {
        ToolKind::Ffmpeg => OsStr::new("-version"),
        ToolKind::Streamlink | ToolKind::YtDlp => OsStr::new("--version"),
    };
    let mut spec = MediaProcessSpec::new(executable)
        .arg(version_arg)
        .timeout(timeout)
        .capture_limit(16 * 1024);
    spec.environment = environment;
    let result = run_media_process(spec, None).await?;

    let status = match result.outcome {
        MediaProcessOutcome::Success => MediaToolProbeStatus::Ok,
        MediaProcessOutcome::ProcessFailure => MediaToolProbeStatus::ProcessFailure,
        MediaProcessOutcome::Timeout => MediaToolProbeStatus::Timeout,
        MediaProcessOutcome::Cancelled => MediaToolProbeStatus::Cancelled,
        MediaProcessOutcome::SpawnFailure => MediaToolProbeStatus::SpawnFailure,
    };

    if status != MediaToolProbeStatus::Ok {
        return Ok(MediaToolProbe {
            kind,
            status,
            version: None,
            exit_code: result.exit_code,
            stdout_truncated: result.stdout.truncated,
            stderr_truncated: result.stderr.truncated,
            detail: probe_failure_detail(&result),
        });
    }

    let version = first_non_empty_line(&result.stdout.text)
        .or_else(|| first_non_empty_line(&result.stderr.text))
        .map(str::to_owned);
    let Some(version) = version else {
        return Ok(MediaToolProbe {
            kind,
            status: MediaToolProbeStatus::InvalidVersion,
            version: None,
            exit_code: result.exit_code,
            stdout_truncated: result.stdout.truncated,
            stderr_truncated: result.stderr.truncated,
            detail: "version probe exited successfully but produced no non-empty version line".into(),
        });
    };

    Ok(MediaToolProbe {
        kind,
        status: MediaToolProbeStatus::Ok,
        version: Some(version.clone()),
        exit_code: result.exit_code,
        stdout_truncated: result.stdout.truncated,
        stderr_truncated: result.stderr.truncated,
        detail: format!("{} version probe succeeded: {version}", kind.label()),
    })
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

fn probe_failure_detail(result: &MediaProcessResult) -> String {
    if let Some(error) = &result.spawn_error {
        return error.clone();
    }
    let stderr = result.stderr.text.trim();
    if stderr.is_empty() {
        format!(
            "media tool probe ended with {:?} (exit={:?})",
            result.outcome, result.exit_code
        )
    } else {
        format!(
            "media tool probe ended with {:?} (exit={:?}); stderr tail: {}",
            result.outcome, result.exit_code, stderr
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform_runtime::test_process_exists;
    use std::{
        fs,
        process::{Child as StdChild, Command as StdCommand, Stdio},
        sync::OnceLock,
    };
    use tempfile::TempDir;
    use uuid::Uuid;

    static FIXTURE: OnceLock<PathBuf> = OnceLock::new();

    fn fixture_path() -> PathBuf {
        FIXTURE
            .get_or_init(|| {
                let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                let source = manifest.join("tests/fixtures/media_tool_fixture.rs");
                let output_dir = manifest.join("target/test-fixtures");
                fs::create_dir_all(&output_dir).unwrap();
                let suffix = if cfg!(windows) { ".exe" } else { "" };
                let output = output_dir.join(format!("media-tool-fixture{suffix}"));
                let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
                let status = StdCommand::new(rustc)
                    .arg("--edition=2024")
                    .arg(&source)
                    .arg("-o")
                    .arg(&output)
                    .status()
                    .unwrap();
                assert!(status.success(), "failed to compile media-tool fixture");
                output
            })
            .clone()
    }

    fn spec(command: &str) -> MediaProcessSpec {
        MediaProcessSpec::new(fixture_path())
            .arg(command)
            .timeout(Duration::from_secs(10))
    }

    fn temp_unicode_dir() -> TempDir {
        tempfile::Builder::new()
            .prefix("Stream Archive 한글 ")
            .tempdir()
            .unwrap()
    }

    async fn wait_for_file(path: &Path) {
        let started = Instant::now();
        while !path.exists() {
            assert!(
                started.elapsed() < Duration::from_secs(8),
                "timed out waiting for fixture marker {}",
                path.display()
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    fn spawn_unrelated(ready: &Path) -> StdChild {
        StdCommand::new(fixture_path())
            .args(["sleep", "30000"])
            .arg(ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    #[tokio::test]
    async fn spawn_preserves_argument_boundaries_and_unicode_output() {
        let value = "C:\\Stream Archive\\한글 폴더\\테스트-영상-🎬.mp4";
        let result = run_media_process(spec("echo-args").arg(value), None)
            .await
            .unwrap();
        assert_eq!(result.outcome, MediaProcessOutcome::Success);
        assert!(result.stdout.text.contains(value));
        assert!(!result.stdout.truncated);
    }

    #[tokio::test]
    async fn stdout_stderr_and_nonzero_exit_are_structured() {
        let result = run_media_process(
            spec("emit")
                .args(["hello-out", "hello-err", "7"]),
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.outcome, MediaProcessOutcome::ProcessFailure);
        assert_eq!(result.exit_code, Some(7));
        assert!(result.stdout.text.contains("hello-out"));
        assert!(result.stderr.text.contains("hello-err"));
    }

    #[tokio::test]
    async fn missing_executable_is_structured_spawn_failure() {
        let result = run_media_process(
            MediaProcessSpec::new(PathBuf::from("definitely-missing-stream-archive-media-tool"))
                .timeout(Duration::from_secs(1)),
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.outcome, MediaProcessOutcome::SpawnFailure);
        assert!(result.spawn_error.is_some());
    }

    #[tokio::test]
    async fn large_concurrent_output_is_bounded_and_keeps_tail() {
        let result = run_media_process(
            spec("large-output")
                .arg("131072")
                .capture_limit(4096),
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.outcome, MediaProcessOutcome::Success);
        assert!(result.stdout.truncated);
        assert!(result.stderr.truncated);
        assert!(result.stdout.text.ends_with("STDOUT-END\n"));
        assert!(result.stderr.text.ends_with("STDERR-END\n"));
        assert!(result.stdout.text.len() <= 4096 * 3);
        assert!(result.stderr.text.len() <= 4096 * 3);
    }

    #[tokio::test]
    async fn invalid_utf8_is_lossy_not_fatal() {
        let result = run_media_process(spec("invalid-utf8"), None)
            .await
            .unwrap();
        assert_eq!(result.outcome, MediaProcessOutcome::Success);
        assert!(result.stdout.text.contains('\u{fffd}'));
        assert!(result.stderr.text.contains('\u{fffd}'));
    }

    #[tokio::test]
    async fn working_directory_and_environment_are_propagated() {
        let dir = temp_unicode_dir();
        let result = run_media_process(
            spec("cwd-env")
                .args(["STREAM_ARCHIVE_FIXTURE_ENV", "argument with spaces 한글"])
                .current_dir(dir.path())
                .env("STREAM_ARCHIVE_FIXTURE_ENV", "환경값-🎬"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.outcome, MediaProcessOutcome::Success);
        assert!(result.stdout.text.contains(&dir.path().display().to_string()));
        assert!(result.stdout.text.contains("환경값-🎬"));
        assert!(result.stdout.text.contains("argument with spaces 한글"));
    }

    #[tokio::test]
    async fn process_runner_does_not_delete_caller_owned_output() {
        let dir = temp_unicode_dir();
        let output = dir.path().join("부분 출력 파일 [한글].part");
        let result = run_media_process(
            spec("write-file")
                .args(vec![
                    output.as_os_str().to_owned(),
                    OsString::from("fixture-output"),
                ]),
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.outcome, MediaProcessOutcome::Success);
        assert_eq!(fs::read_to_string(&output).unwrap(), "fixture-output");
    }

    #[tokio::test]
    async fn timeout_terminates_owned_process_tree() {
        let dir = temp_unicode_dir();
        let child_pid = dir.path().join("child.pid");
        let result = run_media_process(
            spec("spawn-child")
                .args(vec![
                    OsString::from("30000"),
                    child_pid.as_os_str().to_owned(),
                ])
                .timeout(Duration::from_secs(3)),
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.outcome, MediaProcessOutcome::Timeout);
        let pid: u32 = fs::read_to_string(&child_pid)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            !test_process_exists(pid),
            "owned descendant must be gone after timeout cleanup"
        );
    }

    #[tokio::test]
    async fn cancellation_terminates_owned_tree_but_not_unrelated_process() {
        let dir = temp_unicode_dir();
        let owned_pid = dir.path().join("owned-child.pid");
        let owned_ready = dir.path().join("owned.ready");
        let unrelated_ready = dir.path().join("unrelated.ready");

        let mut unrelated = spawn_unrelated(&unrelated_ready);
        wait_for_file(&unrelated_ready).await;

        let cancellation = MediaCancellation::new();
        let task = tokio::spawn(run_media_process(
            spec("spawn-child-ready")
                .args([
                    "30000".into(),
                    owned_pid.as_os_str().to_owned(),
                    owned_ready.as_os_str().to_owned(),
                ])
                .timeout(Duration::from_secs(20)),
            Some(cancellation.clone()),
        ));
        wait_for_file(&owned_ready).await;
        cancellation.cancel();
        let result = tokio::time::timeout(Duration::from_secs(8), task)
            .await
            .expect("cancelled runner did not return")
            .unwrap()
            .unwrap();

        assert_eq!(result.outcome, MediaProcessOutcome::Cancelled);
        let pid: u32 = fs::read_to_string(&owned_pid)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(!test_process_exists(pid));
        assert!(
            unrelated.try_wait().unwrap().is_none(),
            "unrelated fixture process must survive owned cancellation"
        );
        let _ = unrelated.kill();
        let _ = unrelated.wait();
    }

    #[tokio::test]
    async fn version_probe_supports_all_media_tools() {
        for kind in ToolKind::ALL {
            let probe =
                probe_tool_version(kind, &fixture_path(), Duration::from_secs(3))
                    .await
                    .unwrap();
            assert_eq!(probe.status, MediaToolProbeStatus::Ok);
            assert_eq!(probe.version.as_deref(), Some("fixture-media-tool 1.2.3"));
        }
    }

    #[tokio::test]
    async fn version_probe_maps_failure_and_timeout() {
        let failure = probe_tool_version_with_environment(
            ToolKind::YtDlp,
            &fixture_path(),
            Duration::from_secs(3),
            BTreeMap::from([(
                OsString::from("STREAM_ARCHIVE_FIXTURE_VERSION_MODE"),
                OsString::from("fail"),
            )]),
        )
        .await
        .unwrap();
        assert_eq!(failure.status, MediaToolProbeStatus::ProcessFailure);

        let timeout = probe_tool_version_with_environment(
            ToolKind::Ffmpeg,
            &fixture_path(),
            Duration::from_millis(500),
            BTreeMap::from([(
                OsString::from("STREAM_ARCHIVE_FIXTURE_VERSION_MODE"),
                OsString::from("hang"),
            )]),
        )
        .await
        .unwrap();
        assert_eq!(timeout.status, MediaToolProbeStatus::Timeout);
    }

    #[test]
    fn bounded_tail_keeps_last_bytes() {
        let mut tail = Vec::new();
        let mut truncated = false;
        append_tail(&mut tail, b"0123456789", 6, &mut truncated);
        assert_eq!(&tail, b"456789");
        assert!(truncated);
    }

    #[test]
    fn fixture_build_path_is_unique_to_test_artifacts() {
        let path = fixture_path();
        assert!(path.components().any(|part| part.as_os_str() == "test-fixtures"));
        assert!(path.is_file());
    }

    #[test]
    fn test_marker_names_are_unique() {
        let left = format!("{}.ready", Uuid::new_v4().simple());
        let right = format!("{}.ready", Uuid::new_v4().simple());
        assert_ne!(left, right);
    }
}
