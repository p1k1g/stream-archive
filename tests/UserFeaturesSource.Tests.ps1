$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot -Parent
$backendDir = Join-Path $root 'backend'
$backend = (Get-Content -LiteralPath (Join-Path $backendDir 'SOOP_LIVE.ps1') -Raw -Encoding UTF8) + "`n" +
    ((Get-ChildItem -LiteralPath (Join-Path $backendDir 'modules') -Filter '*.ps1' -File |
        ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw -Encoding UTF8 }) -join "`n")
$main = (Get-ChildItem -LiteralPath (Join-Path $root 'overlay') -Filter 'MainWindow*.cs' -File |
    ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw -Encoding UTF8 }) -join "`n"
$settings = Get-Content -LiteralPath (Join-Path $root 'overlay\SettingsFix36.cs') -Raw -Encoding UTF8
$features = Get-Content -LiteralPath (Join-Path $root 'overlay\UserFeaturesFix51.cs') -Raw -Encoding UTF8
$recentStore = Get-Content -LiteralPath (Join-Path $root 'overlay\RecentRecordingStore.cs') -Raw -Encoding UTF8
$diagnostics = Get-Content -LiteralPath (Join-Path $root 'overlay\DiagnosticInfoService.cs') -Raw -Encoding UTF8

if ($backend -notmatch 'elseif\s*\(\$action\s+-eq\s+"RECHECK"\)') {
    throw 'Per-account RECHECK command handling is missing.'
}
if ($recentStore -notmatch 'AtomicReplace\(json\)') {
    throw 'Recent recording history is not using atomic replacement.'
}
if ($diagnostics -notmatch 'TakeLast\(50\)') {
    throw 'Diagnostic copy is not bounded to the latest 50 GUI events.'
}
if ($diagnostics -notmatch 'worker_api_key\|password\|passwd') {
    throw 'Diagnostic token redaction guard is missing.'
}
if ($settings -notmatch 'settingsUiTransitionDepth\s*==\s*0') {
    throw 'Advanced-settings transition dirty guard is missing.'
}
if ($main -notmatch 'RefreshChannelNamesPreviewFix51_Click') {
    throw 'Channel-name refresh preview action is not wired.'
}

Write-Host 'User feature source invariants passed.'
