$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot -Parent
$recorder = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\recorder.rs') -Raw -Encoding UTF8
$vodFacade = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\vod.rs') -Raw -Encoding UTF8
$vod = Get-Content -LiteralPath (Join-Path $root 'rust-web\src\platform\soop\vod.rs') -Raw -Encoding UTF8

function Assert-Contains([string]$Text, [string]$Pattern, [string]$Message) {
    if ($Text -notmatch $Pattern) { throw $Message }
}

function Assert-NotContains([string]$Text, [string]$Pattern, [string]$Message) {
    if ($Text -match $Pattern) { throw $Message }
}

# LIVE recorder: unexpected watcher/task drops must not leave its owned Streamlink
# parent alive, while explicit stop uses the exact recorded PID and its child tree.
Assert-Contains $recorder '\.kill_on_drop\(true\)' 'LIVE Streamlink child must use kill_on_drop as a fallback.'
Assert-Contains $recorder 'taskkill\.exe' 'LIVE recorder must use taskkill for Windows tree cleanup.'
Assert-Contains $recorder '\.arg\("/PID"\)' 'LIVE recorder taskkill must target an owned PID.'
Assert-Contains $recorder '\.arg\("/T"\)' 'LIVE recorder taskkill must include the owned process tree.'
Assert-Contains $recorder '\.arg\("/F"\)' 'LIVE recorder taskkill must force termination when stopping.'
Assert-NotContains $recorder '\.arg\("/IM"\)' 'LIVE recorder must never kill processes by image name.'

# Phase 16 keeps the root VOD module as a platform-neutral facade. Process
# ownership remains an implementation concern of each platform VOD provider.
Assert-NotContains $vodFacade 'taskkill\.exe|yt-dlp|ffmpeg|sooplive\.com' 'Root VOD facade must not regain provider/process implementation details.'

# SOOP VOD cancellation: both progress and capture subprocess paths must funnel
# through stop_child(), whose Windows implementation kills only the recorded PID tree.
$stopCalls = [regex]::Matches($vod, 'stop_child\(&mut child\)\.await;').Count
if ($stopCalls -lt 2) { throw "SOOP VOD cancellation must route both subprocess paths through stop_child(); found $stopCalls call(s)." }
Assert-Contains $vod 'async fn stop_child\(child: &mut Child\)' 'SOOP VOD owned-child stop helper is missing.'
Assert-Contains $vod 'taskkill\.exe' 'SOOP VOD stop_child must use taskkill for Windows tree cleanup.'
Assert-Contains $vod '\.arg\("/PID"\)' 'SOOP VOD taskkill must target an owned PID.'
Assert-Contains $vod '\.arg\("/T"\)' 'SOOP VOD taskkill must include the owned process tree.'
Assert-Contains $vod '\.arg\("/F"\)' 'SOOP VOD taskkill must force termination when cancelling.'
Assert-NotContains $vod '\.arg\("/IM"\)' 'SOOP VOD must never kill processes by image name.'

# Non-zero child exits must remain error paths rather than being treated as a
# successful VOD completion.
Assert-Contains $vod 'if !exit\.success\(\)' 'SOOP VOD capture path must reject non-zero child exits.'
Assert-Contains $vod 'if exit\.success\(\)' 'SOOP VOD progress path must distinguish successful child exits.'

Write-Host 'Owned process lifecycle checks passed.'
