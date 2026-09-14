from pathlib import Path
import re

VOD = Path('rust-web/src/platform/chzzk/vod.rs')
RECORDER = Path('rust-web/src/recorder.rs')

vod = VOD.read_text(encoding='utf-8')
recorder = RECORDER.read_text(encoding='utf-8')


def replace_once(text: str, old: str, new: str, name: str) -> str:
    if old not in text:
        raise RuntimeError(f'missing anchor: {name}')
    return text.replace(old, new, 1)


def sub_once(text: str, pattern: str, replacement: str, name: str) -> str:
    out, count = re.subn(pattern, lambda _m: replacement, text, count=1, flags=re.S)
    if count != 1:
        raise RuntimeError(f'regex replacement failed ({count}): {name}')
    return out

# Reuse the exact CHZZK LIVE timestamp-rebase MPEG-TS player pipeline.
recorder = replace_once(
    recorder,
    'fn resolve_timestamp_rebase_ffmpeg(streamlink: &Path) -> Result<PathBuf> {',
    'pub(crate) fn resolve_timestamp_rebase_ffmpeg(streamlink: &Path) -> Result<PathBuf> {',
    'expose live ffmpeg resolver',
)
recorder = replace_once(
    recorder,
    'fn timestamp_rebase_player_args(output_file: &Path) -> Result<String> {',
    'pub(crate) fn timestamp_rebase_player_args(output_file: &Path) -> Result<String> {',
    'expose live mpegts player args',
)

vod = replace_once(
    vod,
    '    backend::LogBuffer,\n',
    '    backend::{LogBuffer, read_safe_settings, settings_path},\n    recorder::{resolve_timestamp_rebase_ffmpeg, timestamp_rebase_player_args},\n',
    'imports',
)
vod = replace_once(
    vod,
    '    collections::{BTreeSet, VecDeque},\n',
    '    collections::VecDeque,\n',
    'collections import',
)
vod = vod.replace('const PROGRESS_PREFIX: &str = "__CHZZK_PROGRESS__";\n', '')
vod = replace_once(vod, 'const MEDIA_FILE_NAME: &str = "media.mp4";', 'const MEDIA_FILE_NAME: &str = "media.ts";', 'staging extension')
vod = replace_once(
    vod,
    '''#[derive(Debug, Clone)]
struct Tools {
    yt_dlp: PathBuf,
    ffmpeg: Option<PathBuf>,
}
''',
    '''#[derive(Debug, Clone)]
struct ChzzkTools {
    streamlink: PathBuf,
    ffmpeg: PathBuf,
}
''',
    'tools struct',
)
vod = re.sub(r'\n#\[derive\(Clone\)\]\nstruct PublicPlaybackFallback \{.*?\n\}\n', '\n', vod, count=1, flags=re.S)

vod = replace_once(
    vod,
    '    let tools = resolve_tools(backend, &req.yt_dlp_path, &req.ffmpeg_path)?;',
    '    let tools = resolve_chzzk_tools(backend, &req.ffmpeg_path)?;',
    'analysis tools',
)
vod = replace_once(
    vod,
    '    let metadata =\n        load_metadata(&tools, &req.vod_url, cookie_file.as_deref(), cancel, logs).await?;',
    '''    let metadata = load_chzzk_metadata(
        &tools,
        &req.vod_url,
        cookie_file.as_deref(),
        cancel,
        logs,
    )
    .await?;''',
    'analysis metadata',
)
vod = replace_once(
    vod,
    '    let tools = resolve_tools(backend, &req.yt_dlp_path, &req.ffmpeg_path)?;',
    '    let tools = resolve_chzzk_tools(backend, &req.ffmpeg_path)?;',
    'download tools',
)
vod = replace_once(
    vod,
    '    let metadata =\n        load_metadata(&tools, &req.vod_url, cookie_file.as_deref(), cancel, logs).await?;',
    '''    let metadata = load_chzzk_metadata(
        &tools,
        &req.vod_url,
        cookie_file.as_deref(),
        cancel,
        logs,
    )
    .await?;''',
    'download metadata',
)
vod = replace_once(vod, 'claim_collision_path(&output_dir, &base, "mp4")?', 'claim_collision_path(&output_dir, &base, "ts")?', 'destination extension')
vod = replace_once(
    vod,
    'Regex::new(r"^best(?:\\[height<=\\d+\\])?$")',
    'Regex::new(r"^(?:best|worst|\\d+p(?:\\d+)?|best\\[height<=\\d+\\])$")',
    'quality validation',
)

metadata_block = r'''async fn load_chzzk_metadata(
    tools: &ChzzkTools,
    vod_url: &str,
    cookie_file: Option<&Path>,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<Metadata> {
    if cancel.load(Ordering::SeqCst) {
        bail!("CHZZK VOD metadata request cancelled");
    }

    let auth = ChzzkAuth::load()?;
    if auth.partial() {
        bail!("CHZZK 인증정보가 일부만 설정되어 있습니다. NID_AUT/NID_SES를 모두 저장하세요.");
    }

    let video_no = video_id(vod_url)?;
    let client = Client::new();
    let mut request = client.get(format!(
        "https://api.chzzk.naver.com/service/v3/videos/{video_no}"
    ));
    if let Some(cookie) = auth.cookie_header() {
        request = request.header(reqwest::header::COOKIE, cookie);
    }
    let value: Value = request
        .send()
        .await
        .context("CHZZK video detail request failed")?
        .error_for_status()
        .context("CHZZK video detail HTTP error")?
        .json()
        .await
        .context("CHZZK video detail JSON parse failed")?;
    if value.get("code").and_then(Value::as_i64) != Some(200) {
        bail!("CHZZK video detail API returned a non-success response");
    }
    let content = value
        .get("content")
        .filter(|value| !value.is_null())
        .with_context(|| match auth.state() {
            ChzzkAuthState::Missing => "CHZZK VOD 정보를 가져오지 못했습니다. 로그인/연령 확인이 필요한 VOD라면 NID_AUT/NID_SES를 저장하세요.",
            ChzzkAuthState::Partial => "CHZZK 인증정보가 일부만 설정되어 있습니다. NID_AUT/NID_SES를 모두 저장하세요.",
            ChzzkAuthState::Configured => "CHZZK 인증 쿠키가 만료되었거나 이 VOD를 볼 권한이 없을 수 있습니다.",
        })?;

    let mut metadata = metadata_from_chzzk_content(content)?;
    metadata.qualities = streamlink_quality_options(tools, vod_url, cookie_file, cancel, logs)
        .await
        .with_context(|| match auth.state() {
            ChzzkAuthState::Missing => "Streamlink가 CHZZK VOD 스트림을 찾지 못했습니다. 인증이 필요한 VOD인지 확인하세요.",
            ChzzkAuthState::Partial => "CHZZK 인증정보가 일부만 설정되어 있습니다.",
            ChzzkAuthState::Configured => "Streamlink가 CHZZK VOD 스트림을 열지 못했습니다. 쿠키 만료 또는 시청 권한을 확인하세요.",
        })?;
    logs.push(format!(
        "[VOD:CHZZK] metadata via CHZZK API; streamlink qualities={}",
        metadata.qualities.len()
    ))
    .await;
    Ok(metadata)
}

async fn streamlink_quality_options(
    tools: &ChzzkTools,
    vod_url: &str,
    cookie_file: Option<&Path>,
    cancel: &AtomicBool,
    logs: &LogBuffer,
) -> Result<Vec<VodQualityOption>> {
    let mut args = vec![
        "--no-config".to_string(),
        "--json".to_string(),
        "--ffmpeg-ffmpeg".to_string(),
        tools.ffmpeg.display().to_string(),
    ];
    append_streamlink_cookie_arg(&mut args, cookie_file);
    args.push(vod_url.to_string());
    let stdout = run_capture(
        &tools.streamlink,
        &args,
        cancel,
        logs,
        "CHZZK Streamlink analyze",
    )
    .await?;
    let value: Value = serde_json::from_str(stdout.trim())
        .context("Streamlink CHZZK JSON parse failed")?;
    quality_options_from_streamlink_json(&value)
}

fn quality_options_from_streamlink_json(value: &Value) -> Result<Vec<VodQualityOption>> {
    let streams = value
        .get("streams")
        .and_then(Value::as_object)
        .context("Streamlink CHZZK JSON has no streams")?;
    let quality_re = Regex::new(r"^(\d+)p(\d+)?$").unwrap();
    let mut names = streams
        .keys()
        .filter_map(|name| {
            let captures = quality_re.captures(name)?;
            let height = captures.get(1)?.as_str().parse::<u64>().ok()?;
            let fps = captures
                .get(2)
                .and_then(|value| value.as_str().parse::<u64>().ok())
                .unwrap_or(0);
            Some((height, fps, name.clone()))
        })
        .collect::<Vec<_>>();
    names.sort_by(|left, right| right.cmp(left));

    let mut options = vec![VodQualityOption {
        value: "best".into(),
        label: "최고 화질 (자동)".into(),
    }];
    for (_, _, name) in names {
        options.push(VodQualityOption {
            value: name.clone(),
            label: name,
        });
    }
    Ok(options)
}

fn metadata_from_chzzk_content(content: &Value) -> Result<Metadata> {
    let title = content
        .get("videoTitle")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        bail!("CHZZK API did not return a VOD title");
    }
    let streamer = content
        .pointer("/channel/channelName")
        .and_then(Value::as_str)
        .unwrap_or("CHZZK")
        .trim()
        .to_string();
    let streamer_id = content
        .pointer("/channel/channelId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let duration_seconds = content
        .get("duration")
        .and_then(|value| value.as_u64().or_else(|| value.as_f64().map(|n| n.max(0.0) as u64)))
        .unwrap_or(0)
        / 1000;
    let date = content
        .get("publishDate")
        .and_then(Value::as_str)
        .and_then(short_date)
        .or_else(|| {
            content
                .get("publishDateAt")
                .and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|n| n as i64)))
                .and_then(chrono::DateTime::<Utc>::from_timestamp_millis)
                .map(|timestamp| {
                    let local = timestamp.with_timezone(&Local);
                    format!("{:02}{:02}{:02}", local.year() % 100, local.month(), local.day())
                })
        })
        .unwrap_or_else(today_short_date);
    Ok(Metadata {
        title,
        streamer,
        streamer_id,
        date,
        duration_seconds,
        qualities: vec![VodQualityOption {
            value: "best".into(),
            label: "최고 화질 (자동)".into(),
        }],
    })
}
'''
vod = sub_once(vod, r'async fn load_metadata\(.*?\nfn analysis_view\(', metadata_block + '\nfn analysis_view(', 'metadata pipeline')

download_block = r'''async fn download_video(
    tools: &ChzzkTools,
    req: &VodDownloadRequest,
    cookie_file: Option<&Path>,
    output: &Path,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
    _logs: &LogBuffer,
) -> Result<()> {
    let player_args = timestamp_rebase_player_args(output)?;
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
        "--player".to_string(),
        tools.ffmpeg.display().to_string(),
        "--player-args".to_string(),
        player_args,
        "--player-verbose".to_string(),
    ];
    append_streamlink_cookie_arg(&mut args, cookie_file);
    args.append(&mut sorting_args);
    args.push(req.vod_url.clone());
    args.push(stream_name);

    run_streamlink_download(&tools.streamlink, &args, output, status, cancel).await
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

async fn run_streamlink_download(
    streamlink: &Path,
    args: &[String],
    output: &Path,
    status: &Arc<RwLock<VodJobStatus>>,
    cancel: &AtomicBool,
) -> Result<()> {
    let mut command = Command::new(streamlink);
    configure_python_cli_command(&mut command);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(parent) = streamlink.parent() {
        if parent.is_dir() {
            command.current_dir(parent);
        }
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("Streamlink 실행 실패: {}", streamlink.display()))?;

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    if let Some(stdout) = child.stdout.take() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx.send(line);
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx.send(line);
            }
        });
    }
    drop(tx);

    let mut tail = VecDeque::with_capacity(20);
    let exit_status = loop {
        if cancel.load(Ordering::SeqCst) {
            terminate_owned(&mut child).await;
            let _ = fs::remove_file(output);
            return Ok(());
        }
        while let Ok(line) = rx.try_recv() {
            push_tail(&mut tail, &line);
        }
        if let Some(exit) = child.try_wait().context("Streamlink CHZZK 상태 확인 실패")? {
            break exit;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    };
    while let Ok(line) = rx.try_recv() {
        push_tail(&mut tail, &line);
    }
    if !exit_status.success() {
        let _ = fs::remove_file(output);
        bail!(
            "Streamlink CHZZK 다운로드 실패 (exit={}): {}",
            exit_code(exit_status),
            redact(&tail.into_iter().collect::<Vec<_>>().join(" | "))
        );
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

fn configure_python_cli_command(command: &mut Command) {
    command.env("PYTHONUTF8", "1");
    command.env("PYTHONIOENCODING", "utf-8");
}
'''
vod = sub_once(vod, r'async fn download_video\(.*?\nasync fn run_capture\(', download_block + '\nasync fn run_capture(', 'streamlink download pipeline')
vod = vod.replace('configure_ytdlp_command(&mut command);', 'configure_python_cli_command(&mut command);')

resolver_block = r'''fn append_streamlink_cookie_arg(args: &mut Vec<String>, cookie_file: Option<&Path>) {
    if let Some(path) = cookie_file {
        args.push("--http-cookies-file".to_string());
        args.push(path.display().to_string());
    }
}

fn resolve_chzzk_tools(backend: &Path, ffmpeg: &str) -> Result<ChzzkTools> {
    let settings = read_safe_settings(&settings_path(backend)).unwrap_or_default();
    let streamlink = resolve_streamlink_tool(backend, &settings)?;
    let ffmpeg = match resolve_tool(
        ffmpeg,
        &[
            backend.join("vod").join("ffmpeg.exe"),
            backend.join("ffmpeg.exe"),
        ],
        &["ffmpeg.exe", "ffmpeg"],
    ) {
        Some(path) => path,
        None => resolve_timestamp_rebase_ffmpeg(&streamlink)?,
    };
    Ok(ChzzkTools { streamlink, ffmpeg })
}

fn resolve_streamlink_tool(
    backend: &Path,
    settings: &std::collections::BTreeMap<String, String>,
) -> Result<PathBuf> {
    for key in ["STREAMLINK_PATH", "STREAMLINK_FALLBACK"] {
        if let Some(value) = settings.get(key) {
            let value = value.trim();
            if !value.is_empty() && !value.eq_ignore_ascii_case("AUTO") {
                let path = PathBuf::from(value);
                if path.is_file() {
                    return Ok(path);
                }
            }
        }
    }
    let mut candidates = vec![backend.join("streamlink.exe")];
    #[cfg(windows)]
    {
        candidates.push(PathBuf::from(r"C:\Program Files\Streamlink\bin\streamlink.exe"));
        candidates.push(PathBuf::from(r"C:\Program Files\Streamlink\streamlink.exe"));
    }
    resolve_tool("", &candidates, &["streamlink.exe", "streamlink"])
        .ok_or_else(|| anyhow!(
            "Streamlink 실행 파일을 찾지 못했습니다. STREAMLINK_PATH/STREAMLINK_FALLBACK 설정을 확인하세요."
        ))
}
'''
vod = sub_once(vod, r'fn append_cookie_arg\(.*?\nfn resolve_tool\(', resolver_block + '\nfn resolve_tool(', 'tool resolver')

# Direct Streamlink player output is the finished MPEG-TS staging file.
vod = replace_once(vod, '                    "mp4" | "mkv" | "webm"', '                    "ts" | "mp4" | "mkv" | "webm"', 'finished extensions')
vod = replace_once(vod, 'anyhow!("yt-dlp 완료 파일을 찾지 못했습니다: {}", expected.display())', 'anyhow!("CHZZK 완료 파일을 찾지 못했습니다: {}", expected.display())', 'finished output error')

# Replace yt-dlp/fallback-specific tests with Streamlink/API tests, including Codex duration-ms regression.
test_block = r'''    #[test]
    fn streamlink_json_builds_quality_options() {
        let value = serde_json::json!({
            "streams": {
                "144p": {},
                "720p": {},
                "1080p": {},
                "worst": {},
                "best": {}
            }
        });
        let options = quality_options_from_streamlink_json(&value).unwrap();
        assert_eq!(
            options.iter().map(|value| value.value.as_str()).collect::<Vec<_>>(),
            vec!["best", "1080p", "720p", "144p"]
        );
    }

    #[test]
    fn maps_legacy_height_quality_to_streamlink_selector() {
        let (args, stream) = streamlink_quality_args("best[height<=1080]");
        assert_eq!(stream, "best");
        assert_eq!(args, vec!["--stream-sorting-excludes", ">1080p"]);
        let (args, stream) = streamlink_quality_args("720p");
        assert!(args.is_empty());
        assert_eq!(stream, "720p");
    }
'''
vod = sub_once(
    vod,
    r'    #\[test\]\n    fn builds_quality_options_from_video_heights\(\).*?\n    #\[test\]\n    fn validates_single_part_download_contract',
    test_block + '\n    #[test]\n    fn validates_single_part_download_contract',
    'quality tests',
)
api_test = r'''    #[test]
    fn chzzk_api_metadata_maps_milliseconds_to_seconds() {
        let content = serde_json::json!({
            "videoTitle": "테스트 VOD",
            "duration": 1234567,
            "publishDate": "2026-09-13 10:00:00",
            "channel": {
                "channelName": "테스트 채널",
                "channelId": "0123456789abcdef0123456789abcdef"
            }
        });
        let metadata = metadata_from_chzzk_content(&content).unwrap();
        assert_eq!(metadata.title, "테스트 VOD");
        assert_eq!(metadata.streamer, "테스트 채널");
        assert_eq!(metadata.date, "260913");
        assert_eq!(metadata.duration_seconds, 1234);
        let view = analysis_view("https://chzzk.naver.com/video/1", &metadata);
        assert_eq!(view.part_count, 1);
        assert_eq!(view.parts[0].duration_seconds, 1234);
    }
'''
vod = sub_once(
    vod,
    r'    #\[test\]\n    fn public_api_fallback_builds_dash_url_for_abr_hls\(\).*?\n    #\[test\]\n    fn redacts_naver_cookie_values',
    api_test + '\n    #[test]\n    fn redacts_naver_cookie_values',
    'api/fallback tests',
)

vod = vod.replace('fn yt_dlp_staging_name_is_short_and_title_independent()', 'fn streamlink_staging_name_is_short_and_title_independent()')
vod = vod.replace('temp.path().join("media.mp4.part-Frag1.part")', 'temp.path().join("media.ts.part")')
vod = vod.replace('!temp.path().join("media.mp4.part-Frag1.part").exists()', '!temp.path().join("media.ts.part").exists()')

# Container-specific regression tests should reflect the CHZZK TS contract.
vod = vod.replace('claim_collision_path(temp.path(), "same", "mp4")', 'claim_collision_path(temp.path(), "same", "ts")')
vod = vod.replace('"same.mp4"', '"same.ts"')
vod = vod.replace('"same_02.mp4"', '"same_02.ts"')
vod = vod.replace('temp.path().join("source.mp4")', 'temp.path().join("source.ts")')
vod = vod.replace('claim_collision_path(temp.path(), "final", "mp4")', 'claim_collision_path(temp.path(), "final", "ts")')
vod = vod.replace('temp.path().join("final.mp4")', 'temp.path().join("final.ts")')

# Explicitly prove CHZZK no longer executes yt-dlp even though the common request still carries the field for SOOP.
for forbidden in ['--dump-single-json', '--merge-output-format', 'run_ffmpeg_fallback', 'is_source_url_extractor_bug']:
    if forbidden in vod:
        raise RuntimeError(f'legacy CHZZK yt-dlp/fallback path remains: {forbidden}')
if 'tools.yt_dlp' in vod or 'struct Tools {' in vod:
    raise RuntimeError('legacy CHZZK yt-dlp tool dependency remains')
if 'claim_collision_path(&output_dir, &base, "ts")' not in vod:
    raise RuntimeError('CHZZK destination is not TS')
if '/ 1000;' not in vod:
    raise RuntimeError('CHZZK API duration milliseconds conversion missing')
if 'timestamp_rebase_player_args(output)?' not in vod:
    raise RuntimeError('CHZZK VOD is not reusing the LIVE MPEG-TS player pipeline')

VOD.write_text(vod, encoding='utf-8')
RECORDER.write_text(recorder, encoding='utf-8')
