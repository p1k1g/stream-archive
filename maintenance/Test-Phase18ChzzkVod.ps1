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
Assert-NotMatch $queue 'api\.chzzk\.naver\.com|NID_AUT|NID_SES' 'Provider-specific CHZZK network/auth logic leaked into vod_queue.rs.'

# Existing Phase 17 encrypted CHZZK auth must be reused; secrets must travel by temp cookie file only.
Assert-Match $chzzkVod 'ChzzkAuth::load\(\)' 'CHZZK VOD does not reuse the encrypted Phase 17 authentication store.'
Assert-Match $chzzkVod 'chzzk-cookies\.txt' 'CHZZK VOD temporary Netscape cookie transport is missing.'
Assert-Match $chzzkVod 'JobDirGuard' 'CHZZK VOD temporary job/cookie cleanup guard is missing.'
Assert-Match $chzzkVod 'NID_\(\?:AUT\|SES\)' 'CHZZK VOD secret redaction coverage is missing.'
Assert-NotMatch $chzzkVod 'NID_AUT=.*--|NID_SES=.*--' 'CHZZK cookies must not be put directly on process arguments.'
Assert-Match $auth 'CHZZK_NID_AUT' 'Shared CHZZK auth key disappeared.'
Assert-Match $auth 'CHZZK_NID_SES' 'Shared CHZZK auth key disappeared.'

# yt-dlp owns CHZZK metadata/download extraction; cancellation must terminate only the owned PID tree.
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

Write-Host 'Phase 18 CHZZK VOD regression checks passed.'
