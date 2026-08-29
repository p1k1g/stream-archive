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
$models = Get-Content -LiteralPath (Join-Path $root 'overlay\Models.cs') -Raw -Encoding UTF8

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
if ($models -notmatch 'CardBackground' -or $models -notmatch 'StateForeground' -or
    $main -notmatch 'Background="\{Binding CardBackground\}"' -or
    $main -notmatch 'Foreground="\{Binding StateForeground\}"') {
    throw 'Enabled/disabled channel visual distinction is missing.'
}
if ($features -notmatch 'PrimaryButtonText\s*=\s*"변경 사항 반영"' -or
    $features -notmatch 'SecondaryButtonText\s*=\s*"확인"' -or
    $features -notmatch 'CloseButtonText\s*=\s*"취소"') {
    throw 'Channel-name preview apply/confirm/cancel actions are not distinct.'
}
if ($main -notmatch 'NavigationItemFix39\("최근 녹화",\s*"recent",\s*Symbol\.Video\)') {
    throw 'Recent-recordings navigation icon is not distinct from the log document icon.'
}
if ($settings -notmatch 'DispatcherQueue\.CreateTimer\(\)' -or
    $settings -notmatch 'settingsUiTransitionDepth\s*=\s*Math\.Max') {
    throw 'Advanced-settings deferred layout dirty guard is missing.'
}

Write-Host 'User feature source invariants passed.'
