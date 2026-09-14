from pathlib import Path
import os
import subprocess
import tempfile

url = "https://chzzk.naver.com/video/15185683"
streamlink = Path(r"C:\Program Files\Streamlink\bin\streamlink.exe")
ffmpeg = Path(r"C:\Program Files\Streamlink\ffmpeg\ffmpeg.exe")
if not streamlink.is_file():
    raise SystemExit("streamlink missing")
if not ffmpeg.is_file():
    raise SystemExit("ffmpeg missing")

root = Path(tempfile.gettempdir())
out = root / "chzzk-pipe-diagnose.ts"
sl_log = root / "chzzk-pipe-streamlink.log"
ff_log = root / "chzzk-pipe-ffmpeg.log"
for path in (out, sl_log, ff_log):
    try:
        path.unlink()
    except FileNotFoundError:
        pass

env = os.environ.copy()
env["PYTHONUTF8"] = "1"
env["PYTHONIOENCODING"] = "utf-8"
sl_args = [
    str(streamlink), "--no-config", "--loglevel", "info",
    "--progress", "no", "--stream-segmented-duration", "30",
    "--stream-segment-threads", "3", "--ffmpeg-ffmpeg", str(ffmpeg),
    "--stdout", url, "best",
]
ff_args = [
    str(ffmpeg), "-hide_banner", "-loglevel", "warning", "-fflags", "+genpts+discardcorrupt",
    "-i", "pipe:0", "-map", "0:v:0?", "-map", "0:a:0?", "-c", "copy",
    "-bsf:v", "h264_mp4toannexb", "-f", "mpegts", "-mpegts_flags", "resend_headers",
    "-mpegts_copyts", "0", "-avoid_negative_ts", "make_zero", "-muxpreload", "0",
    "-muxdelay", "0", "-y", str(out),
]
with sl_log.open("wb") as slerr, ff_log.open("wb") as fferr:
    sl = subprocess.Popen(sl_args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=slerr, env=env)
    assert sl.stdout is not None
    ff = subprocess.Popen(ff_args, stdin=sl.stdout, stdout=subprocess.DEVNULL, stderr=fferr)
    sl.stdout.close()
    sl_rc = sl.wait(timeout=120)
    ff_rc = ff.wait(timeout=120)

print(f"streamlink_rc={sl_rc}")
print(f"ffmpeg_rc={ff_rc}")
print(f"output_exists={out.is_file()}")
print(f"output_size={out.stat().st_size if out.is_file() else 0}")
print("streamlink_tail=" + sl_log.read_text(encoding="utf-8", errors="replace")[-2000:].replace("\n", " | "))
print("ffmpeg_tail=" + ff_log.read_text(encoding="utf-8", errors="replace")[-2000:].replace("\n", " | "))
if sl_rc != 0 or ff_rc != 0 or not out.is_file() or out.stat().st_size < 100_000:
    raise SystemExit("pipe validation failed")
with out.open("rb") as fh:
    first = fh.read(1)
print("first_byte=" + first.hex())
if first != b"\x47":
    raise SystemExit("output is not MPEG-TS sync byte")
