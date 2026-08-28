$ErrorActionPreference = 'Stop'

$root = Split-Path $PSScriptRoot -Parent
$backend = Get-Content -LiteralPath (Join-Path $root 'backend\SOOP_LIVE.ps1') -Raw -Encoding UTF8
$main = Get-Content -LiteralPath (Join-Path $root 'overlay\MainWindow.xaml.cs') -Raw -Encoding UTF8
$settings = Get-Content -LiteralPath (Join-Path $root 'overlay\SettingsFix36.cs') -Raw -Encoding UTF8
$features = Get-Content -LiteralPath (Join-Path $root 'overlay\UserFeaturesFix51.cs') -Raw -Encoding UTF8

if ($backend -notmatch 'elseif\s*\(\$action\s+-eq\s+"RECHECK"\)') {
    throw 'Per-account RECHECK command handling is missing.'
}
if ($features -notmatch 'AtomicWriteAllText\(RecentRecordingsPath') {
    throw 'Recent recording history is not using atomic replacement.'
}
if ($features -notmatch 'TakeLast\(50\)') {
    throw 'Diagnostic copy is not bounded to the latest 50 GUI events.'
}
if ($features -notmatch 'worker_api_key\|password\|passwd') {
    throw 'Diagnostic token redaction guard is missing.'
}
if ($settings -notmatch 'settingsUiTransitionDepth\s*==\s*0') {
    throw 'Advanced-settings transition dirty guard is missing.'
}
if ($main -notmatch 'RefreshChannelNamesPreviewFix51_Click') {
    throw 'Channel-name refresh preview action is not wired.'
}

Write-Host 'User feature source invariants passed.'
