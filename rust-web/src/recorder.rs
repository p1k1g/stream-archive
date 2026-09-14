use crate::platform_runtime::terminate_owned_checked;
use crate::{
    backend::LogBuffer,
    model::LiveHistoryItem,
    store,
    support::platform::{PlatformId, live::StreamInput},
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};
use uuid::Uuid;

const GB: u64 = 1024 * 1024 * 1024;
const COOKIE_FILE_EXPIRES_UNIX: i64 = 4_102_444_800; // 2100-01-01 UTC

#[derive(Debug, Clone)]
pub struct RecorderConfig {
    pub streamlink: PathBuf,
    pub quality: String,
    pub stall_timeout: u64,
    pub monitor_interval: u64,
    pub min_free_space_gb: f64,
}

struct CookieFile(PathBuf);

impl CookieFile {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for CookieFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
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
    _cookie_file: Option<CookieFile>,
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

fn output_file_for_platform(mut requested: PathBuf, platform: PlatformId) -> Result<PathBuf> {
    let extension = platform.live_output_extension();
    let already_matches = requested
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension));
    if !already_matches {
        requested.set_extension(extension);
    }
    if !requested.exists() {
        return Ok(requested);
    }

    let parent = requested.parent().unwrap_or_else(|| Path::new("."));
    let stem = requested
        .file_stem()
        .ok_or_else(|| anyhow!("output file has no valid file stem"))?
        .to_os_string();
    for number in 2..=9999 {
        let mut file_name = stem.clone();
        file_name.push(format!("_{number:02}.{extension}"));
        let candidate = parent.join(file_name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!("too many output filename collisions")
}

fn find_on_path(names: &[&str]) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub(crate) fn resolve_timestamp_rebase_ffmpeg(streamlink: &Path) -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(streamlink_dir) = streamlink.parent() {
        candidates.push(streamlink_dir.join("ffmpeg.exe"));
        candidates.push(streamlink_dir.join("vod").join("ffmpeg.exe"));
        if let Some(streamlink_root) = streamlink_dir.parent() {
            // Official Streamlink Windows builds install streamlink.exe under bin/
            // and the bundled FFmpeg executable under ffmpeg/.
            candidates.push(streamlink_root.join("ffmpeg").join("ffmpeg.exe"));
            candidates.push(streamlink_root.join("ffmpeg.exe"));
        }
    }
    if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
        return Ok(path);
    }
    if let Some(path) = find_on_path(&["ffmpeg.exe", "ffmpeg"]) {
        return Ok(path);
    }
    bail!(
        "CHZZK LIVE MPEG-TS remux requires FFmpeg. Install the Streamlink Windows build with bundled FFmpeg or add ffmpeg.exe to PATH."
    )
}

pub(crate) fn timestamp_rebase_player_args(output_file: &Path) -> Result<String> {
    let output = output_file.to_string_lossy();
    if output.contains('"') {
        bail!("output file path contains an unsupported quote character");
    }
    // Streamlink expands {...} formatting variables in --player-args. Escape
    // literal braces from user-controlled output paths before handing the string
    // to Streamlink's player argument tokenizer.
    let output = output.replace('{', "{{").replace('}', "}}");
    Ok(format!(
        "-hide_banner -loglevel warning -fflags +genpts+discardcorrupt -i {{playerinput}} -map 0:v:0? -map 0:a:0? -c copy -bsf:v h264_mp4toannexb -f mpegts -mpegts_flags resend_headers -mpegts_copyts 0 -avoid_negative_ts make_zero -muxpreload 0 -muxdelay 0 -avioflags direct -y \"{output}\""
    ))
}

#[derive(Clone)]
pub struct RecorderManager {
    logs: LogBuffer,
}

impl RecorderManager {
    pub fn new(logs: LogBuffer) -> Self {
        Self { logs }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        &self,
        config: &RecorderConfig,
        platform: PlatformId,
        input: &StreamInput,
        output_file: PathBuf,
        bno: String,
        title: String,
        channel: &str,
        account: &str,
    ) -> Result<Recording> {
        let output_file = output_file_for_platform(output_file, platform)?;
        let output_dir = output_file
            .parent()
            .ok_or_else(|| anyhow!("output file has no parent"))?
            .to_path_buf();
        fs::create_dir_all(&output_dir)?;
        let free = free_gb(&output_dir)?;
        if free < config.min_free_space_gb {
            bail!(
                "LOW DISK SPACE - free={free:.2}GB limit={:.2}GB",
                config.min_free_space_gb
            );
        }

        let (stream_url, cookies, start_at_zero) = match input {
            StreamInput::DirectHls(url) => {
                let url = if url.to_ascii_lowercase().starts_with("hls://") {
                    url.clone()
                } else {
                    format!("hls://{url}")
                };
                (url, None, false)
            }
            StreamInput::PluginUrl {
                url,
                cookies,
                start_at_zero,
            } => {
                preflight_plugin_input(config, url, !cookies.is_empty()).await?;
                (url.clone(), Some(cookies.as_slice()), *start_at_zero)
            }
        };

        // Resolve every fallible timestamp-player setting before creating the
        // plaintext authentication cookie file. The CookieFile guard then owns
        // deletion across spawn failures and all subsequent early returns.
        let timestamp_player = if start_at_zero {
            Some((
                resolve_timestamp_rebase_ffmpeg(&config.streamlink)?,
                timestamp_rebase_player_args(&output_file)?,
            ))
        } else {
            None
        };
        let cookie_file = match cookies {
            Some(cookies) if !cookies.is_empty() => Some(write_cookie_file(cookies)?),
            _ => None,
        };

        let stream_timeout = config
            .stall_timeout
            .max(10)
            .saturating_add(config.monitor_interval.max(1));
        let mut command = Command::new(&config.streamlink);
        if let Some(cookie) = cookie_file.as_ref() {
            command.arg("--http-cookies-file").arg(cookie.path());
        }
        if let Some((ffmpeg, player_args)) = timestamp_player {
            // CHZZK's HLS worker must remain in Streamlink because it rewrites
            // segment requests. FFmpeg consumes Streamlink's already-fetched
            // fMP4 stream and remuxes it live into a genuine zero-based MPEG-TS
            // file. This is stream-copy only: no re-encode and no post-recording
            // remux/finalize pass or duplicate-disk-space requirement.
            command
                .arg("--player")
                .arg(ffmpeg)
                .arg("--player-args")
                .arg(player_args)
                .arg("--player-verbose");
        } else {
            command.arg("--output").arg(&output_file).arg("--force");
        }
        command
            .arg("--progress")
            .arg("no")
            .arg("--hls-live-edge")
            .arg("3")
            .arg("--stream-segment-threads")
            .arg("3")
            .arg("--stream-segmented-queue-deadline")
            .arg("0")
            .arg("--stream-timeout")
            .arg(stream_timeout.to_string())
            .arg(&stream_url)
            .arg(&config.quality)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(parent) = config.streamlink.parent() {
            if parent.is_dir() {
                command.current_dir(parent);
            }
        }

        let mut child = command.spawn().with_context(|| {
            format!(
                "failed to start Streamlink: {}",
                config.streamlink.display()
            )
        })?;
        let pid = child
            .id()
            .ok_or_else(|| anyhow!("Streamlink PID unavailable"))?;
        if let Some(stderr) = child.stderr.take() {
            let logs = self.logs.clone();
            let account = account.to_string();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line.trim();
                    if !line.is_empty() {
                        logs.push(format!("[RUST:STREAMLINK:{account}] {line}"))
                            .await;
                    }
                }
            });
        }

        let started_at = Utc::now();
        let history_id = Uuid::new_v4().to_string();
        if let Ok(db) = store::global() {
            if let Err(err) = db.start_live(&LiveHistoryItem {
                platform,
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
                self.logs
                    .push(format!("[DB:WARN] LIVE history start failed: {err:#}"))
                    .await;
            }
        }

        self.logs
            .push(format!(
                "[RUST] RECORD START platform={platform} channel={channel} account={account} bno={bno} pid={pid} file={}",
                output_file.display()
            ))
            .await;

        Ok(Recording {
            pid,
            bno,
            title,
            file: output_file,
            started_at,
            history_id,
            child,
            output_dir,
            _cookie_file: cookie_file,
            last_size: 0,
            last_growth: Instant::now(),
            last_monitor: Instant::now(),
        })
    }

    pub fn poll(&self, rec: &mut Recording, config: &RecorderConfig) -> Result<RecordingPoll> {
        if let Some(status) = rec
            .child
            .try_wait()
            .context("failed to query Streamlink status")?
        {
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
            return terminate_owned_checked(&mut rec.child)
                .await
                .with_context(|| format!("failed to stop recorder pid={}", rec.pid));
        }
        Ok(rec.child.wait().await.ok().and_then(|s| s.code()))
    }

    pub async fn log_finished(&self, channel: &str, account: &str, rec: &Recording, reason: &str) {
        let size = fs::metadata(&rec.file)
            .map(|m| m.len())
            .unwrap_or(rec.last_size);
        let ended_at = Utc::now();
        let secs = (ended_at - rec.started_at).num_seconds().max(0);
        let status = if reason == "NORMAL" {
            "COMPLETED"
        } else if reason.contains("STALLED") || reason.contains("EXIT CODE") {
            "FAILED"
        } else {
            "STOPPED"
        };
        if let Ok(db) = store::global() {
            if let Err(err) = db.finish_live(
                &rec.history_id,
                &ended_at.to_rfc3339(),
                secs,
                size,
                reason,
                status,
            ) {
                self.logs
                    .push(format!("[DB:WARN] LIVE history finish failed: {err:#}"))
                    .await;
            }
        }
        self.logs
            .push(format!(
                "[RUST] RECORD FINISHED channel={channel} account={account} duration={secs}s size={size} reason={reason} file={}",
                rec.file.display()
            ))
            .await;
    }
}

async fn preflight_plugin_input(
    config: &RecorderConfig,
    url: &str,
    needs_cookie_file: bool,
) -> Result<()> {
    let mut can_handle = Command::new(&config.streamlink);
    can_handle
        .arg("--can-handle-url")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(parent) = config.streamlink.parent() {
        if parent.is_dir() {
            can_handle.current_dir(parent);
        }
    }
    let status = can_handle.status().await.with_context(|| {
        format!(
            "failed to inspect Streamlink plugin support: {}",
            config.streamlink.display()
        )
    })?;
    if !status.success() {
        bail!(
            "현재 Streamlink이 이 플랫폼 URL을 처리할 수 없습니다. Streamlink을 최신 버전으로 업데이트하세요: {url}"
        );
    }

    if needs_cookie_file {
        let mut help = Command::new(&config.streamlink);
        help.arg("--help").stdin(Stdio::null());
        if let Some(parent) = config.streamlink.parent() {
            if parent.is_dir() {
                help.current_dir(parent);
            }
        }
        let output = help.output().await.with_context(|| {
            format!(
                "failed to inspect Streamlink cookie-file support: {}",
                config.streamlink.display()
            )
        })?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        if !text.contains("--http-cookies-file") {
            bail!(
                "CHZZK 제한 방송 인증에는 --http-cookies-file을 지원하는 Streamlink 8.2 이상이 필요합니다."
            );
        }
    }
    Ok(())
}

fn write_cookie_file(cookies: &[crate::support::platform::live::HttpCookie]) -> Result<CookieFile> {
    let path = env::temp_dir().join(format!(
        "stream-archive-cookies-{}.txt",
        Uuid::new_v4().simple()
    ));
    let mut text = String::from("# Netscape HTTP Cookie File\n");
    for cookie in cookies {
        if cookie.domain.contains(['\r', '\n', '\t'])
            || cookie.name.contains(['\r', '\n', '\t'])
            || cookie.value.contains(['\r', '\n', '\t'])
        {
            bail!("invalid cookie data");
        }
        let include_subdomains = if cookie.domain.starts_with('.') {
            "TRUE"
        } else {
            "FALSE"
        };
        let secure = if cookie.secure { "TRUE" } else { "FALSE" };
        // Streamlink 8.2+ loads Netscape files with Python's MozillaCookieJar.
        // An expiry value of 0 is treated as already expired and silently dropped,
        // so use a distant future timestamp; this temporary file is deleted after recording.
        text.push_str(&format!(
            "{}\t{}\t/\t{}\t{}\t{}\t{}\n",
            cookie.domain,
            include_subdomains,
            secure,
            COOKIE_FILE_EXPIRES_UNIX,
            cookie.name,
            cookie.value
        ));
    }
    fs::write(&path, text)
        .with_context(|| format!("failed to create Streamlink cookie file {}", path.display()))?;
    Ok(CookieFile(path))
}

pub fn free_gb(path: &Path) -> Result<f64> {
    Ok(fs2::available_space(path)? as f64 / GB as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::platform::live::HttpCookie;

    #[test]
    fn uses_platform_specific_live_output_extension_without_overwriting_existing_file() {
        let dir = env::temp_dir().join(format!(
            "stream-archive-extension-test-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).unwrap();
        let requested = dir.join("capture.ts");

        assert_eq!(
            output_file_for_platform(requested.clone(), PlatformId::Soop).unwrap(),
            requested
        );

        let chzzk = output_file_for_platform(requested.clone(), PlatformId::Chzzk).unwrap();
        assert_eq!(chzzk, dir.join("capture.ts"));
        fs::write(&chzzk, b"existing").unwrap();

        let next = output_file_for_platform(requested, PlatformId::Chzzk).unwrap();
        assert_eq!(next, dir.join("capture_02.ts"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn timestamp_rebase_player_args_write_zero_based_mpegts_stream_copy() {
        let args = timestamp_rebase_player_args(Path::new("capture.ts")).unwrap();
        assert!(args.contains("-fflags +genpts+discardcorrupt"));
        assert!(args.contains("-i {playerinput}"));
        assert!(args.contains("-c copy"));
        assert!(args.contains("-bsf:v h264_mp4toannexb"));
        assert!(args.contains("-f mpegts"));
        assert!(args.contains("-mpegts_flags resend_headers"));
        assert!(args.contains("-mpegts_copyts 0"));
        assert!(args.contains("-avoid_negative_ts make_zero"));
        assert!(args.contains("-muxpreload 0 -muxdelay 0"));
        assert!(args.ends_with("-avioflags direct -y \"capture.ts\""));
        assert!(!args.contains("frag_keyframe"));
    }

    #[test]
    fn timestamp_rebase_player_args_escape_streamlink_format_braces() {
        let args = timestamp_rebase_player_args(Path::new("capture_{live}.ts")).unwrap();
        assert!(args.contains("capture_{{live}}.ts"));
    }

    #[test]
    fn finds_ffmpeg_from_official_streamlink_windows_layout() {
        let root = env::temp_dir().join(format!(
            "stream-archive-streamlink-layout-test-{}",
            Uuid::new_v4().simple()
        ));
        let bin = root.join("bin");
        let ffmpeg_dir = root.join("ffmpeg");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&ffmpeg_dir).unwrap();
        let streamlink = bin.join("streamlink.exe");
        let ffmpeg = ffmpeg_dir.join("ffmpeg.exe");
        fs::write(&streamlink, b"stub").unwrap();
        fs::write(&ffmpeg, b"stub").unwrap();

        assert_eq!(
            resolve_timestamp_rebase_ffmpeg(&streamlink).unwrap(),
            ffmpeg
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn writes_netscape_cookie_file_with_non_expired_cookie() {
        let cookie_file = write_cookie_file(&[HttpCookie {
            domain: ".naver.com".into(),
            name: "NID_AUT".into(),
            value: "secret-value".into(),
            secure: true,
        }])
        .unwrap();
        let text = fs::read_to_string(cookie_file.path()).unwrap();
        assert!(text.contains(&format!(
            ".naver.com\tTRUE\t/\tTRUE\t{}\tNID_AUT\tsecret-value",
            COOKIE_FILE_EXPIRES_UNIX
        )));
        assert!(!text.contains("\t0\tNID_AUT\t"));
    }

    #[test]
    fn cookie_file_guard_removes_plaintext_temp_file_on_drop() {
        let cookie_file = write_cookie_file(&[HttpCookie {
            domain: ".naver.com".into(),
            name: "NID_SES".into(),
            value: "sensitive-session-value".into(),
            secure: true,
        }])
        .unwrap();
        let path = cookie_file.path().to_path_buf();
        assert!(path.is_file());
        drop(cookie_file);
        assert!(!path.exists());
    }
}
