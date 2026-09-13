$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot -Parent

function Read-RepoFile([string]$relativePath) {
    $path = Join-Path $root $relativePath
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Missing required Phase 18 file: $relativePath"
    }
    return Get-Content -LiteralPath $path -Raw -Encoding UTF8
}

function Assert-Match([string]$text, [string]$pattern, [string]$message) {
    if ($text -notmatch $pattern) { throw $message }
}

function Assert-NotMatch([string]$text, [string]$pattern, [string]$message) {
    if ($text -match $pattern) { throw $message }
}

$platform = Read-RepoFile 'rust-web/src/platform/mod.rs'
$chzzk = Read-RepoFile 'rust-web/src/platform/chzzk/mod.rs'
$vodFacade = Read-RepoFile 'rust-web/src/platform/vod.rs'
$chzzkVod = Read-RepoFile 'rust-web/src/platform/chzzk/vod.rs'
$queue = Read-RepoFile 'rust-web/src/vod_queue.rs'
$auth = Read-RepoFile 'rust-web/src/platform/chzzk/auth.rs'

# Platform registration / URL routing.
Assert-Match $chzzk 'pub mod vod;' 'CHZZK VOD provider module is not registered.'
Assert-Match $chzzk 'vod:\s*true' 'CHZZK VOD capability is not enabled.'
Assert-Match $chzzk 'chzzk\.naver\.com' 'CHZZK provider does not validate the VOD host.'
Assert-Match $chzzk 'parts\[0\] == "video"' 'CHZZK /video/<id> URL recognition is missing.'
Assert-Match $platform 'detects_soop_and_chzzk_vod_urls' 'Platform VOD routing regression coverage is missing.'

# Common queue/API orchestration must dispatch through the provider-neutral facade.
Assert-Match $vodFacade 'chzzk:\s*chzzk::vod::VodManager' 'Common VOD facade does not own a CHZZK provider manager.'
Assert-Match $vodFacade 'PlatformId::Chzzk\s*=>\s*self\.chzzk\.analyze' 'CHZZK analyze is not routed through the common VOD facade.'
Assert-Match $vodFacade 'PlatformId::Chzzk\s*=>\s*self\.chzzk\.download' 'CHZZK download is not routed through the common VOD facade.'
Assert-Match $vodFacade 'PlatformId::Chzzk\s*=>\s*chzzk::vod::validate_download_request' 'CHZZK queue validation is not provider-routed.'
Assert-Match $vodFacade 'lifecycle:\s*Mutex<\(\)>' 'Common VOD facade does not serialize provider starts.'
Assert-Match $vodFacade 'ensure_idle\(\)\.await\?' 'Cross-provider VOD start guard is missing.'
Assert-Match $vodFacade 'running_provider\(\)' 'VOD status/cancel cannot recover the actually running provider.'
Assert-NotMatch $queue 'api\.chzzk\.naver\.com|NID_AUT|NID_SES' 'Provider-specific CHZZK network/auth logic leaked into vod_queue.rs.'

# Existing Phase 17 encrypted CHZZK auth must be reused; plaintext temp data is private and scavenged.
Assert-Match $chzzkVod 'ChzzkAuth::load\(\)' 'CHZZK VOD does not reuse the encrypted Phase 17 authentication store.'
Assert-Match $chzzkVod 'chzzk-cookies\.txt' 'CHZZK VOD temporary Netscape cookie transport is missing.'
Assert-Match $chzzkVod 'JobDirGuard' 'CHZZK VOD temporary job/cookie cleanup guard is missing.'
Assert-Match $chzzkVod 'cleanup_stale_job_dirs' 'Stale CHZZK VOD job directory scavenging is missing.'
Assert-Match $chzzkVod 'restrict_job_dir' 'CHZZK VOD temporary directory permission hardening is missing.'
Assert-Match $chzzkVod 'icacls\.exe' 'Windows current-user-only CHZZK temp ACL setup is missing.'
Assert-Match $chzzkVod 'NID_\(\?:AUT\|SES\)' 'CHZZK VOD secret redaction coverage is missing.'
Assert-NotMatch $chzzkVod 'NID_AUT=.*--|NID_SES=.*--' 'CHZZK cookies must not be put directly on process arguments.'
Assert-Match $auth 'CHZZK_NID_AUT' 'Shared CHZZK auth key disappeared.'
Assert-Match $auth 'CHZZK_NID_SES' 'Shared CHZZK auth key disappeared.'

# yt-dlp owns CHZZK extraction, but long user titles must never become HLS fragment temp paths.
Assert-Match $chzzkVod 'MEDIA_FILE_NAME:\s*&str\s*=\s*"media\.mp4"' 'CHZZK VOD short staging filename is missing.'
Assert-Match $chzzkVod 'staging_output\s*=\s*job_dir\.join\(MEDIA_FILE_NAME\)' 'CHZZK VOD does not download into its private short staging path.'
Assert-Match $chzzkVod 'finalize_output\(&staged_file,\s*&final_output\)' 'CHZZK VOD staging output is not finalized into the requested output directory.'
Assert-Match $chzzkVod 'cleanup_job_media' 'CHZZK VOD retry/cancel cleanup is not scoped to the owned job directory.'
Assert-Match $chzzkVod 'PYTHONUTF8' 'yt-dlp UTF-8 child-process environment is missing.'
Assert-Match $chzzkVod 'PYTHONIOENCODING' 'yt-dlp output encoding is not forced to UTF-8.'
Assert-Match $chzzkVod '"--dump-single-json"' 'CHZZK VOD metadata extraction is missing.'
Assert-Match $chzzkVod '"--progress-template"' 'CHZZK VOD download progress integration is missing.'
Assert-Match $chzzkVod '"--merge-output-format"' 'CHZZK VOD MP4 output policy is missing.'
Assert-Match $chzzkVod 'taskkill\.exe' 'Windows owned-process cancellation path is missing.'
Assert-Match $chzzkVod '\.arg\("/PID"\)' 'CHZZK VOD cancellation is not PID scoped.'
Assert-Match $chzzkVod '\.arg\("/T"\)' 'CHZZK VOD cancellation does not include the owned child tree.'
Assert-NotMatch $chzzkVod 'taskkill[^\r\n]*/IM' 'CHZZK VOD must never kill processes by image name.'

# Common VOD model contract: CHZZK is one logical part and still uses queue/history status.
Assert-Match $chzzkVod 'part_count:\s*1' 'CHZZK VOD analysis must expose one logical part.'
Assert-Match $chzzkVod 'current\.platform = PlatformId::Chzzk' 'CHZZK VOD runtime status does not preserve platform identity.'
Assert-Match $chzzkVod 'recognizes_only_chzzk_video_urls' 'CHZZK VOD URL regression test is missing.'
Assert-Match $chzzkVod 'metadata_json_maps_to_common_analysis_shape' 'CHZZK metadata mapping regression test is missing.'
Assert-Match $chzzkVod 'validates_single_part_download_contract' 'CHZZK queue request regression test is missing.'
Assert-Match $chzzkVod 'stale_chzzk_job_dirs_are_scavenged_only' 'CHZZK stale-job cleanup regression test is missing.'
Assert-Match $chzzkVod 'cleanup_is_scoped_to_unique_job_directory' 'CHZZK owned-temp cleanup regression test is missing.'
Assert-Match $chzzkVod 'yt_dlp_staging_name_is_short_and_title_independent' 'CHZZK Windows long-path regression test is missing.'

Write-Host 'Phase 18 CHZZK VOD regression checks passed.'
