from pathlib import Path
import re

vod_path = Path('rust-web/src/platform/chzzk/vod.rs')
text = vod_path.read_text(encoding='utf-8')

# Provider imports: VOD keeps the shared FFmpeg resolver, but no longer asks
# Streamlink to launch the LIVE player itself.
text = text.replace(
    '    recorder::{resolve_timestamp_rebase_ffmpeg, timestamp_rebase_player_args},\n',
    '    recorder::resolve_timestamp_rebase_ffmpeg,\n',
)
text = text.replace(
    '    io::{AsyncBufReadExt, AsyncReadExt, BufReader},\n',
    '    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},\n',
)

# CHZZK service/v3 video detail returns duration in seconds. 15185683 is 29856
# seconds (8:17:36); dividing by 1000 produced the user-visible 29s regression.
old_duration = '''    let duration_seconds = content\n        .get("duration")\n        .and_then(|value| {\n            value\n                .as_u64()\n                .or_else(|| value.as_f64().map(|n| n.max(0.0) as u64))\n        })\n        .unwrap_or(0)\n        / 1000;\n'''
new_duration = '''    let duration_seconds = content\n        .get("duration")\n        .and_then(|value| {\n            value\n                .as_u64()\n                .or_else(|| value.as_f64().map(|n| n.max(0.0) as u64))\n        })\n        .unwrap_or(0);\n'''
if old_duration not in text:
    raise RuntimeError('duration block anchor missing')
text = text.replace(old_duration, new_duration, 1)

# Feed the known API duration into the media pipeline so FFmpeg media timestamps
# can drive real progress instead of the previous hard-coded 95% completion jump.
old_call = '''            &staging_output,\n            status,\n            cancel,\n            logs,\n'''
new_call = '''            &staging_output,\n            metadata.duration_seconds,\n            status,\n            cancel,\n            logs,\n'''
if old_call not in text:
    raise RuntimeError('download_video call anchor missing')
text = text.replace(old_call, new_call, 1)

start = text.index('async fn download_video(')
end = text.index('\nfn configure_python_cli_command', start)
replacement = r'''async fn download_video(
    tools: &ChzzkTools,
    req: &VodDownloadRequest,
    cookie_file: Option<&Path>,
    output: &Path,
    duration_seconds: u64,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
    _logs: &LogBuffer,
) -> Result<()> {
    let (mut sorting_args, stream_name) = streamlink_quality_args(req.quality.trim());
    let mut args = vec![
        "--no-config".to_string(),
        "--loglevel".to_string(),
        "info".to_string(),
        "--progress".to_string(),
        "no".to_string(),
        "--stream-segment-threads".to_string(),
        "3".to_string(),
        "--ffmpeg-ffmpeg".to_string(),
        tools.ffmpeg.display().to_string(),
        "--ffmpeg-fout".to_string(),
        "mpegts".to_string(),
        "--stdout".to_string(),
    ];
    append_streamlink_cookie_arg(&mut args, cookie_file);
    args.append(&mut sorting_args);
    args.push(req.vod_url.clone());
    args.push(stream_name);

    run_streamlink_download(
        &tools.streamlink,
        &tools.ffmpeg,
        &args,
        output,
        duration_seconds,
        status,
        cancel,
    )
    .await
}

fn streamlink_quality_args(quality: &str) -> (Vec<String>, String) {
    if let Some(captures) = Regex::new(r"^best\[height<=(\d+)\]$")
        .unwrap()
        .captures(quality)
    {
        let height = captures.get(1).unwrap().as_str();
        return (
            vec!["--stream-sorting-excludes".into(), format!(">{height}p")],
            "best".into(),
        );
    }
    let stream = if quality.is_empty() { "best" } else { quality };
    (Vec::new(), stream.to_string())
}

fn ffmpeg_progress_seconds(line: &str) -> Option<f64> {
    let raw = line.trim().strip_prefix("out_time_us=")?;
    let micros = raw.parse::<u64>().ok()?;
    Some(micros as f64 / 1_000_000.0)
}

fn download_progress_percent(media_seconds: f64, duration_seconds: u64) -> f64 {
    if duration_seconds == 0 {
        return 0.0;
    }
    (media_seconds.max(0.0) / duration_seconds as f64 * 95.0).clamp(0.0, 95.0)
}

fn format_media_time(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

async fn update_download_progress(
    status: &Arc<RwLock<VodJobStatus>>,
    media_seconds: f64,
    duration_seconds: u64,
) {
    let mut current = status.write().await;
    current.percent = download_progress_percent(media_seconds, duration_seconds);
    current.current_part = 1;
    current.part_count = 1;
    current.message = if duration_seconds > 0 {
        format!(
            "CHZZK VOD 다운로드 중 · {} / {}",
            format_media_time(media_seconds),
            format_media_time(duration_seconds as f64)
        )
    } else {
        format!("CHZZK VOD 다운로드 중 · {}", format_media_time(media_seconds))
    };
}

fn spawn_line_reader<R>(reader: R, tx: mpsc::UnboundedSender<String>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx.send(line);
        }
    });
}

async fn run_streamlink_download(
    streamlink: &Path,
    ffmpeg: &Path,
    args: &[String],
    output: &Path,
    duration_seconds: u64,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
) -> Result<()> {
    let mut streamlink_command = Command::new(streamlink);
    configure_python_cli_command(&mut streamlink_command);
    streamlink_command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(parent) = streamlink.parent() {
        if parent.is_dir() {
            streamlink_command.current_dir(parent);
        }
    }
    let mut streamlink_child = streamlink_command
        .spawn()
        .with_context(|| format!("Streamlink 실행 실패: {}", streamlink.display()))?;
    let mut streamlink_stdout = streamlink_child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Streamlink stdout unavailable"))?;

    let mut ffmpeg_command = Command::new(ffmpeg);
    ffmpeg_command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("warning")
        .arg("-fflags")
        .arg("+genpts+discardcorrupt")
        .arg("-i")
        .arg("pipe:0")
        .arg("-map")
        .arg("0:v:0?")
        .arg("-map")
        .arg("0:a:0?")
        .arg("-c")
        .arg("copy")
        .arg("-bsf:v")
        .arg("h264_mp4toannexb")
        .arg("-f")
        .arg("mpegts")
        .arg("-mpegts_flags")
        .arg("resend_headers")
        .arg("-mpegts_copyts")
        .arg("0")
        .arg("-avoid_negative_ts")
        .arg("make_zero")
        .arg("-muxpreload")
        .arg("0")
        .arg("-muxdelay")
        .arg("0")
        .arg("-avioflags")
        .arg("direct")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg("-y")
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut ffmpeg_child = ffmpeg_command
        .spawn()
        .with_context(|| format!("FFmpeg 실행 실패: {}", ffmpeg.display()))?;
    let mut ffmpeg_stdin = ffmpeg_child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("FFmpeg stdin unavailable"))?;

    let pump = tokio::spawn(async move {
        let copied = tokio::io::copy(&mut streamlink_stdout, &mut ffmpeg_stdin).await;
        let _ = ffmpeg_stdin.shutdown().await;
        copied
    });

    let (log_tx, mut log_rx) = mpsc::unbounded_channel::<String>();
    if let Some(stderr) = streamlink_child.stderr.take() {
        spawn_line_reader(stderr, log_tx.clone());
    }
    if let Some(stderr) = ffmpeg_child.stderr.take() {
        spawn_line_reader(stderr, log_tx.clone());
    }
    drop(log_tx);

    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<String>();
    if let Some(stdout) = ffmpeg_child.stdout.take() {
        spawn_line_reader(stdout, progress_tx);
    }

    let mut tail = VecDeque::with_capacity(20);
    let mut streamlink_exit = None;
    let mut ffmpeg_exit = None;

    loop {
        if cancel.load(Ordering::SeqCst) {
            terminate_owned(&mut streamlink_child).await;
            terminate_owned(&mut ffmpeg_child).await;
            pump.abort();
            let _ = pump.await;
            let _ = fs::remove_file(output);
            return Ok(());
        }

        while let Ok(line) = log_rx.try_recv() {
            push_tail(&mut tail, &line);
        }
        while let Ok(line) = progress_rx.try_recv() {
            if let Some(media_seconds) = ffmpeg_progress_seconds(&line) {
                update_download_progress(status, media_seconds, duration_seconds).await;
            }
        }

        if streamlink_exit.is_none() {
            streamlink_exit = streamlink_child
                .try_wait()
                .context("Streamlink CHZZK 상태 확인 실패")?;
            if let Some(exit) = streamlink_exit.as_ref() {
                if !exit.success() {
                    terminate_owned(&mut ffmpeg_child).await;
                    pump.abort();
                    let _ = pump.await;
                    let _ = fs::remove_file(output);
                    while let Ok(line) = log_rx.try_recv() {
                        push_tail(&mut tail, &line);
                    }
                    bail!(
                        "Streamlink CHZZK 다운로드 실패 (exit={}): {}",
                        exit_code(*exit),
                        redact(&tail.into_iter().collect::<Vec<_>>().join(" | "))
                    );
                }
            }
        }

        if ffmpeg_exit.is_none() {
            ffmpeg_exit = ffmpeg_child
                .try_wait()
                .context("FFmpeg CHZZK 상태 확인 실패")?;
            if let Some(exit) = ffmpeg_exit.as_ref() {
                if !exit.success() {
                    terminate_owned(&mut streamlink_child).await;
                    pump.abort();
                    let _ = pump.await;
                    let _ = fs::remove_file(output);
                    while let Ok(line) = log_rx.try_recv() {
                        push_tail(&mut tail, &line);
                    }
                    bail!(
                        "FFmpeg CHZZK MPEG-TS 저장 실패 (exit={}): {}",
                        exit_code(*exit),
                        redact(&tail.into_iter().collect::<Vec<_>>().join(" | "))
                    );
                }
            }
        }

        if streamlink_exit.is_some() && ffmpeg_exit.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    while let Ok(line) = progress_rx.try_recv() {
        if let Some(media_seconds) = ffmpeg_progress_seconds(&line) {
            update_download_progress(status, media_seconds, duration_seconds).await;
        }
    }
    while let Ok(line) = log_rx.try_recv() {
        push_tail(&mut tail, &line);
    }

    let copied = pump
        .await
        .context("CHZZK Streamlink-to-FFmpeg pipe task join failed")?
        .context("CHZZK Streamlink-to-FFmpeg pipe failed")?;
    if copied == 0 {
        let _ = fs::remove_file(output);
        bail!("CHZZK Streamlink MPEG-TS pipe produced no media bytes");
    }

    let size = fs::metadata(output)
        .with_context(|| format!("Streamlink CHZZK output missing: {}", output.display()))?
        .len();
    if size == 0 {
        let _ = fs::remove_file(output);
        bail!("Streamlink CHZZK output is empty");
    }
    let mut current = status.write().await;
    current.percent = 95.0;
    current.current_part = 1;
    current.part_count = 1;
    current.message = "CHZZK VOD 저장 마무리 중…".into();
    Ok(())
}
'''
text = text[:start] + replacement + text[end:]

# Replace the now-invalid milliseconds regression with an API-seconds regression,
# and add progress parsing/calculation coverage.
old_test = '''    #[test]\n    fn chzzk_api_metadata_maps_milliseconds_to_seconds() {\n        let content = serde_json::json!({\n            "videoTitle": "테스트 VOD",\n            "duration": 1234567,\n            "publishDate": "2026-09-13 10:00:00",\n            "channel": {\n                "channelName": "테스트 채널",\n                "channelId": "0123456789abcdef0123456789abcdef"\n            }\n        });\n        let metadata = metadata_from_chzzk_content(&content).unwrap();\n        assert_eq!(metadata.title, "테스트 VOD");\n        assert_eq!(metadata.streamer, "테스트 채널");\n        assert_eq!(metadata.date, "260913");\n        assert_eq!(metadata.duration_seconds, 1234);\n        let view = analysis_view("https://chzzk.naver.com/video/1", &metadata);\n        assert_eq!(view.part_count, 1);\n        assert_eq!(view.parts[0].duration_seconds, 1234);\n    }\n'''
new_test = '''    #[test]\n    fn chzzk_api_metadata_preserves_seconds() {\n        let content = serde_json::json!({\n            "videoTitle": "테스트 VOD",\n            "duration": 29856,\n            "publishDate": "2026-09-13 10:00:00",\n            "channel": {\n                "channelName": "테스트 채널",\n                "channelId": "0123456789abcdef0123456789abcdef"\n            }\n        });\n        let metadata = metadata_from_chzzk_content(&content).unwrap();\n        assert_eq!(metadata.title, "테스트 VOD");\n        assert_eq!(metadata.streamer, "테스트 채널");\n        assert_eq!(metadata.date, "260913");\n        assert_eq!(metadata.duration_seconds, 29856);\n        let view = analysis_view("https://chzzk.naver.com/video/15185683", &metadata);\n        assert_eq!(view.part_count, 1);\n        assert_eq!(view.parts[0].duration_seconds, 29856);\n    }\n\n    #[test]\n    fn ffmpeg_media_progress_drives_chzzk_percent_and_time() {\n        assert_eq!(ffmpeg_progress_seconds("out_time_us=3600000000"), Some(3600.0));\n        let percent = download_progress_percent(3600.0, 7200);\n        assert!((percent - 47.5).abs() < 0.001);\n        assert_eq!(format_media_time(29856.0), "08:17:36");\n        assert_eq!(download_progress_percent(99999.0, 29856), 95.0);\n    }\n'''
if old_test not in text:
    raise RuntimeError('old duration regression test anchor missing')
text = text.replace(old_test, new_test, 1)

vod_path.write_text(text, encoding='utf-8')

# Update the Phase 18 regression guard for the corrected API unit and the
# Streamlink internal DASH->MPEG-TS mux + owned external FFmpeg progress pipeline.
guard_path = Path('maintenance/Test-Phase18ChzzkVod.ps1')
g = guard_path.read_text(encoding='utf-8')
g = g.replace(
    "Assert-Match $chzzkVod 'duration_seconds[\\s\\S]*?/ 1000;' 'CHZZK API duration is not converted from milliseconds to seconds.'\nAssert-Match $chzzkVod 'chzzk_api_metadata_maps_milliseconds_to_seconds' 'CHZZK duration conversion regression test is missing.'\n",
    "Assert-NotMatch $chzzkVod 'duration_seconds[\\s\\S]*?/ 1000;' 'CHZZK API duration is already seconds and must not be divided by 1000.'\nAssert-Match $chzzkVod 'chzzk_api_metadata_preserves_seconds' 'CHZZK API seconds regression test is missing.'\n",
)
g = g.replace(
    "Assert-Match $chzzkVod '\"--player\"' 'CHZZK VOD does not use the LIVE-style Streamlink player pipeline.'\nAssert-Match $chzzkVod '\"--player-args\"' 'CHZZK VOD does not pass timestamp-rebase player arguments.'\nAssert-Match $chzzkVod 'timestamp_rebase_player_args\\(output\\)' 'CHZZK VOD does not reuse the CHZZK LIVE MPEG-TS remux arguments.'\n",
    "Assert-Match $chzzkVod '\"--ffmpeg-fout\"' 'CHZZK VOD does not force Streamlink DASH mux output format.'\nAssert-Match $chzzkVod '\"mpegts\"' 'CHZZK VOD Streamlink DASH mux is not forced to MPEG-TS.'\nAssert-Match $chzzkVod '\"--stdout\"' 'CHZZK VOD Streamlink MPEG-TS pipe is missing.'\nAssert-Match $chzzkVod '\"-progress\"' 'CHZZK VOD owned FFmpeg progress output is missing.'\nAssert-Match $chzzkVod '\"pipe:1\"' 'CHZZK VOD FFmpeg progress pipe is missing.'\nAssert-Match $chzzkVod 'ffmpeg_progress_seconds' 'CHZZK VOD FFmpeg media progress parser is missing.'\nAssert-Match $chzzkVod 'ffmpeg_media_progress_drives_chzzk_percent_and_time' 'CHZZK VOD progress regression test is missing.'\n",
)
# LIVE helper remains protected independently; VOD plumbing must no longer depend
# on Streamlink launching the external player itself.
g = g.replace(
    "Assert-Match $recorder 'pub\\(crate\\) fn timestamp_rebase_player_args' 'Shared CHZZK LIVE/VOD MPEG-TS player helper is not exposed.'\n",
    "Assert-Match $recorder 'pub\\(crate\\) fn timestamp_rebase_player_args' 'CHZZK LIVE MPEG-TS player helper disappeared.'\n",
)
guard_path.write_text(g, encoding='utf-8')

print('patched CHZZK VOD duration, MPEG-TS media pipeline, and progress reporting')
