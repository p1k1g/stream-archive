from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]


def read(rel: str) -> str:
    return (ROOT / rel).read_text(encoding="utf-8")


def write(rel: str, text: str) -> None:
    path = ROOT / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def replace_regex(text: str, pattern: str, repl: str, label: str) -> str:
    out, count = re.subn(pattern, repl, text, count=1, flags=re.S)
    if count != 1:
        raise RuntimeError(f"expected one replacement for {label}, found {count}")
    return out


# Providers.ps1: preserve all Phase 17 provider contracts while moving them under
# the permanent runtime-contract entry point.
old_provider = read("maintenance/Test-Phase17Chzzk.ps1")
marker = "$platform = Read-RepoFile 'rust-web/src/platform/mod.rs'"
pos = old_provider.find(marker)
if pos < 0:
    raise RuntimeError("Phase 17 provider guard marker not found")
providers = ". (Join-Path $PSScriptRoot 'Common.ps1')\n\n" + old_provider[pos:]
providers = providers.replace(
    "$phase14 = Read-RepoFile 'rust-web/web/phase14.js'\n",
    "$phase14 = Read-RepoFile 'rust-web/web/phase14.js'\n$soopVod = Read-RepoFile 'rust-web/src/platform/soop/vod.rs'\n",
    1,
)
providers = providers.replace(
    "# Provider registration / Phase 17 LIVE boundary. Phase 18 may enable VOD separately.",
    "# Provider registration / CHZZK LIVE boundary. CHZZK VOD may enable VOD separately.",
    1,
)
capability = "Assert-Match $chzzk 'vod:\\s*(?:false|true)' 'CHZZK provider VOD capability field is missing.'\n"
if capability not in providers:
    raise RuntimeError("CHZZK VOD capability guard not found")
providers = providers.replace(
    capability,
    capability + "Assert-Match $soopVod 'tools\\.yt_dlp' 'SOOP VOD must remain yt-dlp based.'\n",
    1,
)
providers = providers.replace(
    "Write-Host 'Phase 17 CHZZK LIVE regression checks passed.'",
    "Write-Host 'Provider contracts passed.'",
    1,
)
write("maintenance/guards/Providers.ps1", providers)

# LIVE recorder: centralize exact-owned process termination in platform_runtime.
recorder = read("rust-web/src/recorder.rs")
if "use crate::platform_runtime::terminate_owned_checked;" not in recorder:
    recorder = "use crate::platform_runtime::terminate_owned_checked;\n" + recorder
recorder = replace_regex(
    recorder,
    r"    pub async fn stop\(&self, rec: &mut Recording\) -> Result<Option<i32>> \{.*?\n    \}\n\n    pub async fn log_finished",
    """    pub async fn stop(&self, rec: &mut Recording) -> Result<Option<i32>> {
        if rec.child.try_wait()?.is_none() {
            return terminate_owned_checked(&mut rec.child)
                .await
                .with_context(|| format!(\"failed to stop recorder pid={}\", rec.pid));
        }
        Ok(rec.child.wait().await.ok().and_then(|s| s.code()))
    }

    pub async fn log_finished""",
    "RecorderManager::stop",
)
write("rust-web/src/recorder.rs", recorder)

# SOOP VOD: keep provider behavior, but route cancellation through the same
# exact-owned process boundary as LIVE and CHZZK VOD.
soop_vod = read("rust-web/src/platform/soop/vod.rs")
if "use crate::platform_runtime::terminate_owned;" not in soop_vod:
    soop_vod = "use crate::platform_runtime::terminate_owned;\n" + soop_vod
soop_vod = replace_regex(
    soop_vod,
    r"async fn stop_child\(child: &mut Child\) \{.*?\n\}\n\nfn collect_set_cookies",
    """async fn stop_child(child: &mut Child) {
    terminate_owned(child).await;
}

fn collect_set_cookies""",
    "SOOP VOD stop_child",
)
write("rust-web/src/platform/soop/vod.rs", soop_vod)

# Historical phase-specific entry points/workflows are fully superseded by the
# permanent runtime contract entry point and rust-web-check workflow.
for rel in [
    ".github/workflows/phase10-2-ui-followup-check.yml",
    ".github/workflows/phase11-watcher-stopped-check.yml",
    ".github/workflows/phase18-chzzk-vod-check.yml",
    "maintenance/Test-Phase15Architecture.ps1",
    "maintenance/Test-ProcessLifecycle.ps1",
    "maintenance/Test-Phase17Chzzk.ps1",
    "maintenance/Test-PublicReleaseSafety.ps1",
    "maintenance/Test-Phase18ChzzkVod.ps1",
]:
    path = ROOT / rel
    if path.exists():
        path.unlink()

print("Applied Phase 19.7 remaining guard/runtime consolidation changes")
