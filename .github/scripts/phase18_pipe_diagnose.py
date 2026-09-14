from pathlib import Path
import os
import subprocess
import tempfile
import time

url = "https://chzzk.naver.com/video/15185683"
streamlink = Path(r"C:\Program Files\Streamlink\bin\streamlink.exe")
ffmpeg = Path(r"C:\Program Files\Streamlink\ffmpeg\ffmpeg.exe")
if not streamlink.is_file():
    raise SystemExit("streamlink missing")
if not ffmpeg.is_file():
    raise SystemExit("ffmpeg missing")

root = Path(tempfile.gettempdir())
env = os.environ.copy()
env["PYTHONUTF8"] = "1"
env["PYTHONIOENCODING"] = "utf-8"


def kill_tree(pid: int) -> None:
    subprocess.run(["taskkill.exe", "/PID", str(pid), "/T", "/F"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def inspect(label: str, out: Path, log_paths: list[Path]) -> bool:
    size = out.stat().st_size if out.is_file() else 0
    first = b""
    if out.is_file() and size:
        with out.open("rb") as fh:
            first = fh.read(1)
    print(f"{label}.exists={out.is_file()}")
    print(f"{label}.size={size}")
    print(f"{label}.first_byte={first.hex() if first else 'none'}")
    for log in log_paths:
        print(log.name + "_tail=" + log.read_text(encoding="utf-8", errors="replace")[-1600:].replace("\n", " | "))
    return out.is_file() and size > 100_000 and first == b"\x47"


# A: Streamlink file output without segmented-duration. Let it fetch for 12 wall-clock seconds.
direct = root / "chzzk-direct-livecut.ts"
direct_log = root / "chzzk-direct-livecut.log"
for p in (direct, direct_log):
    try: p.unlink()
    except FileNotFoundError: pass
with direct_log.open("wb") as err:
    sl = subprocess.Popen([
        str(streamlink), "--no-config", "--loglevel", "info", "--progress", "no",
        "--stream-segment-threads", "3", "--ffmpeg-ffmpeg", str(ffmpeg),
        "--output", str(direct), "--force", url, "best",
    ], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=err, env=env)
    time.sleep(12)
    kill_tree(sl.pid)
    sl.wait(timeout=30)
direct_ok = inspect("direct", direct, [direct_log])

# B: Streamlink stdout -> owned FFmpeg, same 12 wall-clock seconds.
pipe_out = root / "chzzk-pipe-livecut.ts"
sl_log = root / "chzzk-pipe-livecut-streamlink.log"
ff_log = root / "chzzk-pipe-livecut-ffmpeg.log"
for p in (pipe_out, sl_log, ff_log):
    try: p.unlink()
    except FileNotFoundError: pass
with sl_log.open("wb") as slerr, ff_log.open("wb") as fferr:
    sl = subprocess.Popen([
        str(streamlink), "--no-config", "--loglevel", "info", "--progress", "no",
        "--stream-segment-threads", "3", "--ffmpeg-ffmpeg", str(ffmpeg),
        "--stdout", url, "best",
    ], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=slerr, env=env)
    assert sl.stdout is not None
    ff = subprocess.Popen([
        str(ffmpeg), "-hide_banner", "-loglevel", "warning", "-fflags", "+genpts+discardcorrupt",
        "-i", "pipe:0", "-map", "0:v:0?", "-map", "0:a:0?", "-c", "copy",
        "-bsf:v", "h264_mp4toannexb", "-f", "mpegts", "-mpegts_flags", "resend_headers",
        "-mpegts_copyts", "0", "-avoid_negative_ts", "make_zero", "-muxpreload", "0",
        "-muxdelay", "0", "-y", str(pipe_out),
    ], stdin=sl.stdout, stdout=subprocess.DEVNULL, stderr=fferr)
    sl.stdout.close()
    time.sleep(12)
    kill_tree(sl.pid)
    sl.wait(timeout=30)
    try:
        ff.wait(timeout=20)
    except subprocess.TimeoutExpired:
        kill_tree(ff.pid)
        ff.wait(timeout=20)
pipe_ok = inspect("pipe", pipe_out, [sl_log, ff_log])

print(f"direct_ok={direct_ok}")
print(f"pipe_ok={pipe_ok}")
if not (direct_ok or pipe_ok):
    raise SystemExit("neither media path produced a valid MPEG-TS sample")
