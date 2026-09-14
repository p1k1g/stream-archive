from pathlib import Path

vod = Path('rust-web/src/platform/chzzk/vod.rs')
text = vod.read_text(encoding='utf-8')
old = '(media_seconds.max(0.0) / duration_seconds as f64 * 95.0).clamp(0.0, 95.0)'
new = '(media_seconds.max(0.0) / duration_seconds as f64 * 100.0).clamp(0.0, 99.0)'
if old not in text:
    raise RuntimeError('progress percent anchor missing')
text = text.replace(old, new, 1)
text = text.replace('    current.percent = 95.0;\n    current.current_part = 1;\n', '    current.percent = 99.0;\n    current.current_part = 1;\n', 1)
text = text.replace('        assert!((percent - 47.5).abs() < 0.001);', '        assert!((percent - 50.0).abs() < 0.001);', 1)
text = text.replace('        assert_eq!(download_progress_percent(99999.0, 29856), 95.0);', '        assert_eq!(download_progress_percent(99999.0, 29856), 99.0);', 1)
vod.write_text(text, encoding='utf-8')

app = Path('rust-web/web/app.js')
js = app.read_text(encoding='utf-8')
old_js = "const parts=(s.analysis.parts||[]).map(p=>`P${p.part}:${p.duration_seconds||0}s`).join(' / ');"
new_js = "const parts=(s.analysis.parts||[]).map(p=>`P${p.part}:${duration(p.duration_seconds)}`).join(' / ');"
if old_js not in js:
    raise RuntimeError('VOD duration UI anchor missing')
js = js.replace(old_js, new_js, 1)
app.write_text(js, encoding='utf-8')

guard = Path('maintenance/Test-Phase18ChzzkVod.ps1')
g = guard.read_text(encoding='utf-8')
g = g.replace(
    '# Phase 18 contract: CHZZK API metadata + Streamlink using the shared LIVE MPEG-TS player pipeline.',
    '# Phase 18 contract: CHZZK API metadata + Streamlink DASH-to-MPEG-TS mux + owned FFmpeg TS finalizer/progress.',
    1,
)
g = g.replace(
    '# CHZZK metadata comes from CHZZK API; Streamlink owns VOD media extraction and uses the LIVE MPEG-TS player pipeline.',
    '# CHZZK metadata comes from CHZZK API; Streamlink owns DASH extraction/mux and the VOD-owned FFmpeg finalizes TS with media progress.',
    1,
)
guard.write_text(g, encoding='utf-8')

print('finalized CHZZK VOD progress and duration display')
