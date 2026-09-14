from pathlib import Path

vod = Path('rust-web/src/platform/chzzk/vod.rs')
text = vod.read_text(encoding='utf-8')

if 'CHZZK_API_TOTAL_TIMEOUT' in text:
    print('cancel fix already present')
    raise SystemExit(0)

old = '''const COPY_BUFFER_SIZE: usize = 1024 * 1024;\n'''
new = '''const COPY_BUFFER_SIZE: usize = 1024 * 1024;\nconst CHZZK_API_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);\nconst CHZZK_API_TOTAL_TIMEOUT: Duration = Duration::from_secs(30);\nconst CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);\n'''
if old not in text:
    raise RuntimeError('constant anchor missing')
text = text.replace(old, new, 1)

old = '''    let video_no = video_id(vod_url)?;\n    let client = Client::new();\n    let mut request = client.get(format!(\n        "https://api.chzzk.naver.com/service/v3/videos/{video_no}"\n    ));\n    if let Some(cookie) = auth.cookie_header() {\n        request = request.header(reqwest::header::COOKIE, cookie);\n    }\n    let value: Value = request\n        .send()\n        .await\n        .context("CHZZK video detail request failed")?\n        .error_for_status()\n        .context("CHZZK video detail HTTP error")?\n        .json()\n        .await\n        .context("CHZZK video detail JSON parse failed")?;\n'''
new = '''    let video_no = video_id(vod_url)?;\n    let client = Client::builder()\n        .connect_timeout(CHZZK_API_CONNECT_TIMEOUT)\n        .timeout(CHZZK_API_TOTAL_TIMEOUT)\n        .build()\n        .context("CHZZK video detail client build failed")?;\n    let mut request = client.get(format!(\n        "https://api.chzzk.naver.com/service/v3/videos/{video_no}"\n    ));\n    if let Some(cookie) = auth.cookie_header() {\n        request = request.header(reqwest::header::COOKIE, cookie);\n    }\n    let response = tokio::select! {\n        result = request.send() => result.context("CHZZK video detail request failed")?,\n        _ = wait_for_cancel(cancel) => bail!("CHZZK VOD metadata request cancelled"),\n    }\n    .error_for_status()\n    .context("CHZZK video detail HTTP error")?;\n    let value: Value = tokio::select! {\n        result = response.json() => result.context("CHZZK video detail JSON parse failed")?,\n        _ = wait_for_cancel(cancel) => bail!("CHZZK VOD metadata request cancelled"),\n    };\n'''
if old not in text:
    raise RuntimeError('request block anchor missing')
text = text.replace(old, new, 1)

anchor = '''async fn streamlink_quality_options(\n'''
helper = '''async fn wait_for_cancel(cancel: &AtomicBool) {\n    while !cancel.load(Ordering::SeqCst) {\n        tokio::time::sleep(CANCEL_POLL_INTERVAL).await;\n    }\n}\n\n'''
if anchor not in text:
    raise RuntimeError('streamlink quality anchor missing')
text = text.replace(anchor, helper + anchor, 1)

anchor = '''    #[test]\n    fn recognizes_only_chzzk_video_urls() {\n'''
tests = '''    #[test]\n    fn chzzk_api_timeouts_are_bounded() {\n        assert!(CHZZK_API_CONNECT_TIMEOUT <= CHZZK_API_TOTAL_TIMEOUT);\n        assert!(CHZZK_API_TOTAL_TIMEOUT <= Duration::from_secs(30));\n        assert!(CANCEL_POLL_INTERVAL <= Duration::from_millis(100));\n    }\n\n    #[tokio::test]\n    async fn cancellation_waiter_observes_atomic_flag() {\n        let cancel = Arc::new(AtomicBool::new(false));\n        let setter = cancel.clone();\n        let task = tokio::spawn(async move {\n            tokio::time::sleep(Duration::from_millis(20)).await;\n            setter.store(true, Ordering::SeqCst);\n        });\n        tokio::time::timeout(Duration::from_secs(1), wait_for_cancel(cancel.as_ref()))\n            .await\n            .expect("cancellation waiter timed out");\n        task.await.unwrap();\n    }\n\n'''
if anchor not in text:
    raise RuntimeError('test anchor missing')
text = text.replace(anchor, tests + anchor, 1)
vod.write_text(text, encoding='utf-8')

# Update Phase 18 guard for the Codex cancellation/timeout finding.
guard = Path('maintenance/Test-Phase18ChzzkVod.ps1')
g = guard.read_text(encoding='utf-8')
if 'CHZZK_API_TOTAL_TIMEOUT' not in g:
    marker = '# Common VOD model contract: CHZZK is one logical part and still uses queue/history status.\n'
    checks = '''# CHZZK API metadata lookup must be bounded and cancellation-aware.\nAssert-Match $chzzkVod 'Client::builder\\(\\)' 'CHZZK video-detail API client must use an explicit builder.'\nAssert-Match $chzzkVod 'connect_timeout\\(CHZZK_API_CONNECT_TIMEOUT\\)' 'CHZZK video-detail API connect timeout is missing.'\nAssert-Match $chzzkVod '\\.timeout\\(CHZZK_API_TOTAL_TIMEOUT\\)' 'CHZZK video-detail API total timeout is missing.'\nAssert-Match $chzzkVod 'tokio::select!' 'CHZZK video-detail API lookup is not cancellation-raced.'\nAssert-Match $chzzkVod 'wait_for_cancel\\(cancel\\)' 'CHZZK video-detail API cancellation waiter is missing.'\nAssert-Match $chzzkVod 'chzzk_api_timeouts_are_bounded' 'CHZZK API timeout regression test is missing.'\nAssert-Match $chzzkVod 'cancellation_waiter_observes_atomic_flag' 'CHZZK API cancellation regression test is missing.'\n\n'''
    if marker not in g:
        raise RuntimeError('guard marker missing')
    g = g.replace(marker, checks + marker, 1)
    guard.write_text(g, encoding='utf-8')

print('patched CHZZK API timeout/cancellation review finding')
